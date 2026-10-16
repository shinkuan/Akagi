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
use crate::config::HttpCaptureConfig;
use crate::event_bus::{MjaiBus, NotifyBus};
use crate::inspector::annotate::{self, RequestView};
use crate::inspector::InspectorWriter;
use crate::schema::{
    CaptureSource, FrameDirection, FrameRaw, HttpBody, HttpExchange, HttpHeader, HttpPhase,
    InspectorEntry,
};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chromiumoxide::page::Page;
use chromiumoxide::{
    cdp::browser_protocol::network::{
        EnableParams as NetworkEnableParams, EventRequestWillBeSent, EventResponseReceived,
        EventWebSocketClosed, EventWebSocketCreated, EventWebSocketFrameReceived,
        EventWebSocketFrameSent, Headers, ResourceType,
    },
    Browser,
};
use chrono::Local;
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

/// Decide whether the autoplay page handle — currently owned by tab
/// `owner` (its `TargetId`, if any) — must be cleared when the page-poll
/// loop reaps the `removed` tabs this tick.
///
/// The handle tracks the browser **tab**, not any single WebSocket, so it
/// is cleared only when its owning tab disappears from the snapshot. This
/// is the crux of the "autoplay silently stops mid-game" fix: Majsoul
/// opens and closes many short-lived Route-probe / lobby-reconnect sockets
/// to `*.maj-soul.com` while a game runs on a separate `game-gateway`
/// socket, and those socket closures must **not** drop the handle. Pure so
/// the decision is unit-testable without a live `Page`.
pub fn page_handle_cleared_by_removal(owner: Option<&str>, removed: &[String]) -> bool {
    matches!(owner, Some(o) if removed.iter().any(|r| r == o))
}

