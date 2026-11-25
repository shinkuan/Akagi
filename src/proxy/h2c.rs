//! Cleartext HTTP/2 (h2c, prior knowledge) tunnels: a passive frame tap and
//! the relay that feeds it.
//!
//! After a `CONNECT`, hudsucker only understands HTTP/1 and TLS; any other
//! first bytes are tunneled blind. HoloMahjong's battle server speaks gRPC
//! over h2c, so for that platform the proxy relays tunnels itself
//! ([`relay`]) and runs both directions through [`H2Tap`], which turns the
//! byte stream back into complete gRPC messages. Bytes are always forwarded
//! before they are parsed, and a tap that loses track only stops decoding —
//! the tunnel keeps relaying.

use crate::bridge::Direction;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The client connection preface (RFC 9113 §3.4).
pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

const FRAME_HEADER_LEN: usize = 9;
const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const RST_STREAM: u8 = 0x3;
const PUSH_PROMISE: u8 = 0x5;
const CONTINUATION: u8 = 0x9;
const FLAG_END_STREAM: u8 = 0x1;
const FLAG_END_HEADERS: u8 = 0x4;
const FLAG_PADDED: u8 = 0x8;
const FLAG_PRIORITY: u8 = 0x20;
/// gRPC length-prefixed message header: compressed flag + u32 length.
const GRPC_PREFIX_LEN: usize = 5;
/// A stream buffering more than this without completing a message is not gRPC.
const MAX_MESSAGE_LEN: usize = 16 * 1024 * 1024;
/// Ceiling on a header block spread over CONTINUATION frames.
const MAX_HEADER_BLOCK: usize = 1024 * 1024;

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

/// Copy `from` → `to`, forwarding each chunk before handing it to the tap.
/// Ends on EOF or error, shutting down the write side so the peer sees it.
pub async fn pump<R, W, F>(mut from: R, mut to: W, dir: Direction, tap: Arc<Mutex<H2Tap>>, sink: F)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    F: Fn(Vec<TapEvent>),
{
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match from.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if to.write_all(&buf[..n]).await.is_err() {
            break;
        }
        let events = tap
            .lock()
            .expect("h2c tap mutex poisoned")
            .feed(dir, &buf[..n]);
        if !events.is_empty() {
            sink(events);
        }
    }
    let _ = to.shutdown().await;
}

/// Relay a tunnel whose client side already sent `first`, tapping both
/// directions. Returns when either side closes.
pub async fn relay<C, S, F>(client: C, mut server: S, first: &[u8], sink: F) -> std::io::Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(Vec<TapEvent>) + Clone,
{
    let tap = Arc::new(Mutex::new(H2Tap::new()));
    server.write_all(first).await?;
    let events = tap
        .lock()
        .expect("h2c tap mutex poisoned")
        .feed(Direction::Up, first);
    if !events.is_empty() {
        sink(events);
    }
    let (cr, cw) = tokio::io::split(client);
    let (sr, sw) = tokio::io::split(server);
    let up = pump(cr, sw, Direction::Up, tap.clone(), sink.clone());
    let down = pump(sr, cw, Direction::Down, tap, sink);
    // Each pump half-closes its peer at EOF, like `copy_bidirectional`; the
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
            tokio::spawn(async move { relay(client_proxy, server_proxy, &first, sink).await });

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
