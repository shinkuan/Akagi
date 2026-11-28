//! Cleartext HTTP/2 (h2c, prior knowledge) tunnels: a passive frame tap and
//! the relay that feeds it.
//!
//! After a `CONNECT`, hudsucker only understands HTTP/1 and TLS; any other
//! first bytes are tunneled blind. HoloMahjong's battle server speaks gRPC
//! over h2c, so for that platform the proxy relays tunnels itself
//! ([`relay`]) and runs both directions through [`H2Tap`], which turns the
//! byte stream back into complete gRPC messages. A tap that loses track only
//! stops decoding — the tunnel keeps relaying.
//!
//! Bytes go on a whole frame at a time, so the relay can also add messages
//! of its own to a client stream (a [`Splice`], for autoplay): as a DATA
//! frame between two of the client's frames, never inside one of its gRPC
//! messages, and only within the window the server granted. What it spends
//! of that window is taken back out of the server's WINDOW_UPDATEs (see
//! `Flow`), so the client's flow control stays true.

use crate::bridge::Direction;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{broadcast, Notify};
use tracing::warn;

const LOG: &str = "akagi::proxy::forward";

/// The client connection preface (RFC 9113 §3.4).
pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

const FRAME_HEADER_LEN: usize = 9;
const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const RST_STREAM: u8 = 0x3;
const SETTINGS: u8 = 0x4;
const PUSH_PROMISE: u8 = 0x5;
const WINDOW_UPDATE: u8 = 0x8;
const CONTINUATION: u8 = 0x9;
const FLAG_END_STREAM: u8 = 0x1;
const FLAG_ACK: u8 = 0x1;
const FLAG_END_HEADERS: u8 = 0x4;
const FLAG_PADDED: u8 = 0x8;
const FLAG_PRIORITY: u8 = 0x20;
/// gRPC length-prefixed message header: compressed flag + u32 length.
const GRPC_PREFIX_LEN: usize = 5;
/// A stream buffering more than this without completing a message is not gRPC.
const MAX_MESSAGE_LEN: usize = 16 * 1024 * 1024;
/// Ceiling on a header block spread over CONTINUATION frames.
const MAX_HEADER_BLOCK: usize = 1024 * 1024;
const SETTINGS_INITIAL_WINDOW_SIZE: u16 = 0x4;
const SETTINGS_MAX_FRAME_SIZE: u16 = 0x5;
/// RFC 9113's initial window, connection and streams alike.
const DEFAULT_WINDOW: i64 = 65_535;
const DEFAULT_MAX_FRAME: usize = 16_384;
/// A spliced message that cannot go out this soon is dropped, not sent late.
const SPLICE_STALE: Duration = Duration::from_secs(2);
const SPLICE_QUEUE: usize = 8;