/// Hosts whose WebSocket creation hands the page handle to autoplay.
/// `maj-soul.com` covers en/cn/jp portals; `mahjongsoul.com` is the
/// Yostar mirror. `tenhou.net` and `mjv.jp` are Tenhou's portal and its
/// game gateway respectively — Tenhou autoplay speaks on the page's own
/// socket, so it needs the same page handle.
const AUTOPLAY_HOST_HINTS: &[&str] = &["maj-soul.com", "mahjongsoul.com", "tenhou.net", "mjv.jp"];

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
    http_cfg: HttpCaptureConfig,
    notify: NotifyBus,
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
            for id in &removes {
                if let Some(h) = subscribed.remove(id) {
                    h.abort();
                    debug!("CDP: dropped subscription for closed target {id}");
                }
            }

            // The autoplay page handle tracks the browser *tab*, not any
            // single WebSocket, so it is cleared here — when its owning tab
            // is actually gone — rather than on `webSocketClosed`. Majsoul
            // opens and closes many short-lived Route-probe / lobby-reconnect
            // sockets to *.maj-soul.com while a game runs on a separate
            // game-gateway socket; clearing the handle on those closes was
            // silently stopping autoplay mid-game.
            if let (Some(ctx), false) = (&autoplay, removes.is_empty()) {
                // Hold the write lock across the check + clear so a
                // concurrent rebind from another tab's task can't slip
                // between reading the owner and nulling the handle.
                let mut guard = ctx.page.write().await;
                let owner = guard.as_ref().map(|p| p.target_id().inner().clone());
                if page_handle_cleared_by_removal(owner.as_deref(), &removes) {
                    *guard = None;
                    drop(guard);
                    *ctx.canvas_rect.write().await = None;
                    info!("autoplay: page handle cleared — owning Majsoul tab closed");
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
                    http_cfg.clone(),
                    notify.clone(),
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

/// URL of the script that carries the Tenhou client.
///
/// Versioned (`/4/1141.js`, reached through a redirect from `latest.js`), so
/// the pattern matches by shape rather than by version.
const TENHOU_CLIENT_URL_PATTERN: &str = "*tenhou.net/4/*.js";

/// Ask the browser to hand us the Tenhou client script before the page runs
/// it, so it can be rewritten to expose its handler registry.
///
/// Paused at the *response* stage: we want the bytes, not just the request.
async fn enable_script_rewrite(page: &Page) -> Result<()> {
    use chromiumoxide::cdp::browser_protocol::fetch::{EnableParams, RequestPattern, RequestStage};
    let pattern = RequestPattern {
        url_pattern: Some(TENHOU_CLIENT_URL_PATTERN.to_string()),
        resource_type: None,
        request_stage: Some(RequestStage::Response),
    };
    let params = EnableParams {
        patterns: Some(vec![pattern]),
        handle_auth_requests: None,
    };
    page.execute(params).await.context("Fetch.enable")?;
    Ok(())
}

/// Reload the page if its client script slipped past the interceptor.
///
/// Attaching is racy by construction: the browser opens on the game URL and
/// we subscribe afterwards, so the client script is usually fetched — or
/// served from disk cache — before `Fetch.enable` takes effect. The
/// interceptor is then armed for a request that has already happened.
///
/// Reloading past the cache puts the script back through it. Done once, at
/// attach, so it lands on the lobby rather than mid-game; a page that already
/// carries the door is left alone, which is what keeps this from looping.
async fn reload_if_uninstrumented(page: &Page) -> Result<()> {
    use chromiumoxide::cdp::browser_protocol::page::ReloadParams;

    let expr = format!(
        "(()=>{{try{{return !!window.{} || !/tenhou\\.net/.test(location.host);}}\
          catch(e){{return true}}}})()",
        crate::autoplay::tenhou::inject::EXPORT_GLOBAL
    );
    let done = page
        .evaluate(expr)
        .await
        .ok()
        .and_then(|r| r.value().and_then(|v| v.as_bool()))
        .unwrap_or(true);
    if done {
        return Ok(());
    }
    info!("CDP: re-loading the Tenhou client so it passes through the interceptor");
    let params = ReloadParams::builder().ignore_cache(true).build();
    page.execute(params).await.context("Page.reload")?;
    Ok(())
}

/// Rewrite one paused response and let it through.
///
/// Every outcome continues the request — instrumentation that fails must cost
/// the discard path, never the page. A derivation failure is surfaced to the
/// user because it means the client changed shape and the pattern that finds
/// its handler registry needs revisiting; that is a report worth having.
async fn rewrite_paused_script(
    page: &Page,
    notify: &NotifyBus,
    request_id: chromiumoxide::cdp::browser_protocol::fetch::RequestId,
    url: &str,
) {
    use crate::autoplay::tenhou::inject;
    use chromiumoxide::cdp::browser_protocol::fetch::{
        ContinueRequestParams, FulfillRequestParams, GetResponseBodyParams,
    };

    let passthrough = |id: chromiumoxide::cdp::browser_protocol::fetch::RequestId| {
        ContinueRequestParams::builder().request_id(id).build().ok()
    };

    let body = match page
        .execute(GetResponseBodyParams::new(request_id.clone()))
        .await
    {
        Ok(r) => {
            let inner = r.result;
            if inner.base64_encoded {
                base64::engine::general_purpose::STANDARD
                    .decode(inner.body.as_bytes())
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
            } else {
                Some(inner.body)
            }
        }
        Err(e) => {
            // Redirects and other bodiless responses land here. The pattern
            // is deliberately loose enough to catch the client whatever
            // version it is served under, so it also catches its neighbours;
            // that is expected, not a fault.
            debug!("CDP: no body for {url}: {e:#}");
            None
        }
    };

    let rewritten = match body.as_deref().map(inject::rewrite_client) {
        Some(Ok(js)) => Some(js),
        // Not the client at all — the URL pattern matches its neighbours
        // too (`inflate_min.js`, the `latest.js` redirect). Nothing to say.
        Some(Err(inject::InjectError::NoDiscardHandler)) => {
            debug!("CDP: {url} is not the Tenhou client; passing through");
            None
        }
        // It *is* the client — it registers a discard handler — but the
        // registry could not be recovered. That means the client changed
        // shape, which is the one failure here that cannot be diagnosed from
        // logs alone, so ask for a report.
        Some(Err(e)) => {
            warn!("CDP: cannot instrument the Tenhou client ({url}): {e}");
            let _ = notify.send(
                crate::schema::Notification::warn("Tenhou autoplay unavailable")
                    .body(format!(
                        "This build of the Tenhou client could not be instrumented ({e}).                          Discards will not be played. Please report this so the client                          pattern can be updated."
                    ))
                    .sticky(),
            );
            None
        }
        None => None,
    };

    let sent = match rewritten {
        Some(js) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(js.as_bytes());
            match FulfillRequestParams::builder()
                .request_id(request_id.clone())
                .response_code(200)
                // A fulfilled response carries none of the original headers.
                // Today the browser sniffs the missing type and executes
                // anyway, but one `X-Content-Type-Options: nosniff` on
                // tenhou.net's side would turn that into a blocked script —
                // so say what it is.
                .response_header(
                    chromiumoxide::cdp::browser_protocol::fetch::HeaderEntry::new(
                        "Content-Type",
                        "text/javascript",
                    ),
                )
                .body(encoded)
                .build()
            {
                Ok(p) => {
                    if let Err(e) = page.execute(p).await {
                        warn!("CDP: could not serve the instrumented client: {e:#}");
                        false
                    } else {
                        info!("CDP: Tenhou client instrumented ({url})");
                        true
                    }
                }
                Err(e) => {
                    warn!("CDP: could not build the instrumented response: {e}");
                    false
                }
            }
        }
        None => false,
    };

    if !sent {
        if let Some(p) = passthrough(request_id) {
            let _ = page.execute(p).await;
        }
    }
}

/// Enable Network on the page, subscribe to the four WS events, and
/// spawn a routing task. Returns the task handle so the poll loop can
/// abort it when the tab closes.
#[allow(clippy::too_many_arguments)]
async fn attach_page(
    page: Page,
    target_id: String,
    bridges: Arc<FlowBridges<FlowKey>>,
    mjai_bus: MjaiBus,
    inspector: InspectorWriter,
    autoplay: Option<Arc<AutoplayContext>>,
    http_cfg: HttpCaptureConfig,
    notify: NotifyBus,
) -> Result<JoinHandle<()>> {
    page.execute(NetworkEnableParams::default())
        .await
        .context("Network.enable")?;
    // The pause listener has to exist before `Fetch.enable` arms the
    // interceptor: a request paused with no listener yet is a request nobody
    // ever continues, and the page hangs on it. The stream buffers
    // (unbounded) until the routing task below starts polling, so
    // subscribing early costs nothing.
    let mut on_paused = page
        .event_listener::<chromiumoxide::cdp::browser_protocol::fetch::EventRequestPaused>()
        .await
        .context("subscribe requestPaused")?;
    // Instrument the Tenhou client on its way in. Scoped to that one script
    // so nothing else on the page is paused; a failure to enable is logged
    // and the session continues without a discard path.
    if let Err(e) = enable_script_rewrite(&page).await {
        warn!("CDP: client instrumentation unavailable on target {target_id}: {e:#}");
    } else if let Err(e) = reload_if_uninstrumented(&page).await {
        warn!("CDP: could not re-load the client for instrumentation: {e:#}");
    }
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
    // Every HTTP request the page makes, so we can pick the game's
    // analytics beacons out of it. `Network.enable` above already turns
    // this event on; the filtering is ours, in the select arm.
    let mut on_request = page
        .event_listener::<EventRequestWillBeSent>()
        .await
        .context("subscribe requestWillBeSent")?;
    let mut on_response = page
        .event_listener::<EventResponseReceived>()
        .await
        .context("subscribe responseReceived")?;

    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(ev) = on_paused.next() => {
                    let url = ev.request.url.clone();
                    rewrite_paused_script(&page, &notify, ev.request_id.clone(), &url).await;
                }
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
                    // into it. The handle is bound to the *tab* and lives
                    // until the tab closes (see the poll loop); a new WS on
                    // the same tab just refreshes it. Multi-tab user:
                    // most-recent wins, per the plan.
                    if let Some(ctx) = &autoplay {
                        if is_autoplay_target_url(&ev.url) {
                            let mut guard = ctx.page.write().await;
                            let prev_target =
                                guard.as_ref().map(|p| p.target_id().inner().clone());
                            let same_tab = prev_target.as_deref() == Some(target_id.as_str());
                            *guard = Some(page.clone());
                            drop(guard);
                            if same_tab {
                                // Majsoul re-opens sockets (Route probes,
                                // lobby reconnects) constantly on the same
                                // tab; refreshing the handle is a no-op and
                                // must not spam warnings.
                                debug!(
                                    "autoplay: page handle refreshed on new WS for target {target_id} ({})",
                                    ev.url
                                );
                            } else {
                                if let Some(prev) = &prev_target {
                                    warn!(
                                        "autoplay: page handle moving from tab {prev} to {target_id}"
                                    );
                                    // The cached canvas rect belonged to the
                                    // old tab; a different tab may have
                                    // different geometry, so drop it and let
                                    // the manager re-query against the new page.
                                    *ctx.canvas_rect.write().await = None;
                                }
                                info!(
                                    "autoplay: page handle bound to target {target_id} via WS {}",
                                    ev.url
                                );
                            }
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

                    // NB: we deliberately do NOT touch the autoplay page
                    // handle here. Majsoul closes short-lived Route-probe /
                    // lobby-reconnect sockets to *.maj-soul.com throughout a
                    // game while the real game-gateway socket stays open;
                    // dropping the handle on those closes silently stopped
                    // autoplay mid-game. The handle is tied to the tab and
                    // cleared by the poll loop when the tab itself closes.
                }
                Some(ev) = on_request.next() => {
                    // Fires for every subresource the page loads — the
                    // asymmetry with the MITM leg, where a whole session
                    // is a couple of dozen requests. Filter first, and
                    // keep the recognizers' work off the hot path.
                    if is_static_asset(ev.r#type.as_ref()) && !http_cfg.static_assets {
                        continue;
                    }
                    let headers = headers_of(&ev.request.headers);
                    let annotations = annotate::annotate_request(&RequestView::new(
                        &ev.request.method,
                        &ev.request.url,
                        &headers,
                    ));
                    if let Some(a) = annotations.first() {
                        info!(
                            target: "akagi::capture::http",
                            "recognized {} {}", a.kind, a.summary,
                        );
                    }
                    if !http_cfg.record_all && annotations.is_empty() {
                        continue;
                    }
                    inspector.record(InspectorEntry::Http {
                        ts_ms: Local::now().timestamp_millis(),
                        source: CaptureSource::Chromium,
                        exchange: HttpExchange {
                            // CDP hands out a real request id, so pairing
                            // here is exact — unlike the MITM leg.
                            exchange_id: Some(ev.request_id.inner().clone()),
                            phase: HttpPhase::Request,
                            method: ev.request.method.clone(),
                            url: ev.request.url.clone(),
                            host: host_of(&ev.request.url),
                            version: String::new(),
                            status: None,
                            headers,
                            body: None,
                            annotations,
                        },
                    });
                }
                Some(ev) = on_response.next() => {
                    if !http_cfg.record_all {
                        continue;
                    }
                    if is_static_asset(Some(&ev.r#type)) && !http_cfg.static_assets {
                        continue;
                    }
                    inspector.record(InspectorEntry::Http {
                        ts_ms: Local::now().timestamp_millis(),
                        source: CaptureSource::Chromium,
                        exchange: HttpExchange {
                            exchange_id: Some(ev.request_id.inner().clone()),
                            phase: HttpPhase::Response,
                            method: String::new(),
                            url: ev.response.url.clone(),
                            host: host_of(&ev.response.url),
                            version: String::new(),
                            status: Some(ev.response.status as u16),
                            headers: headers_of(&ev.response.headers),
                            // Reading a body here costs a separate
                            // `Network.getResponseBody` round-trip per
                            // request, which the MITM leg does not need.
                            // Say so rather than look like there was none.
                            body: Some(HttpBody {
                                text: None,
                                bytes: None,
                                skipped: Some(
                                    "not captured on the chromium backend".to_string(),
                                ),
                            }),
                            annotations: Vec::new(),
                        },
                    });
                }
                else => break,
            }
        }
    });
    Ok(handle)
}

