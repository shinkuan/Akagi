//! Per-page CDP subscription that routes WebSocket frames into the
//! platform bridge.
//!
//! Why per-page (not browser-level): chromiumoxide 0.9.1 does not deliver
//! page-scoped events to `Browser::event_listener` even with
//! `Target.setAutoAttach { flatten: true }`. The events arrive on the
//! browser connection but stay tagged with the originating page session;
//! `Browser::event_listener` only surfaces browser-level events. The
//! canonical pattern (see `chromiumoxide-0.9.1/examples/interception.rs`)
//! is to grab a `Page` and call `page.event_listener::<E>()` on it.
//!
//! Subscription lifecycle:
//! - Poll `browser.pages()` every ~1s.
//! - On a new `target_id`: enable Network domain on that page, subscribe
//!   to the four WS events, spawn a routing task.
//! - On a `target_id` disappearing from the snapshot (tab closed):
//!   `JoinHandle::abort` the routing task and drop our entry.
//!
//! Service-worker WebSockets are not subscribed in v1 — Majsoul uses
//! page-scoped WS today. If real-world testing shows otherwise, expand
//! the polling to include `browser.targets()` and filter on type.

use crate::autoplay::AutoplayContext;
use crate::bridge::Direction;
use crate::capture::flow::{slugify, FlowBridges};
use crate::event_bus::MjaiBus;
use crate::inspector::InspectorWriter;
use crate::schema::{FrameDirection, FrameRaw, InspectorEntry};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chrono::Local;
use chromiumoxide::page::Page;
use chromiumoxide::{
    cdp::browser_protocol::network::{
        EnableParams as NetworkEnableParams, EventWebSocketClosed, EventWebSocketCreated,
        EventWebSocketFrameReceived, EventWebSocketFrameSent,
    },
    Browser,
};
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

const PAGE_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Per-flow key for `FlowBridges`. `target` is the page that owns the
/// WebSocket; `request` is the CDP request id that the page assigned to
/// `new WebSocket(...)`. The pair is unique across the browser session
/// even when two tabs both open a connection to the same Majsoul host.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub target: String,
    pub request: String,
}

fn decode_payload(b64: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(b64.as_bytes())
        .ok()
}

/// Outcome of decoding a `Network.webSocketFrame*` payload into raw bytes
/// for the bridge.
///
/// CDP's `WebSocketFrame.payloadData` is shaped by the WS opcode (RFC 6455):
///
/// - opcode `1` (text): the field is a **plain UTF-8 string**. Tenhou
///   uses this — frames look like `{"tag":"INIT",…}` and the heartbeat
///   `<Z/>`. We pass the bytes straight through; the bridge re-parses them.
/// - opcode `2` (binary): the field is a **base64-encoded string**.
///   Majsoul uses this (length-prefixed protobuf).
/// - everything else (`0` continuation, `8` close, `9` ping, `10` pong):
///   carries no game data — drop.
///
/// Splitting this out so the dispatch is unit-testable: prior to this
/// fix the inline branches only handled opcode 2, which silently dropped
/// every Tenhou frame on the chromium backend.
#[derive(Debug, PartialEq, Eq)]
enum FrameDecode {
    Bytes(Vec<u8>),
    Skip,
    BadBase64,
}

fn decode_frame_payload(opcode: i64, payload_data: &str) -> FrameDecode {
    match opcode {
        1 => FrameDecode::Bytes(payload_data.as_bytes().to_vec()),
        2 => match decode_payload(payload_data) {
            Some(b) => FrameDecode::Bytes(b),
            None => FrameDecode::BadBase64,
        },
        _ => FrameDecode::Skip,
    }
}

/// Compute the symmetric difference between the previous and current
/// page snapshots. Returns `(adds, removes)` — target ids to subscribe
/// and target ids whose subscription tasks should be reaped. Pure so
/// the diff logic is unit-testable independent of the CDP loop.
pub fn diff_pages(prev: &HashSet<String>, current: &HashSet<String>) -> (Vec<String>, Vec<String>) {
    let adds: Vec<_> = current.difference(prev).cloned().collect();
    let removes: Vec<_> = prev.difference(current).cloned().collect();
    (adds, removes)
}

/// Hosts whose WebSocket creation hands the page handle to autoplay.
/// `maj-soul.com` covers en/cn/jp portals; `mahjongsoul.com` is the
/// Yostar mirror.
const AUTOPLAY_HOST_HINTS: &[&str] = &["maj-soul.com", "mahjongsoul.com"];

