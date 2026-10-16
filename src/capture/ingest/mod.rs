//! Ingest capture backend.
//!
//! Akagi listens on a loopback WebSocket, and something else pushes the
//! game's WebSocket frames into it — today, the userscript in
//! [`userscript`], running in the user's own browser. Frames are routed into
//! the platform [`crate::bridge::Bridge`] exactly as the Chromium backend
//! routes the frames CDP hands it, so nothing past this module can tell the
//! two apart.
//!
//! No CA, no system proxy, no browser launched by Akagi: the user plays in
//! whichever browser they like, with their own profile. What it costs is
//! autoplay — there is no page handle to drive, as on the MITM path.
//!
//! The server also serves the userscript itself at
//! [`userscript::USERSCRIPT_PATH`], rendered with this endpoint's address,
//! so installing it is one click, and at `/` an [`install_page`] that
//! links it and explains what to do without a userscript manager. Before
//! the backend runs — while the mode is picked but not yet saved —
//! [`preview`] serves both on the same address.
//!
//! Who may connect is decided in [`origin`]; what they send is
//! [`protocol`].

pub mod install_page;
pub mod origin;
pub mod preview;
pub mod protocol;
pub mod userscript;

use super::flow::{slugify, FlowBridges};
use super::frame::record_ws_frame;
use super::{CaptureBackend, CaptureCtx, CaptureDescriptor, CaptureKind, ShutdownToken};
use crate::bridge::{BridgeHooks, Direction};
use crate::config::{IngestConfig, Platform};
use crate::event_bus::{MjaiBus, NotifyBus};
use crate::inspector::InspectorWriter;
use crate::schema::{FrameDirection, FrameRaw, Notification};
use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{RawQuery, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use origin::{check_origin, platform_for_origin, OriginCheck};
use protocol::{decode_payload, Envelope, FrameDir, FrameKind, PROTOCOL_VERSION};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{debug, info, warn};

const LOG: &str = "akagi::capture::ingest";

/// WebSocket close code for "policy violation": the source was understood
/// and turned away (wrong protocol version, wrong platform).
const CLOSE_POLICY: u16 = 1008;

/// Flow identity: the endpoint connection, then the page's own socket
/// number, which is only unique within that connection.
type FlowKey = (u64, u32);

/// Parse a configured listen address, refusing anything but loopback.
///
/// Whatever reaches the endpoint is fed straight into the protocol bridges,
/// and the address is baked into the userscript, so it is never something
/// to expose to a network.
pub fn parse_loopback_addr(addr: &str) -> Result<SocketAddr> {
    let parsed: SocketAddr = addr
        .trim()
        .parse()
        .with_context(|| format!("invalid ingest address {addr:?} (expected ip:port)"))?;
    if !parsed.ip().is_loopback() {
        anyhow::bail!("ingest address {parsed} is not a loopback address; use 127.0.0.1 or [::1]");
    }
    Ok(parsed)
}

pub struct IngestBackend {
    cfg: IngestConfig,
}

impl IngestBackend {
    pub fn new(cfg: IngestConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait]
impl CaptureBackend for IngestBackend {
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()> {
        let addr = parse_loopback_addr(&self.cfg.addr)?;
        // The install preview typically holds this very address until now.
        preview::stop_any().await;
        let listener = TcpListener::bind(addr).await.with_context(|| {
            format!("binding the ingest endpoint on {addr} (is another Akagi running?)")
        })?;
        info!(target: LOG, "ingest backend listening on {addr}");

        // Graceful shutdown stops accepting, but an upgraded WebSocket has
        // left the server's hands: each connection watches this instead, so
        // a stopped backend stops feeding the bridges.
        let (stop_tx, stop_rx) = watch::channel(false);
        let shared = Arc::new(Shared::new(&ctx, addr, stop_rx));
        let app = Router::new()
            .route(userscript::INGEST_PATH, get(ingest_ws))
            .route(userscript::USERSCRIPT_PATH, get(serve_userscript))
            .route(userscript::INSTALL_PATH, get(serve_install_page))
            .route("/ping", get(|| async { "pong" }))
            .with_state(shared);

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.wait().await;
                let _ = stop_tx.send(true);
            })
            .await
            .context("ingest endpoint stopped with an error")
    }

    fn descriptor(&self) -> CaptureDescriptor {
        CaptureDescriptor {
            kind: CaptureKind::Ingest,
            label: self.cfg.addr.clone(),
        }
    }
}

struct Shared {
    platform: Platform,
    mjai_bus: MjaiBus,
    notify_bus: NotifyBus,
    inspector: InspectorWriter,
    bridges: FlowBridges<FlowKey>,
    userscript: String,
    next_conn: AtomicU64,
    stop: watch::Receiver<bool>,
}