/// Subresource types that say nothing about the client and would bury
/// everything else. A WebGL game pulls thousands; the MITM leg never sees
/// them at all because the game fetches them outside the proxied path.
fn is_static_asset(kind: Option<&ResourceType>) -> bool {
    matches!(
        kind,
        Some(
            ResourceType::Image
                | ResourceType::Font
                | ResourceType::Media
                | ResourceType::Stylesheet
        )
    )
}

/// CDP delivers headers as a JSON object, which has no wire order to
/// preserve — unlike the MITM leg, where order is a real fingerprint.
/// Sorted so two captures of the same request compare equal.
fn headers_of(headers: &Headers) -> Vec<HttpHeader> {
    let Some(map) = headers.inner().as_object() else {
        return Vec::new();
    };
    let mut out: Vec<HttpHeader> = map
        .iter()
        .map(|(name, value)| HttpHeader {
            name: name.clone(),
            value: match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            },
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Host of an absolute URL, or empty when it has none (`data:`, `blob:`).
fn host_of(url: &str) -> String {
    url.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_string))
        .unwrap_or_default()
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
    crate::capture::frame::record_ws_frame(
        inspector,
        direction,
        flow_id,
        raw,
        payload.len(),
        result,
    );
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

    /// Regression: a Majsoul Route-probe / lobby-reconnect socket closing
    /// must NOT clear the autoplay page handle — that was making autoplay
    /// silently stop mid-game. The handle is tied to the browser tab, so
    /// only the owning tab's removal from the page snapshot clears it.
    #[test]
    fn page_handle_cleared_only_when_owning_tab_closes() {
        let owner = Some("TAB_A");
        // A *different* tab closing (or a WS closing, which never reaches
        // this predicate at all) leaves our handle intact.
        assert!(!page_handle_cleared_by_removal(owner, &["TAB_B".into()]));
        // Nothing reaped this tick — keep the handle.
        assert!(!page_handle_cleared_by_removal(owner, &[]));
        // The owning tab itself disappearing is the only trigger.
        assert!(page_handle_cleared_by_removal(
            owner,
            &["TAB_B".into(), "TAB_A".into()]
        ));
        // No handle bound → nothing to clear regardless of what closed.
        assert!(!page_handle_cleared_by_removal(None, &["TAB_A".into()]));
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
        assert_eq!(
            decode_frame_payload(2, &b64),
            FrameDecode::Bytes(raw.to_vec())
        );
    }

    #[test]
    fn binary_frame_bad_base64_signals_decode_error() {
        // Distinguished from `Skip` so the inline branch can WARN —
        // legit malformed CDP from Chrome shouldn't be confused with
        // an intentionally-ignored control frame.
        assert_eq!(
            decode_frame_payload(2, "not base64!@#"),
            FrameDecode::BadBase64
        );
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