fn is_autoplay_target_url(ws_url: &str) -> bool {
    AUTOPLAY_HOST_HINTS.iter().any(|h| ws_url.contains(h))
}

/// Run the CDP loop until the browser disconnects or an unrecoverable
/// error occurs. Frames flow through `bridges` into `mjai_bus`, and each
/// frame is also recorded into `inspector` for the Logs → Inspector tab.
///
/// `autoplay` is `Some` only on the chromium backend when the autoplay
/// feature is wired (`AppState.autoplay_context`). On Majsoul WS open
/// we publish the page handle into it; autoplay reads it back to dispatch
/// `Input.dispatchMouseEvent`. Passing `None` makes the loop bridge-only.
pub async fn run(
    endpoint: &str,
    bridges: Arc<FlowBridges<FlowKey>>,
    mjai_bus: MjaiBus,
    inspector: InspectorWriter,
    autoplay: Option<Arc<AutoplayContext>>,
) -> Result<()> {
    info!("CDP connecting to {endpoint}");
    let (browser_owned, mut handler) = Browser::connect(endpoint)
        .await
        .with_context(|| format!("CDP connect to {endpoint}"))?;
    // `Browser` is not `Clone`; share via Arc for the page-poll task.
    let browser = Arc::new(browser_owned);

    // Pump the chromiumoxide handler — required so its internal
    // request/response oneshots resolve. The handler also surfaces
    // `WS Invalid message` warnings when Chrome sends events
    // chromiumoxide doesn't have a typed binding for; those are
    // non-fatal noise and the stream keeps running.
    let pump = tokio::spawn(async move {
        while let Some(ev) = handler.next().await {
            if let Err(e) = ev {
                debug!("chromiumoxide handler event error: {e:?}");
            }
        }
    });

    // Per-page subscription registry. Key: TargetId stringified.
    let mut subscribed: HashMap<String, JoinHandle<()>> = HashMap::new();

    let poll_loop = async {
        loop {
            let pages = match browser.pages().await {
                Ok(p) => p,
                Err(e) => {
                    debug!("browser.pages() error: {e:?}");
                    tokio::time::sleep(PAGE_POLL_INTERVAL).await;
                    continue;
                }
            };
            let current: HashSet<String> = pages
                .iter()
                .map(|p| p.target_id().inner().clone())
                .collect();
            let prev: HashSet<String> = subscribed.keys().cloned().collect();
            let (adds, removes) = diff_pages(&prev, &current);

            // Reap closed tabs first so we don't leak resources during
            // long sessions where users open + close many tabs.
            for id in removes {
                if let Some(h) = subscribed.remove(&id) {
                    h.abort();
                    debug!("CDP: dropped subscription for closed target {id}");
                }
            }

            // Subscribe new tabs.
            for page in pages {
                let id = page.target_id().inner().clone();
                if !adds.contains(&id) {
                    continue;
                }
                match attach_page(
                    page.clone(),
                    id.clone(),
                    bridges.clone(),
                    mjai_bus.clone(),
                    inspector.clone(),
                    autoplay.clone(),
                )
                .await
                {
                    Ok(handle) => {
                        info!("CDP: attached to page target {id}");
                        subscribed.insert(id, handle);
                    }
                    Err(e) => {
                        warn!("CDP: failed to attach to target {id}: {e:#}");
                    }
                }
            }

            tokio::time::sleep(PAGE_POLL_INTERVAL).await;
        }
        // unreachable, but type-check the future as `()` for select arm
        #[allow(unreachable_code)]
        ()
    };

    tokio::select! {
        _ = pump => info!("CDP handler pump exited"),
        _ = poll_loop => info!("CDP page poll exited"),
    }
    // Abort any still-live page subscriptions before tearing down.
    for (_id, h) in subscribed {
        h.abort();
    }
    drop(browser);
    Err(anyhow!("CDP loop terminated"))
}