impl Shared {
    fn new(ctx: &CaptureCtx, addr: SocketAddr, stop: watch::Receiver<bool>) -> Self {
        // The same hooks the Chromium backend hands its bridges, so a bridge
        // behaves identically whichever of the two feeds it. Autoplay itself
        // cannot act here (no page handle), but the bridges still keep the
        // shared state it reads current.
        let mut hooks = ctx
            .autoplay
            .as_ref()
            .map(|a| BridgeHooks {
                time_budget: Some(a.time_budget.clone()),
                input_watch: Some(a.input_watch.clone()),
                tenhou_state: Some(a.tenhou_state.clone()),
                riichi_inject: None,
                notify: None,
            })
            .unwrap_or_default();
        hooks.notify = Some(ctx.notify_bus.clone());
        Self {
            platform: ctx.platform,
            mjai_bus: ctx.mjai_bus.clone(),
            notify_bus: ctx.notify_bus.clone(),
            inspector: ctx.session.inspector(),
            bridges: FlowBridges::new(ctx.session.clone(), ctx.platform, hooks),
            userscript: userscript::render(addr),
            next_conn: AtomicU64::new(1),
            stop,
        }
    }
}

async fn serve_userscript(State(shared): State<Arc<Shared>>) -> Response {
    userscript_response(&shared.userscript)
}

/// The userscript as served — by the backend and by [`preview`] alike.
fn userscript_response(script: &str) -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        script.to_string(),
    )
        .into_response()
}

/// The install page, in the language the UI asked for with `?lang=`.
/// Served by the backend and by [`preview`] alike.
async fn serve_install_page(RawQuery(query): RawQuery) -> Response {
    let lang = query.as_deref().and_then(|q| {
        form_urlencoded::parse(q.as_bytes())
            .find(|(k, _)| k == "lang")
            .map(|(_, v)| v.into_owned())
    });
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        install_page::render(lang.as_deref()),
    )
        .into_response()
}

async fn ingest_ws(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let check = match headers.get(header::ORIGIN) {
        None => check_origin(None),
        Some(v) => match v.to_str() {
            Ok(o) => check_origin(Some(o)),
            Err(_) => OriginCheck::Foreign,
        },
    };
    if check == OriginCheck::Foreign {
        warn!(
            target: LOG,
            "refusing ingest connection from origin {:?}",
            headers.get(header::ORIGIN)
        );
        return (StatusCode::FORBIDDEN, "this page may not feed Akagi").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(shared, socket, check))
}