/// Whether a tunnel's first client bytes open an h2c connection. Needs at
/// least the 4 bytes hudsucker itself peeks at.
pub fn looks_like_h2c(first: &[u8]) -> bool {
    first.len() >= 4 && PREFACE.starts_with(&first[..first.len().min(PREFACE.len())])
        || first.starts_with(PREFACE)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TapEvent {
    /// A complete header block: request headers upstream, response headers or
    /// trailers downstream.
    Headers {
        stream: u32,
        dir: Direction,
        headers: Vec<(String, String)>,
    },
    /// One gRPC message. `path` is the stream's request `:path` when the
    /// request headers were decoded.
    Message {
        stream: u32,
        dir: Direction,
        path: Option<String>,
        compressed: bool,
        payload: Vec<u8>,
    },
    /// The tap stopped following this direction (bytes still relay).
    Desync { dir: Direction, reason: String },
    /// A message the relay itself put on a client stream (see [`Splice`]).
    Injected {
        stream: u32,
        path: String,
        payload: Vec<u8>,
    },
}

struct Side {
    buf: Vec<u8>,
    need_preface: bool,
    hpack: loona_hpack::Decoder<'static>,
    hpack_broken: bool,
    /// Header block awaiting CONTINUATION frames: (stream, fragments so far).
    block: Option<(u32, Vec<u8>)>,
    grpc: HashMap<u32, Vec<u8>>,
    dead: bool,
}

impl Side {
    fn new(need_preface: bool) -> Self {
        Self {
            buf: Vec::new(),
            need_preface,
            hpack: loona_hpack::Decoder::new(),
            hpack_broken: false,
            block: None,
            grpc: HashMap::new(),
            dead: false,
        }
    }
}

/// Incremental parser for one h2c connection, fed both directions.
pub struct H2Tap {
    up: Side,
    down: Side,
    paths: HashMap<u32, String>,
}

impl Default for H2Tap {
    fn default() -> Self {
        Self {
            up: Side::new(true),
            down: Side::new(false),
            paths: HashMap::new(),
        }
    }
}

impl H2Tap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes seen in `dir`; returns every event they completed.
    pub fn feed(&mut self, dir: Direction, data: &[u8]) -> Vec<TapEvent> {
        let mut out = Vec::new();
        let side = match dir {
            Direction::Up => &mut self.up,
            Direction::Down => &mut self.down,
        };
        if side.dead {
            return out;
        }
        side.buf.extend_from_slice(data);
        if side.need_preface {
            if side.buf.len() < PREFACE.len() {
                if !PREFACE.starts_with(&side.buf) {
                    return desync(side, dir, "missing client preface", out);
                }
                return out;
            }
            if !side.buf.starts_with(PREFACE) {
                return desync(side, dir, "missing client preface", out);
            }
            side.buf.drain(..PREFACE.len());
            side.need_preface = false;
        }
        // Frames are parsed from a cursor and the buffer drained once, so a
        // read holding many small frames stays linear.
        let mut pos = 0;
        loop {
            let side = match dir {
                Direction::Up => &mut self.up,
                Direction::Down => &mut self.down,
            };
            let avail = &side.buf[pos..];
            if avail.len() < FRAME_HEADER_LEN {
                break;
            }
            let len = u32::from_be_bytes([0, avail[0], avail[1], avail[2]]) as usize;
            if avail.len() < FRAME_HEADER_LEN + len {
                break;
            }
            let ftype = avail[3];
            let flags = avail[4];
            let stream = u32::from_be_bytes([avail[5], avail[6], avail[7], avail[8]]) & 0x7FFF_FFFF;
            let payload: Vec<u8> = avail[FRAME_HEADER_LEN..FRAME_HEADER_LEN + len].to_vec();
            pos += FRAME_HEADER_LEN + len;
            if let Err(reason) = self.frame(dir, ftype, flags, stream, &payload, &mut out) {
                let side = match dir {
                    Direction::Up => &mut self.up,
                    Direction::Down => &mut self.down,
                };
                return desync(side, dir, &reason, out);
            }
        }
        let side = match dir {
            Direction::Up => &mut self.up,
            Direction::Down => &mut self.down,
        };
        side.buf.drain(..pos);
        out
    }

    fn frame(
        &mut self,
        dir: Direction,
        ftype: u8,
        flags: u8,
        stream: u32,
        payload: &[u8],
        out: &mut Vec<TapEvent>,
    ) -> Result<(), String> {
        let side = match dir {
            Direction::Up => &mut self.up,
            Direction::Down => &mut self.down,
        };
        if let Some((open, _)) = &side.block {
            if ftype != CONTINUATION || *open != stream {
                return Err(format!("frame type {ftype} interrupted a header block"));
            }
        }
        match ftype {
            DATA => {
                let data = strip_padding(flags, payload)?;
                let buf = side.grpc.entry(stream).or_default();
                buf.extend_from_slice(data);
                while buf.len() >= GRPC_PREFIX_LEN {
                    let mlen = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
                    if mlen > MAX_MESSAGE_LEN {
                        return Err(format!("gRPC message length {mlen} on stream {stream}"));
                    }
                    if buf.len() < GRPC_PREFIX_LEN + mlen {
                        break;
                    }
                    let compressed = buf[0] == 1;
                    let msg: Vec<u8> = buf[GRPC_PREFIX_LEN..GRPC_PREFIX_LEN + mlen].to_vec();
                    buf.drain(..GRPC_PREFIX_LEN + mlen);
                    out.push(TapEvent::Message {
                        stream,
                        dir,
                        path: self.paths.get(&stream).cloned(),
                        compressed,
                        payload: msg,
                    });
                }
                if flags & FLAG_END_STREAM != 0 {
                    self.end_stream(dir, stream);
                }
            }
            HEADERS | PUSH_PROMISE => {
                let mut frag = strip_padding(flags, payload)?;
                if ftype == HEADERS && flags & FLAG_PRIORITY != 0 {
                    frag = frag.get(5..).ok_or("short PRIORITY block")?;
                }
                if ftype == PUSH_PROMISE {
                    frag = frag.get(4..).ok_or("short PUSH_PROMISE")?;
                }
                if flags & FLAG_END_HEADERS != 0 {
                    let block = frag.to_vec();
                    self.header_block(dir, stream, &block, out);
                } else {
                    side.block = Some((stream, frag.to_vec()));
                }
                if ftype == HEADERS && flags & FLAG_END_STREAM != 0 {
                    self.end_stream(dir, stream);
                }
            }
            CONTINUATION => {
                let Some((_, mut block)) = side.block.take() else {
                    return Err("CONTINUATION without a header block".into());
                };
                block.extend_from_slice(payload);
                if block.len() > MAX_HEADER_BLOCK {
                    return Err(format!("header block over {MAX_HEADER_BLOCK} bytes"));
                }
                if flags & FLAG_END_HEADERS != 0 {
                    self.header_block(dir, stream, &block, out);
                } else {
                    side.block = Some((stream, block));
                }
            }
            RST_STREAM => {
                self.up.grpc.remove(&stream);
                self.down.grpc.remove(&stream);
                self.paths.remove(&stream);
            }
            _ => {}
        }
        Ok(())
    }

    /// One side finished a stream. The path is kept until the server's side
    /// ends, since responses still need it.
    fn end_stream(&mut self, dir: Direction, stream: u32) {
        match dir {
            Direction::Up => {
                self.up.grpc.remove(&stream);
            }
            Direction::Down => {
                self.down.grpc.remove(&stream);
                self.paths.remove(&stream);
            }
        }
    }

    fn header_block(&mut self, dir: Direction, stream: u32, block: &[u8], out: &mut Vec<TapEvent>) {
        let side = match dir {
            Direction::Up => &mut self.up,
            Direction::Down => &mut self.down,
        };
        // HPACK state is per connection direction: once one block fails, every
        // later one is unreadable, but DATA framing does not depend on it.
        if side.hpack_broken {
            return;
        }
        match side.hpack.decode(block) {
            Ok(list) => {
                let headers: Vec<(String, String)> = list
                    .into_iter()
                    .map(|(n, v)| {
                        (
                            String::from_utf8_lossy(&n).into_owned(),
                            String::from_utf8_lossy(&v).into_owned(),
                        )
                    })
                    .collect();
                if dir == Direction::Up {
                    if let Some((_, p)) = headers.iter().find(|(n, _)| n == ":path") {
                        self.paths.insert(stream, p.clone());
                    }
                }
                out.push(TapEvent::Headers {
                    stream,
                    dir,
                    headers,
                });
            }
            Err(e) => {
                side.hpack_broken = true;
                out.push(TapEvent::Desync {
                    dir,
                    reason: format!("HPACK: {e}"),
                });
            }
        }
    }
}

fn strip_padding(flags: u8, payload: &[u8]) -> Result<&[u8], String> {
    if flags & FLAG_PADDED == 0 {
        return Ok(payload);
    }
    let pad = *payload.first().ok_or("empty padded frame")? as usize;
    if 1 + pad > payload.len() {
        return Err("padding longer than frame".into());
    }
    Ok(&payload[1..payload.len() - pad])
}