/// Enable Network on the page, subscribe to the four WS events, and
/// spawn a routing task. Returns the task handle so the poll loop can
/// abort it when the tab closes.
async fn attach_page(
    page: Page,
    target_id: String,
    bridges: Arc<FlowBridges<FlowKey>>,
    mjai_bus: MjaiBus,
    inspector: InspectorWriter,
    autoplay: Option<Arc<AutoplayContext>>,
) -> Result<JoinHandle<()>> {
    page.execute(NetworkEnableParams::default())
        .await
        .context("Network.enable")?;
    let mut on_created = page
        .event_listener::<EventWebSocketCreated>()
        .await
        .context("subscribe webSocketCreated")?;
    let mut on_recv = page
        .event_listener::<EventWebSocketFrameReceived>()
        .await
        .context("subscribe webSocketFrameReceived")?;
    let mut on_sent = page
        .event_listener::<EventWebSocketFrameSent>()
        .await
        .context("subscribe webSocketFrameSent")?;
    let mut on_closed = page
        .event_listener::<EventWebSocketClosed>()
        .await
        .context("subscribe webSocketClosed")?;

    let handle = tokio::spawn(async move {
        // Track the most recent autoplay-target request id for this page,
        // so we know which WS close to react to when clearing the page
        // handle from the autoplay context.
        let mut autoplay_request_id: Option<String> = None;
        loop {
            tokio::select! {
                Some(ev) = on_created.next() => {
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    let label = format!("ws {}", ev.url);
                    let slug = slugify(&ev.url);
                    let _ = bridges.acquire(key, &slug, &label);
                    debug!("ws created: {} (target {target_id} request {})", ev.url, ev.request_id.inner());

                    // If this is the platform's WS (Majsoul), capture
                    // the owning page so autoplay can dispatch input
                    // into it. Multi-tab user: most-recent wins, per
                    // the plan.
                    if let Some(ctx) = &autoplay {
                        if is_autoplay_target_url(&ev.url) {
                            let mut guard = ctx.page.write().await;
                            if guard.is_some() {
                                warn!(
                                    "autoplay: replacing page handle on new WS for target {target_id}"
                                );
                            }
                            *guard = Some(page.clone());
                            autoplay_request_id = Some(ev.request_id.inner().clone());
                            info!(
                                "autoplay: page handle bound to target {target_id} via WS {}",
                                ev.url
                            );
                        }
                    }
                }
                Some(ev) = on_recv.next() => {
                    let opcode = ev.response.opcode as i64;
                    let payload = match decode_frame_payload(opcode, &ev.response.payload_data) {
                        FrameDecode::Bytes(b) => b,
                        FrameDecode::BadBase64 => {
                            warn!("base64 decode failed for inbound WS frame");
                            continue;
                        }
                        FrameDecode::Skip => continue,
                    };
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    let flow_id = format_flow_id(&key);
                    let bridge = bridges.acquire(key, "ws", "ws frame");
                    let result = {
                        let mut b = bridge.lock().expect("bridge mutex poisoned");
                        b.parse(Direction::Down, &payload)
                    };
                    record_frame(
                        &inspector,
                        FrameDirection::Down,
                        flow_id,
                        opcode,
                        &payload,
                        &ev.response.payload_data,
                        &result,
                    );
                    for e in result.events {
                        let _ = mjai_bus.send(e);
                    }
                }
                Some(ev) = on_sent.next() => {
                    let opcode = ev.response.opcode as i64;
                    let payload = match decode_frame_payload(opcode, &ev.response.payload_data) {
                        FrameDecode::Bytes(b) => b,
                        FrameDecode::BadBase64 => {
                            warn!("base64 decode failed for outbound WS frame");
                            continue;
                        }
                        FrameDecode::Skip => continue,
                    };
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    let flow_id = format_flow_id(&key);
                    let bridge = bridges.acquire(key, "ws", "ws frame");
                    let result = {
                        let mut b = bridge.lock().expect("bridge mutex poisoned");
                        b.parse(Direction::Up, &payload)
                    };
                    record_frame(
                        &inspector,
                        FrameDirection::Up,
                        flow_id,
                        opcode,
                        &payload,
                        &ev.response.payload_data,
                        &result,
                    );
                    for e in result.events {
                        let _ = mjai_bus.send(e);
                    }
                }
                Some(ev) = on_closed.next() => {
                    let key = FlowKey {
                        target: target_id.clone(),
                        request: ev.request_id.inner().clone(),
                    };
                    debug!("ws closed: target={target_id} request={}", ev.request_id.inner());
                    // Synthetic empty bridge ref so we can call release.
                    // FlowBridges::release reaps the entry when no other
                    // direction's task is holding a clone.
                    let bridge = bridges.acquire(key.clone(), "ws", "ws frame");
                    bridges.release(&key, bridge);

                    // If this is the autoplay-target WS, drop our hold
                    // on the page handle. The next reconnection (game
                    // restart, network blip) re-binds it from `on_created`.
                    if let (Some(ctx), Some(req)) = (&autoplay, &autoplay_request_id) {
                        if *req == *ev.request_id.inner() {
                            *ctx.page.write().await = None;
                            autoplay_request_id = None;
                            debug!("autoplay: page handle cleared on WS close for target {target_id}");
                        }
                    }
                }
                else => break,
            }
        }
    });
    Ok(handle)
}