async fn handle_socket(shared: Arc<Shared>, mut socket: WebSocket, check: OriginCheck) {
    let mut source = Source {
        conn: shared.next_conn.fetch_add(1, Ordering::Relaxed),
        check,
        greeted: false,
        warned_ungreeted: false,
        flows: HashSet::new(),
    };
    debug!(target: LOG, "ingest connection {} opened", source.conn);
    let mut stop = shared.stop.clone();
    loop {
        if *stop.borrow() {
            let _ = socket.send(Message::Close(None)).await;
            break;
        }
        let msg = tokio::select! {
            changed = stop.changed() => {
                // A dropped sender means the backend is gone without having
                // said so; `changed()` would then return at once forever.
                if changed.is_err() {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
                continue;
            }
            m = socket.recv() => m,
        };
        match msg {
            Some(Ok(Message::Text(text))) => {
                if let Some(reason) = source.handle(&shared, text.as_str()) {
                    let _ = socket
                        .send(Message::Close(Some(CloseFrame {
                            code: CLOSE_POLICY,
                            reason: reason.into(),
                        })))
                        .await;
                    break;
                }
            }
            Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
            // Binary, ping, pong: nothing in the protocol uses them, and
            // axum answers pings itself.
            Some(Ok(_)) => {}
        }
    }
    source.release_all(&shared);
    debug!(target: LOG, "ingest connection {} closed", source.conn);
}

/// One connected source and the page sockets it has announced.
struct Source {
    conn: u64,
    check: OriginCheck,
    greeted: bool,
    warned_ungreeted: bool,
    flows: HashSet<u32>,
}

impl Source {
    /// Apply one message. `Some(reason)` means the source is turned away and
    /// the connection should close with that reason.
    fn handle(&mut self, shared: &Shared, text: &str) -> Option<&'static str> {
        let envelope = match serde_json::from_str::<Envelope>(text) {
            Ok(e) => e,
            Err(e) => {
                debug!(target: LOG, "ingest {}: ignoring unreadable message: {e}", self.conn);
                return None;
            }
        };
        match envelope {
            Envelope::Hello {
                v,
                agent,
                origin,
                url,
            } => self.greet(shared, v, &agent, &origin, &url),
            _ if !self.greeted => {
                if !self.warned_ungreeted {
                    self.warned_ungreeted = true;
                    warn!(
                        target: LOG,
                        "ingest {}: ignoring messages sent before hello", self.conn
                    );
                }
                None
            }
            Envelope::Open { flow, url } => {
                let key = (self.conn, flow);
                drop(
                    shared
                        .bridges
                        .acquire(key, &slugify(&url), &format!("ingest {url}")),
                );
                self.flows.insert(flow);
                None
            }
            Envelope::Frame {
                flow,
                dir,
                kind,
                data,
            } => {
                self.frame(shared, flow, dir, kind, data);
                None
            }
            Envelope::Close { flow } => {
                self.release(shared, flow);
                None
            }
        }
    }

    fn greet(
        &mut self,
        shared: &Shared,
        v: u32,
        agent: &str,
        claimed_origin: &str,
        url: &str,
    ) -> Option<&'static str> {
        if v != PROTOCOL_VERSION {
            warn!(
                target: LOG,
                "ingest {}: {agent} speaks protocol v{v}, this build speaks v{PROTOCOL_VERSION}",
                self.conn
            );
            let _ = shared.notify_bus.send(
                Notification::warn("Reinstall the Akagi userscript")
                    .body(format!(
                        "The installed userscript speaks protocol v{v}; this Akagi speaks \
                         v{PROTOCOL_VERSION}. Reinstall it from Settings → Capture."
                    ))
                    .id("ingest-version"),
            );
            return Some("protocol version mismatch");
        }

        // A browser page is judged by the Origin its browser sent; only a
        // local program, which sent none, is taken at its word.
        let page_platform = match self.check {
            OriginCheck::Game(p) => Some(p),
            OriginCheck::Local => platform_for_origin(claimed_origin),
            OriginCheck::Foreign => None,
        };
        if let Some(p) = page_platform.filter(|p| *p != shared.platform) {
            warn!(
                target: LOG,
                "ingest {}: page {url:?} is {p:?} but the platform is {:?}; ignoring it",
                self.conn,
                shared.platform
            );
            let _ = shared.notify_bus.send(
                Notification::warn("Userscript ignored: wrong platform")
                    .body(format!(
                        "{url} is a {p:?} page, but Akagi is set to {:?}. Switch the \
                         platform in Settings, or close that tab.",
                        shared.platform
                    ))
                    .id("ingest-platform"),
            );
            return Some("platform mismatch");
        }

        self.greeted = true;
        info!(
            target: LOG,
            "ingest {}: source connected ({agent}, {url})", self.conn
        );
        let _ = shared.notify_bus.send(
            Notification::success("Userscript connected")
                .body(url.to_string())
                .id("ingest-source"),
        );
        None
    }

    fn frame(&mut self, shared: &Shared, flow: u32, dir: FrameDir, kind: FrameKind, data: String) {
        let Some(payload) = decode_payload(kind, &data) else {
            warn!(
                target: LOG,
                "ingest {}: base64 decode failed for a frame on flow {flow}", self.conn
            );
            return;
        };
        let key = (self.conn, flow);
        let bridge = shared.bridges.acquire(key, "ws", "ws frame");
        self.flows.insert(flow);
        let direction = Direction::from(dir);
        let result = {
            let mut b = bridge.lock().expect("bridge mutex poisoned");
            b.parse(direction, &payload)
        };
        let raw = match kind {
            FrameKind::Text => FrameRaw::Text(data),
            FrameKind::Binary => FrameRaw::Binary(data),
        };
        let frame_dir = match dir {
            FrameDir::Up => FrameDirection::Up,
            FrameDir::Down => FrameDirection::Down,
        };
        record_ws_frame(
            &shared.inspector,
            frame_dir,
            format!("ingest:{}:{flow}", self.conn),
            raw,
            payload.len(),
            &result,
        );
        for e in result.events {
            let _ = shared.mjai_bus.send(e);
        }
    }

    fn release(&mut self, shared: &Shared, flow: u32) {
        // Only flows this connection created: releasing an unknown one would
        // first create a bridge (and its log file) just to drop it.
        if self.flows.remove(&flow) {
            let key = (self.conn, flow);
            let bridge = shared.bridges.acquire(key, "ws", "ws frame");
            shared.bridges.release(&key, bridge);
        }
    }

    fn release_all(&mut self, shared: &Shared) {
        for flow in std::mem::take(&mut self.flows) {
            let key = (self.conn, flow);
            let bridge = shared.bridges.acquire(key, "ws", "ws frame");
            shared.bridges.release(&key, bridge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_addresses_are_accepted() {
        assert_eq!(
            parse_loopback_addr("127.0.0.1:23411").unwrap(),
            "127.0.0.1:23411".parse().unwrap()
        );
        assert!(parse_loopback_addr(" [::1]:23411 ").is_ok());
    }

    /// Whatever reaches the endpoint goes into the bridges, so it must not
    /// be reachable from a network.
    #[test]
    fn non_loopback_addresses_are_refused() {
        for addr in ["0.0.0.0:23411", "192.168.1.10:23411", "[::]:23411"] {
            let err = parse_loopback_addr(addr).unwrap_err().to_string();
            assert!(err.contains("not a loopback"), "{addr}: {err}");
        }
    }

    #[test]
    fn malformed_addresses_are_refused() {
        assert!(parse_loopback_addr("localhost:23411").is_err());
        assert!(parse_loopback_addr("127.0.0.1").is_err());
    }
}