fn desync(side: &mut Side, dir: Direction, reason: &str, mut out: Vec<TapEvent>) -> Vec<TapEvent> {
    side.dead = true;
    side.buf.clear();
    side.grpc.clear();
    side.block = None;
    out.push(TapEvent::Desync {
        dir,
        reason: reason.to_string(),
    });
    out
}

/// Messages to put on one gRPC stream of the connection as if the client had
/// sent them: the newest stream the client opened on `path`. Every relay may
/// subscribe to the same channel; only the one `owner` names writes, and a
/// relay names itself when its client opens such a stream.
pub struct Splice {
    pub path: String,
    /// Unframed gRPC message payloads.
    pub rx: broadcast::Receiver<Vec<u8>>,
    pub owner: Arc<AtomicU64>,
    /// This relay's id; never 0.
    pub id: u64,
}

impl Splice {
    fn claim(&self) {
        self.owner.store(self.id, Ordering::Release);
    }

    fn owns(&self) -> bool {
        self.owner.load(Ordering::Acquire) == self.id
    }

    fn release(&self) {
        let _ = self
            .owner
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire);
    }
}

enum Piece {
    /// The preface, or anything once a direction is not HTTP/2.
    Bytes(Vec<u8>),
    /// One whole frame, header included.
    Frame(Vec<u8>),
}

/// Cuts one direction at frame boundaries, so whatever the relay adds or
/// changes goes between whole frames.
struct Framer {
    buf: Vec<u8>,
    preface: bool,
    /// Each side's first frame is SETTINGS (RFC 9113 §3.4); anything else
    /// means this is not HTTP/2.
    settings_first: bool,
    /// Not HTTP/2 after all: bytes pass as they come.
    raw: bool,
}

impl Framer {
    fn new(preface: bool) -> Self {
        Self {
            buf: Vec::new(),
            preface,
            settings_first: true,
            raw: false,
        }
    }

    fn push(&mut self, data: &[u8]) -> Vec<Piece> {
        if self.raw {
            return vec![Piece::Bytes(data.to_vec())];
        }
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        let mut pos = 0;
        if self.preface {
            let n = self.buf.len().min(PREFACE.len());
            if self.buf[..n] != PREFACE[..n] {
                return self.give_up(out, 0);
            }
            if n < PREFACE.len() {
                return out;
            }
            out.push(Piece::Bytes(PREFACE.to_vec()));
            pos = PREFACE.len();
            self.preface = false;
        }
        while self.buf.len() - pos >= FRAME_HEADER_LEN {
            let h = &self.buf[pos..];
            if std::mem::take(&mut self.settings_first) && (h[3] != SETTINGS || h[5..9] != [0; 4]) {
                return self.give_up(out, pos);
            }
            let end = FRAME_HEADER_LEN + u32::from_be_bytes([0, h[0], h[1], h[2]]) as usize;
            if h.len() < end {
                break;
            }
            out.push(Piece::Frame(h[..end].to_vec()));
            pos += end;
        }
        self.buf.drain(..pos);
        out
    }

    /// Stop framing: everything from `pos` on passes as it is.
    fn give_up(&mut self, mut out: Vec<Piece>, pos: usize) -> Vec<Piece> {
        self.raw = true;
        out.push(Piece::Bytes(self.buf.split_off(pos)));
        self.buf.clear();
        out
    }

    /// Whatever is left of an unfinished frame, at EOF.
    fn rest(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buf)
    }
}

/// `(payload length, type, flags, stream)` of a whole frame.
fn header(f: &[u8]) -> (usize, u8, u8, u32) {
    (
        u32::from_be_bytes([0, f[0], f[1], f[2]]) as usize,
        f[3],
        f[4],
        u32::from_be_bytes([f[5], f[6], f[7], f[8]]) & 0x7FFF_FFFF,
    )
}

/// The client's send windows as the server grants them (RFC 9113 §6.9),
/// spliced bytes included. Those spend the server's window behind the
/// client's back, so they are owed back: taken out of the server's next
/// WINDOW_UPDATEs before they reach the client, which then never counts on
/// window that is gone.
struct Flow {
    conn: i64,
    /// The server's SETTINGS_INITIAL_WINDOW_SIZE.
    initial: i64,
    /// The server's SETTINGS_MAX_FRAME_SIZE.
    max_frame: usize,
    /// Windows of the streams the client can still send on.
    streams: HashMap<u32, i64>,
    owed_conn: u32,
    owed: HashMap<u32, u32>,
}

impl Default for Flow {
    fn default() -> Self {
        Self {
            conn: DEFAULT_WINDOW,
            initial: DEFAULT_WINDOW,
            max_frame: DEFAULT_MAX_FRAME,
            streams: HashMap::new(),
            owed_conn: 0,
            owed: HashMap::new(),
        }
    }
}

impl Flow {
    fn client_frame(&mut self, f: &[u8]) {
        let (len, ftype, flags, stream) = header(f);
        match ftype {
            HEADERS => {
                self.streams.entry(stream).or_insert(self.initial);
            }
            DATA => {
                self.conn -= len as i64;
                if let Some(w) = self.streams.get_mut(&stream) {
                    *w -= len as i64;
                }
            }
            RST_STREAM => self.forget(stream),
            _ => {}
        }
        if matches!(ftype, HEADERS | DATA) && flags & FLAG_END_STREAM != 0 {
            self.forget(stream);
        }
    }