/// Build a stable flow id for the inspector. Uses just the request id
/// (truncated, since CDP request ids are opaque hashes ~10 chars) for
/// brevity — the timeline already implicitly groups by flow because
/// frames from one connection arrive interleaved.
fn format_flow_id(key: &FlowKey) -> String {
    let req = &key.request;
    let trim = if req.len() > 10 { &req[..10] } else { req };
    format!("ws:{trim}")
}

/// Record one inspector `WsFrame` entry for a parsed frame. For text
/// frames (`opcode == 1`) `payload_data` is the original UTF-8 string —
/// we use it verbatim so the JSONL stays human-readable. For binary
/// frames (`opcode == 2`) we re-emit the original base64 (`payload_data`)
/// rather than re-encoding `payload`, which is identical content but
/// avoids a copy.
fn record_frame(
    inspector: &InspectorWriter,
    direction: FrameDirection,
    flow_id: String,
    opcode: i64,
    payload: &[u8],
    payload_data: &str,
    result: &crate::bridge::ParseResult,
) {
    let raw = if opcode == 1 {
        FrameRaw::Text(payload_data.to_string())
    } else {
        FrameRaw::Binary(payload_data.to_string())
    };
    inspector.record(InspectorEntry::WsFrame {
        ts_ms: Local::now().timestamp_millis(),
        direction,
        flow_id,
        size: payload.len(),
        raw,
        parsed: result.parsed.clone(),
        emitted: result.events.len(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_payload_ok() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"hello");
        assert_eq!(decode_payload(&b64), Some(b"hello".to_vec()));
    }

    #[test]
    fn decode_payload_bad() {
        assert_eq!(decode_payload("not-base64-!@#"), None);
    }

    #[test]
    fn diff_adds_and_removes() {
        let prev: HashSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
        let current: HashSet<String> = ["b", "c", "d"].into_iter().map(String::from).collect();
        let (adds, removes) = diff_pages(&prev, &current);
        let mut adds = adds;
        let mut removes = removes;
        adds.sort();
        removes.sort();
        assert_eq!(adds, vec!["d"]);
        assert_eq!(removes, vec!["a"]);
    }

    #[test]
    fn diff_empty_when_unchanged() {
        let s: HashSet<String> = ["x", "y"].into_iter().map(String::from).collect();
        let (adds, removes) = diff_pages(&s, &s);
        assert!(adds.is_empty());
        assert!(removes.is_empty());
    }

    #[test]
    fn diff_initial_subscribe() {
        let prev: HashSet<String> = HashSet::new();
        let current: HashSet<String> = ["a", "b"].into_iter().map(String::from).collect();
        let (adds, removes) = diff_pages(&prev, &current);
        let mut adds = adds;
        adds.sort();
        assert_eq!(adds, vec!["a", "b"]);
        assert!(removes.is_empty());
    }

    /// Regression: prior code dropped every non-binary frame, which
    /// silently broke Tenhou capture (Tenhou uses opcode 1 / text).
    #[test]
    fn text_frame_passes_through_as_utf8_bytes() {
        let payload = r#"{"tag":"INIT","seed":"1,0,0,2,5,134"}"#;
        assert_eq!(
            decode_frame_payload(1, payload),
            FrameDecode::Bytes(payload.as_bytes().to_vec())
        );
    }

    #[test]
    fn text_heartbeat_passes_through() {
        // Tenhou's `<Z/>` heartbeat is a 4-byte text frame.
        assert_eq!(
            decode_frame_payload(1, "<Z/>"),
            FrameDecode::Bytes(b"<Z/>".to_vec())
        );
    }

    #[test]
    fn binary_frame_base64_decodes() {
        let raw = b"\x00\x01\x02hello";
        let b64 = base64::engine::general_purpose::STANDARD.encode(raw);
        assert_eq!(decode_frame_payload(2, &b64), FrameDecode::Bytes(raw.to_vec()));
    }

    #[test]
    fn binary_frame_bad_base64_signals_decode_error() {
        // Distinguished from `Skip` so the inline branch can WARN —
        // legit malformed CDP from Chrome shouldn't be confused with
        // an intentionally-ignored control frame.
        assert_eq!(decode_frame_payload(2, "not base64!@#"), FrameDecode::BadBase64);
    }

    #[test]
    fn control_and_continuation_frames_are_skipped() {
        for opcode in [0i64, 8, 9, 10] {
            assert_eq!(
                decode_frame_payload(opcode, "irrelevant"),
                FrameDecode::Skip,
                "opcode {opcode} should skip"
            );
        }
    }
}