    /// A whole server frame; `None` when it has to be swallowed.
    fn server_frame(&mut self, mut f: Vec<u8>) -> Option<Vec<u8>> {
        let (len, ftype, flags, stream) = header(&f);
        match ftype {
            SETTINGS if flags & FLAG_ACK == 0 => {
                for s in f[FRAME_HEADER_LEN..].as_chunks::<6>().0 {
                    let value = u32::from_be_bytes([s[2], s[3], s[4], s[5]]);
                    match u16::from_be_bytes([s[0], s[1]]) {
                        SETTINGS_INITIAL_WINDOW_SIZE => {
                            let delta = i64::from(value) - self.initial;
                            self.streams.values_mut().for_each(|w| *w += delta);
                            self.initial = i64::from(value);
                        }
                        SETTINGS_MAX_FRAME_SIZE => self.max_frame = value as usize,
                        _ => {}
                    }
                }
            }
            RST_STREAM => self.forget(stream),
            WINDOW_UPDATE if len == 4 => {
                let p = &f[FRAME_HEADER_LEN..];
                let inc = u32::from_be_bytes([p[0], p[1], p[2], p[3]]) & 0x7FFF_FFFF;
                let owed = if stream == 0 {
                    self.conn += i64::from(inc);
                    &mut self.owed_conn
                } else if let Some(w) = self.streams.get_mut(&stream) {
                    *w += i64::from(inc);
                    self.owed.entry(stream).or_default()
                } else {
                    return Some(f);
                };
                let paid = (*owed).min(inc);
                *owed -= paid;
                if paid == inc {
                    return (paid == 0).then_some(f);
                }
                f[FRAME_HEADER_LEN..].copy_from_slice(&(inc - paid).to_be_bytes());
            }
            _ => {}
        }
        Some(f)
    }

    /// Spend `len` bytes of `stream`'s window on a spliced frame, if it has them.
    fn take(&mut self, stream: u32, len: usize) -> bool {
        let n = len as i64;
        let Some(w) = self.streams.get_mut(&stream) else {
            return false;
        };
        if len > self.max_frame || *w < n || self.conn < n {
            return false;
        }
        *w -= n;
        self.conn -= n;
        self.owed_conn += len as u32;
        *self.owed.entry(stream).or_default() += len as u32;
        true
    }

    fn forget(&mut self, stream: u32) {
        self.streams.remove(&stream);
        self.owed.remove(&stream);
    }
}

/// One relayed connection: the tap, each direction's framing, the flow
/// accounting, and spliced messages waiting for their turn.
struct Conn {
    tap: H2Tap,
    up: Framer,
    down: Framer,
    flow: Flow,
    path: Option<String>,
    queue: VecDeque<(Instant, Vec<u8>)>,
}

impl Conn {
    fn new(path: Option<String>) -> Self {
        Self {
            tap: H2Tap::new(),
            up: Framer::new(true),
            down: Framer::new(false),
            flow: Flow::default(),
            path,
            queue: VecDeque::new(),
        }
    }

    /// Client bytes in; the bytes for the server, and what the tap saw.
    fn client_bytes(&mut self, data: &[u8]) -> (Vec<u8>, Vec<TapEvent>) {
        let mut events = self.tap.feed(Direction::Up, data);
        let mut out = Vec::with_capacity(data.len());
        for piece in self.up.push(data) {
            match piece {
                Piece::Bytes(b) => out.extend(b),
                Piece::Frame(f) => {
                    self.flow.client_frame(&f);
                    out.extend(f);
                }
            }
        }
        self.flush(&mut out, &mut events);
        (out, events)
    }

    /// Server bytes in; the bytes for the client, and what the tap saw.
    fn server_bytes(&mut self, data: &[u8]) -> (Vec<u8>, Vec<TapEvent>) {
        let events = self.tap.feed(Direction::Down, data);
        let mut out = Vec::with_capacity(data.len());
        for piece in self.down.push(data) {
            match piece {
                Piece::Bytes(b) => out.extend(b),
                Piece::Frame(f) => {
                    if let Some(f) = self.flow.server_frame(f) {
                        out.extend(f);
                    }
                }
            }
        }
        (out, events)
    }

    /// Queue a message for the spliced stream; the bytes it can go out as now.
    fn splice(&mut self, message: Vec<u8>) -> (Vec<u8>, Vec<TapEvent>) {
        if self.queue.len() >= SPLICE_QUEUE {
            warn!(target: LOG, "splice queue full; dropping the oldest message");
            self.queue.pop_front();
        }
        self.queue.push_back((Instant::now(), message));
        let (mut out, mut events) = (Vec::new(), Vec::new());
        self.flush(&mut out, &mut events);
        (out, events)
    }

    /// The newest stream the client opened on the splice path and can still send on.
    fn target(&self) -> Option<u32> {
        let path = self.path.as_deref()?;
        self.tap
            .paths
            .iter()
            .filter(|(s, p)| p.as_str() == path && self.flow.streams.contains_key(s))
            .map(|(s, _)| *s)
            .max()
    }

    fn flush(&mut self, out: &mut Vec<u8>, events: &mut Vec<TapEvent>) {
        while let Some((queued_at, message)) = self.queue.front() {
            if queued_at.elapsed() > SPLICE_STALE {
                warn!(target: LOG, "a spliced message waited too long; dropped");
                self.queue.pop_front();
                continue;
            }
            if self.up.raw || self.down.raw || self.tap.up.dead || self.tap.down.dead {
                warn!(target: LOG, "the connection is no longer followed; dropping spliced messages");
                self.queue.clear();
                return;
            }
            let Some(stream) = self.target() else {
                warn!(target: LOG, "no open stream to splice into; dropped");
                self.queue.pop_front();
                continue;
            };
            // Between the client's messages, never inside one, and never
            // between a header block's frames (RFC 9113 §6.10).
            if self.tap.up.block.is_some()
                || self.tap.up.grpc.get(&stream).is_some_and(|b| !b.is_empty())
            {
                return;
            }
            let mut payload = vec![0];
            payload.extend_from_slice(&(message.len() as u32).to_be_bytes());
            payload.extend_from_slice(message);
            if !self.flow.take(stream, payload.len()) {
                return;
            }
            let (_, message) = self.queue.pop_front().expect("front was Some");
            out.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
            out.extend_from_slice(&[DATA, 0]);
            out.extend_from_slice(&stream.to_be_bytes());
            out.extend(payload);
            events.push(TapEvent::Injected {
                stream,
                path: self.path.clone().unwrap_or_default(),
                payload: message,
            });
        }
    }
}

/// Relay a tunnel whose client side already sent `first`, tapping both
/// directions and, with a [`Splice`], adding its messages to the client's
/// stream. Returns when both sides have closed.
pub async fn relay<C, S, F>(
    client: C,
    server: S,
    first: &[u8],
    sink: F,
    splice: Option<Splice>,
) -> std::io::Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(Vec<TapEvent>),
{
    let conn = Mutex::new(Conn::new(splice.as_ref().map(|s| s.path.clone())));
    let wake = Notify::new();
    let (mut cr, mut cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);
    let lock = || conn.lock().expect("h2c relay mutex poisoned");

    let (out, events) = lock().client_bytes(first);
    sw.write_all(&out).await?;
    let claims = |events: &[TapEvent], splice: &Option<Splice>| {
        let Some(s) = splice else { return };
        let opened = events.iter().any(|e| match e {
            TapEvent::Headers {
                dir: Direction::Up,
                headers,
                ..
            } => headers.iter().any(|(n, v)| n == ":path" && *v == s.path),
            _ => false,
        });
        if opened {
            s.claim();
        }
    };
    claims(&events, &splice);
    if !events.is_empty() {
        sink(events);
    }

    let up = async {
        let mut splice = splice;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let (out, events) = tokio::select! {
                r = cr.read(&mut buf) => match r {
                    Ok(0) | Err(_) => break,
                    Ok(n) => lock().client_bytes(&buf[..n]),
                },
                m = async {
                    match splice.as_mut() {
                        Some(s) => s.rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => match m {
                    Ok(message) if splice.as_ref().is_some_and(Splice::owns) => lock().splice(message),
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(target: LOG, "splice channel lagged; {n} message(s) lost");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        splice = None;
                        continue;
                    }
                },
                _ = wake.notified() => {
                    let (mut out, mut events) = (Vec::new(), Vec::new());
                    lock().flush(&mut out, &mut events);
                    (out, events)
                }
            };
            if !out.is_empty() && sw.write_all(&out).await.is_err() {
                break;
            }
            claims(&events, &splice);
            if !events.is_empty() {
                sink(events);
            }
        }
        let rest = lock().up.rest();
        let _ = sw.write_all(&rest).await;
        let _ = sw.shutdown().await;
        if let Some(s) = &splice {
            s.release();
        }
    };
    let down = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = match sr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let (out, events, pending) = {
                let mut c = lock();
                let (out, events) = c.server_bytes(&buf[..n]);
                (out, events, !c.queue.is_empty())
            };
            if !out.is_empty() && cw.write_all(&out).await.is_err() {
                break;
            }
            if pending {
                // A WINDOW_UPDATE may have made room for a queued splice.
                wake.notify_one();
            }
            if !events.is_empty() {
                sink(events);
            }
        }
        let rest = lock().down.rest();
        let _ = cw.write_all(&rest).await;
        let _ = cw.shutdown().await;
    };
    // Each side half-closes its peer at EOF, like `copy_bidirectional`; the
    // tunnel ends once both directions have.
    tokio::join!(up, down);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ftype: u8, flags: u8, stream: u32, payload: &[u8]) -> Vec<u8> {
        let mut f = (payload.len() as u32).to_be_bytes()[1..].to_vec();
        f.push(ftype);
        f.push(flags);
        f.extend_from_slice(&stream.to_be_bytes());
        f.extend_from_slice(payload);
        f
    }

    fn grpc(msg: &[u8]) -> Vec<u8> {
        let mut m = vec![0];
        m.extend_from_slice(&(msg.len() as u32).to_be_bytes());
        m.extend_from_slice(msg);
        m
    }

    fn messages(events: &[TapEvent]) -> Vec<(Direction, Option<String>, Vec<u8>)> {
        events
            .iter()
            .filter_map(|e| match e {
                TapEvent::Message {
                    dir, path, payload, ..
                } => Some((*dir, path.clone(), payload.clone())),
                _ => None,
            })
            .collect()
    }

    fn request_headers(enc: &mut loona_hpack::Encoder<'static>, path: &str) -> Vec<u8> {
        enc.encode(vec![
            (&b":method"[..], &b"POST"[..]),
            (&b":path"[..], path.as_bytes()),
            (&b"content-type"[..], &b"application/grpc"[..]),
        ])
    }

    #[test]
    fn detects_the_preface_from_its_first_four_bytes() {
        assert!(looks_like_h2c(b"PRI "));
        assert!(looks_like_h2c(PREFACE));
        assert!(looks_like_h2c(&[PREFACE, b"\x00\x00\x00\x04"].concat()));
        assert!(!looks_like_h2c(b"PRI"));
        assert!(!looks_like_h2c(b"GET / HTTP/1.1"));
        assert!(!looks_like_h2c(b"\x16\x03\x01\x02\x00"));
    }

    #[test]
    fn yields_grpc_messages_with_their_request_path() {
        let mut enc = loona_hpack::Encoder::new();
        let mut tap = H2Tap::new();
        let mut up = PREFACE.to_vec();
        up.extend(frame(0x4, 0, 0, &[])); // SETTINGS
        up.extend(frame(
            HEADERS,
            FLAG_END_HEADERS,
            1,
            &request_headers(&mut enc, "/IHub/Connect"),
        ));
        up.extend(frame(DATA, 0, 1, &grpc(b"\x92\x01\x02")));
        let ev = tap.feed(Direction::Up, &up);
        assert!(matches!(&ev[0], TapEvent::Headers { stream: 1, .. }));
        assert_eq!(
            messages(&ev),
            vec![(
                Direction::Up,
                Some("/IHub/Connect".into()),
                b"\x92\x01\x02".to_vec()
            )]
        );

        let down = frame(DATA, 0, 1, &grpc(b"\x92\x03\x04"));
        assert_eq!(
            messages(&tap.feed(Direction::Down, &down)),
            vec![(
                Direction::Down,
                Some("/IHub/Connect".into()),
                b"\x92\x03\x04".to_vec()
            )]
        );
    }

    #[test]
    fn reassembles_messages_split_across_reads_and_frames() {
        let mut tap = H2Tap::new();
        assert!(tap.feed(Direction::Up, PREFACE).is_empty());
        let mut msgs = grpc(b"first-message");
        msgs.extend(grpc(b"second"));
        // One gRPC stream split into two DATA frames, delivered one byte at a time.
        let (a, b) = msgs.split_at(7);
        let mut wire = frame(DATA, 0, 3, a);
        wire.extend(frame(DATA, FLAG_END_STREAM, 3, b));
        let mut got = Vec::new();
        for byte in wire {
            got.extend(messages(&tap.feed(Direction::Down, &[byte])));
        }
        let payloads: Vec<_> = got.into_iter().map(|(_, _, p)| p).collect();
        assert_eq!(
            payloads,
            vec![b"first-message".to_vec(), b"second".to_vec()]
        );
    }

    #[test]
    fn strips_padding_and_priority_and_joins_continuations() {
        let mut enc = loona_hpack::Encoder::new();
        let block = request_headers(&mut enc, "/A/B");
        let (h1, h2) = block.split_at(3);
        let mut headers = vec![0u8; 5]; // priority: dependency + weight
        headers.extend_from_slice(h1);
        let mut tap = H2Tap::new();
        let mut up = PREFACE.to_vec();
        up.extend(frame(HEADERS, FLAG_PRIORITY, 5, &headers));
        up.extend(frame(CONTINUATION, FLAG_END_HEADERS, 5, h2));
        let mut padded = vec![2u8];
        padded.extend(grpc(b"x"));
        padded.extend([0, 0]);
        up.extend(frame(DATA, FLAG_PADDED, 5, &padded));
        assert_eq!(
            messages(&tap.feed(Direction::Up, &up)),
            vec![(Direction::Up, Some("/A/B".into()), b"x".to_vec())]
        );
    }

    #[test]
    fn stops_decoding_a_direction_that_is_not_h2() {
        let mut tap = H2Tap::new();
        let ev = tap.feed(Direction::Up, b"GET / HTTP/1.1\r\n\r\n.........");
        assert!(matches!(
            ev.as_slice(),
            [TapEvent::Desync {
                dir: Direction::Up,
                ..
            }]
        ));
        assert!(tap.feed(Direction::Up, PREFACE).is_empty());
    }

    #[test]
    fn a_broken_header_block_still_lets_data_through() {
        let mut tap = H2Tap::new();
        let mut up = PREFACE.to_vec();
        // Indexed header field 127 doesn't exist in an empty table.
        up.extend(frame(HEADERS, FLAG_END_HEADERS, 1, &[0xFF, 0x00]));
        up.extend(frame(DATA, 0, 1, &grpc(b"still")));
        let ev = tap.feed(Direction::Up, &up);
        assert!(ev.iter().any(|e| matches!(e, TapEvent::Desync { .. })));
        assert_eq!(
            messages(&ev),
            vec![(Direction::Up, None, b"still".to_vec())]
        );
    }

    #[test]
    fn finished_and_reset_streams_are_forgotten() {
        let mut enc = loona_hpack::Encoder::new();
        let mut tap = H2Tap::new();
        let mut up = PREFACE.to_vec();
        up.extend(frame(
            HEADERS,
            FLAG_END_HEADERS,
            1,
            &request_headers(&mut enc, "/A/B"),
        ));
        up.extend(frame(
            HEADERS,
            FLAG_END_HEADERS,
            3,
            &request_headers(&mut enc, "/A/C"),
        ));
        up.extend(frame(DATA, 0, 3, &grpc(b"partial")[..4]));
        tap.feed(Direction::Up, &up);
        assert_eq!(tap.paths.len(), 2);
        tap.feed(Direction::Down, &frame(DATA, FLAG_END_STREAM, 1, &[]));
        tap.feed(Direction::Down, &frame(RST_STREAM, 0, 3, &[0, 0, 0, 8]));
        assert!(tap.paths.is_empty());
        assert!(tap.up.grpc.is_empty());
    }

    #[test]
    fn an_endless_header_block_is_cut_off() {
        let mut tap = H2Tap::new();
        let mut up = PREFACE.to_vec();
        up.extend(frame(HEADERS, 0, 1, &[0x82]));
        let chunk = vec![0u8; 64 * 1024];
        for _ in 0..(MAX_HEADER_BLOCK / chunk.len() + 1) {
            up.extend(frame(CONTINUATION, 0, 1, &chunk));
        }
        let ev = tap.feed(Direction::Up, &up);
        assert!(matches!(ev.last(), Some(TapEvent::Desync { .. })));
        assert!(tap.up.dead && tap.up.block.is_none());
    }

    const HUB: &str = "/IHub/Connect";

    /// A client that opened `HUB` on stream 1 and has sent one message on it.
    fn hub_conn() -> (Conn, loona_hpack::Encoder<'static>) {
        let mut enc = loona_hpack::Encoder::new();
        let mut conn = Conn::new(Some(HUB.into()));
        let mut up = PREFACE.to_vec();
        up.extend(frame(SETTINGS, 0, 0, &[]));
        up.extend(frame(
            HEADERS,
            FLAG_END_HEADERS,
            1,
            &request_headers(&mut enc, HUB),
        ));
        up.extend(frame(DATA, 0, 1, &grpc(b"join")));
        let (out, _) = conn.client_bytes(&up);
        assert_eq!(out, up, "client bytes pass unchanged");
        let settings = frame(SETTINGS, 0, 0, &[]);
        assert_eq!(conn.server_bytes(&settings).0, settings);
        (conn, enc)
    }

    fn injected(events: &[TapEvent]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter_map(|e| match e {
                TapEvent::Injected { payload, .. } => Some(payload.clone()),
                _ => None,
            })
            .collect()
    }

    fn window_update(stream: u32, inc: u32) -> Vec<u8> {
        frame(WINDOW_UPDATE, 0, stream, &inc.to_be_bytes())
    }

    #[test]
    fn a_splice_goes_out_as_a_data_frame_on_the_stream() {
        let (mut conn, _) = hub_conn();
        let (out, events) = conn.splice(b"call".to_vec());
        assert_eq!(out, frame(DATA, 0, 1, &grpc(b"call")));
        assert_eq!(injected(&events), vec![b"call".to_vec()]);
        // The tap still reads the client's stream as before.
        let (_, ev) = conn.client_bytes(&frame(DATA, 0, 1, &grpc(b"next")));
        assert_eq!(messages(&ev).len(), 1);
    }

    #[test]
    fn a_splice_waits_out_a_client_message_split_over_frames() {
        let (mut conn, _) = hub_conn();
        let msg = grpc(b"client-message");
        let (head, tail) = msg.split_at(6);
        let (out, _) = conn.client_bytes(&frame(DATA, 0, 1, head));
        assert_eq!(out, frame(DATA, 0, 1, head));
        let (out, events) = conn.splice(b"call".to_vec());
        assert!(
            out.is_empty() && events.is_empty(),
            "never inside the client's message"
        );
        let (out, events) = conn.client_bytes(&frame(DATA, 0, 1, tail));
        let mut want = frame(DATA, 0, 1, tail);
        want.extend(frame(DATA, 0, 1, &grpc(b"call")));
        assert_eq!(out, want, "right after it");
        assert_eq!(injected(&events), vec![b"call".to_vec()]);
    }

    #[test]
    fn a_splice_waits_out_a_header_block_left_open() {
        let (mut conn, mut enc) = hub_conn();
        let block = request_headers(&mut enc, "/IHub/Other");
        let (h1, h2) = block.split_at(2);
        conn.client_bytes(&frame(HEADERS, 0, 3, h1));
        let (out, _) = conn.splice(b"call".to_vec());
        assert!(out.is_empty(), "only CONTINUATION may follow");
        let (out, _) = conn.client_bytes(&frame(CONTINUATION, FLAG_END_HEADERS, 3, h2));
        let mut want = frame(CONTINUATION, FLAG_END_HEADERS, 3, h2);
        want.extend(frame(DATA, 0, 1, &grpc(b"call")));
        assert_eq!(out, want);
    }

    #[test]
    fn a_partial_frame_is_held_until_it_is_whole() {
        let (mut conn, _) = hub_conn();
        let f = frame(DATA, 0, 1, &grpc(b"abc"));
        let (out, _) = conn.client_bytes(&f[..5]);
        assert!(out.is_empty());
        let (out, events) = conn.splice(b"call".to_vec());
        assert_eq!(
            out,
            frame(DATA, 0, 1, &grpc(b"call")),
            "between whole frames"
        );
        assert_eq!(injected(&events).len(), 1);
        let (out, _) = conn.client_bytes(&f[5..]);
        assert_eq!(out, f);
    }

    #[test]
    fn spliced_bytes_are_taken_out_of_the_servers_window_updates() {
        let (mut conn, _) = hub_conn();
        let len = grpc(b"call").len() as u32;
        conn.splice(b"call".to_vec());

        // Connection level: a smaller update is swallowed, the rest is cut.
        let (out, _) = conn.server_bytes(&window_update(0, len - 1));
        assert!(out.is_empty());
        let (out, _) = conn.server_bytes(&window_update(0, 100));
        assert_eq!(out, window_update(0, 99));
        let (out, _) = conn.server_bytes(&window_update(0, 100));
        assert_eq!(out, window_update(0, 100), "nothing owed any more");

        // Stream level, the same; other streams pass untouched.
        let (out, _) = conn.server_bytes(&window_update(1, len + 5));
        assert_eq!(out, window_update(1, 5));
        let (out, _) = conn.server_bytes(&window_update(7, 3));
        assert_eq!(out, window_update(7, 3));
    }

    #[test]
    fn a_splice_waits_for_window() {
        let (mut conn, _) = hub_conn();
        // The server shrinks stream windows below one spliced frame.
        let mut settings = SETTINGS_INITIAL_WINDOW_SIZE.to_be_bytes().to_vec();
        settings.extend_from_slice(&(grpc(b"join").len() as u32 + 4).to_be_bytes());
        conn.server_bytes(&frame(SETTINGS, 0, 0, &settings));
        let (out, _) = conn.splice(b"call".to_vec());
        assert!(out.is_empty(), "4 bytes of window left");
        let (out, _) = conn.server_bytes(&window_update(1, 100));
        assert_eq!(
            out,
            window_update(1, 100),
            "granted before the splice spent it"
        );
        let (mut out, mut events) = (Vec::new(), Vec::new());
        conn.flush(&mut out, &mut events);
        assert_eq!(out, frame(DATA, 0, 1, &grpc(b"call")));
    }

    #[test]
    fn splices_without_an_open_stream_or_in_time_are_dropped() {
        let mut conn = Conn::new(Some(HUB.into()));
        conn.client_bytes(PREFACE);
        let (out, _) = conn.splice(b"call".to_vec());
        assert!(out.is_empty() && conn.queue.is_empty(), "no hub stream yet");

        let (mut conn, _) = hub_conn();
        let msg = grpc(b"client-message");
        conn.client_bytes(&frame(DATA, 0, 1, &msg[..6]));
        conn.splice(b"late".to_vec());
        conn.queue[0].0 -= SPLICE_STALE + Duration::from_millis(1);
        let (out, events) = conn.client_bytes(&frame(DATA, 0, 1, &msg[6..]));
        assert_eq!(out, frame(DATA, 0, 1, &msg[6..]));
        assert!(injected(&events).is_empty() && conn.queue.is_empty());

        // The client ended the stream: nothing more goes on it.
        conn.client_bytes(&frame(DATA, FLAG_END_STREAM, 1, &[]));
        let (out, _) = conn.splice(b"call".to_vec());
        assert!(out.is_empty());
    }

    #[test]
    fn a_server_that_does_not_open_with_settings_is_relayed_raw() {
        let mut enc = loona_hpack::Encoder::new();
        let mut conn = Conn::new(Some(HUB.into()));
        let mut up = PREFACE.to_vec();
        up.extend(frame(SETTINGS, 0, 0, &[]));
        up.extend(frame(
            HEADERS,
            FLAG_END_HEADERS,
            1,
            &request_headers(&mut enc, HUB),
        ));
        conn.client_bytes(&up);
        let (out, _) = conn.server_bytes(b"HTTP/1.1 400 Bad Request\r\n\r\n");
        assert_eq!(
            out, b"HTTP/1.1 400 Bad Request\r\n\r\n",
            "not held as one giant frame"
        );
        assert!(conn.splice(b"call".to_vec()).0.is_empty());
    }

    #[test]
    fn a_frame_cut_off_by_eof_still_reaches_the_peer() {
        let mut f = Framer::new(false);
        let settings = frame(SETTINGS, 0, 0, &[0, 4, 0, 0, 0, 9]);
        assert!(f.push(&settings[..10]).is_empty());
        assert_eq!(f.rest(), settings[..10]);
    }

    #[test]
    fn a_client_that_is_not_h2_is_relayed_raw_and_never_spliced() {
        let mut conn = Conn::new(Some(HUB.into()));
        let (out, _) = conn.client_bytes(b"GET / HTTP/1.1\r\n\r\n");
        assert_eq!(out, b"GET / HTTP/1.1\r\n\r\n");
        let (out, _) = conn.client_bytes(b"more");
        assert_eq!(out, b"more");
        assert!(conn.splice(b"call".to_vec()).0.is_empty());
    }

    #[tokio::test]
    async fn only_the_relay_holding_the_hub_writes_spliced_messages() {
        let (tx, _) = broadcast::channel(4);
        let owner = Arc::new(AtomicU64::new(0));
        let splice = |id| Splice {
            path: HUB.into(),
            rx: tx.subscribe(),
            owner: owner.clone(),
            id,
        };
        let mut enc = loona_hpack::Encoder::new();
        let mut first = PREFACE.to_vec();
        first.extend(frame(SETTINGS, 0, 0, &[]));
        first.extend(frame(
            HEADERS,
            FLAG_END_HEADERS,
            1,
            &request_headers(&mut enc, HUB),
        ));

        // Relay 2 opened the hub; relay 3 is some other h2c connection.
        let (client_app, client_proxy) = tokio::io::duplex(4096);
        let (server_proxy, mut server_app) = tokio::io::duplex(4096);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = seen.clone();
            move |ev: Vec<TapEvent>| seen.lock().unwrap().extend(ev)
        };
        let hub = tokio::spawn({
            let (first, s) = (first.clone(), splice(2));
            async move { relay(client_proxy, server_proxy, &first, sink, Some(s)).await }
        });
        let (other_app, other_proxy) = tokio::io::duplex(4096);
        let (other_server_proxy, mut other_server_app) = tokio::io::duplex(4096);
        let other = tokio::spawn({
            let s = splice(3);
            async move { relay(other_proxy, other_server_proxy, PREFACE, |_| {}, Some(s)).await }
        });

        let mut got = vec![0u8; first.len()];
        server_app.read_exact(&mut got).await.unwrap();
        let mut preface = vec![0u8; PREFACE.len()];
        other_server_app.read_exact(&mut preface).await.unwrap();
        while owner.load(Ordering::Acquire) != 2 {
            tokio::task::yield_now().await;
        }
        tx.send(b"call".to_vec()).unwrap();
        let want = frame(DATA, 0, 1, &grpc(b"call"));
        let mut got = vec![0u8; want.len()];
        server_app.read_exact(&mut got).await.unwrap();
        assert_eq!(got, want);

        drop((client_app, other_app));
        drop(other_server_app);
        other.await.unwrap().unwrap();
        let mut rest = Vec::new();
        server_app.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty(), "written once, by the hub's relay alone");
        drop(server_app);
        hub.await.unwrap().unwrap();
        assert_eq!(owner.load(Ordering::Acquire), 0, "released when it closed");
        assert_eq!(injected(&seen.lock().unwrap()), vec![b"call".to_vec()]);
    }

    #[tokio::test]
    async fn relay_forwards_bytes_untouched_and_taps_them() {
        let (client_app, client_proxy) = tokio::io::duplex(1024);
        let (server_proxy, mut server_app) = tokio::io::duplex(1024);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = {
            let seen = seen.clone();
            move |ev: Vec<TapEvent>| seen.lock().unwrap().extend(ev)
        };
        let mut first = PREFACE.to_vec();
        first.extend(frame(DATA, 0, 1, &grpc(b"hello")));
        let relay =
            tokio::spawn(
                async move { relay(client_proxy, server_proxy, &first, sink, None).await },
            );

        let mut got = vec![0u8; PREFACE.len() + 9 + 10];
        server_app.read_exact(&mut got).await.unwrap();
        assert!(got.starts_with(PREFACE));
        let reply = frame(DATA, 0, 1, &grpc(b"world"));
        server_app.write_all(&reply).await.unwrap();
        let mut client_app = client_app;
        let mut back = vec![0u8; reply.len()];
        client_app.read_exact(&mut back).await.unwrap();
        assert_eq!(back, reply);

        drop(server_app);
        drop(client_app);
        relay.await.unwrap().unwrap();
        let payloads: Vec<_> = messages(&seen.lock().unwrap())
            .into_iter()
            .map(|(d, _, p)| (d, p))
            .collect();
        assert_eq!(
            payloads,
            vec![
                (Direction::Up, b"hello".to_vec()),
                (Direction::Down, b"world".to_vec())
            ]
        );
    }
}
