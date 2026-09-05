use crate::{
    bridge::{self, Bridge, Direction},
    capture::http::{self as httpcap, BodyPlan, ExchangePairing, HttpCapturePolicy},
    config::Platform,
    event_bus::{MjaiBus, NotifyBus},
    inspector::{
        annotate::{self, RequestView},
        InspectorWriter,
    },
    logger::{BinaryLogger, Session},
    proxy::{certstore::CertStore, rewrite::majsoul_cert},
    schema::{
        CaptureSource, FrameDirection, FrameRaw, HttpAnnotation, HttpExchange, HttpPhase,
        InspectorEntry, Notification,
    },
};
use base64::Engine as _;
use chrono::Local;
use http_body_util::{BodyExt as _, Full};
use hudsucker::{
    futures::{Sink, SinkExt, Stream, StreamExt},
    hyper::{self, Method, Request, Response, StatusCode, Uri},
    tokio_tungstenite::tungstenite::{self, Message},
    Body, HttpContext, HttpHandler, RequestOrResponse, WebSocketContext, WebSocketHandler,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex as StdMutex,
    },
};
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

const TAG_CLIENT_TO_SERVER: u8 = 0;
const TAG_SERVER_TO_CLIENT: u8 = 1;

/// Shared, per-WS-upgrade bridge. Both directions of the same WebSocket
/// connection (client→server and server→client) need the same `Bridge`
/// instance because Majsoul's request/response correlation lives in the
/// parser's `pending` map: the Request travels client→server and the
/// matching Response travels server→client.
type SharedBridge = Arc<StdMutex<Box<dyn Bridge>>>;

/// Per-flow inspector identity. The map is keyed on the same
/// `SocketAddr` the bridges map uses, so they line up.
type FlowInspectorIds = Arc<StdMutex<HashMap<SocketAddr, String>>>;

#[derive(Clone)]
pub struct ProxyHandler {
    session: Arc<Session>,
    binary: Arc<BinaryLogger>,
    platform: Platform,
    bridges: Arc<StdMutex<HashMap<SocketAddr, SharedBridge>>>,
    next_flow_id: Arc<AtomicU64>,
    /// Optional fan-out for parsed mjai events. `None` keeps the proxy
    /// usable in tests and in standalone "log only" mode.
    mjai_tx: Option<MjaiBus>,
    /// Optional fan-out for user-facing toasts. `None` in "log only" mode.
    notify_tx: Option<NotifyBus>,
    /// Latches on the first refused loopback CONNECT. A misconfigured
    /// redirector produces one per socket the game opens, and the user only
    /// needs telling once — the log keeps every occurrence.
    loopback_notified: Arc<AtomicBool>,
    /// Inspector writer. Cloned from `session.inspector()` at construction.
    /// Cheap to clone (Arc inside).
    inspector: InspectorWriter,
    /// Stable inspector flow id per client SocketAddr. Lets the inspector
    /// timeline group frames by flow even though the underlying bridge
    /// already lives at the SocketAddr key.
    inspector_flow_ids: FlowInspectorIds,
    /// Triggered by `stop_capture` to kick all in-flight WS flows. Without
    /// this, hudsucker's `with_graceful_shutdown` only blocks new
    /// connections; existing ones would drain naturally and the game
    /// client would never see a disconnect.
    force_close: Arc<Notify>,
    /// How much of the intercepted HTTP traffic to record.
    http_policy: HttpCapturePolicy,
    /// Request↔response pairing state. See `capture::http`.
    pairing: Arc<ExchangePairing>,
    /// Certificates observed on the upstream leg, shared with the TLS
    /// verifier that records them.
    certs: Arc<CertStore>,
    /// Whether to correct the client's certificate report. See
    /// `rewrite::majsoul_cert`.
    rewrite_cert_report: bool,
    /// Whether to drop the game's Aliyun SLS telemetry beacons instead of
    /// forwarding them. See `block_telemetry_beacon`.
    block_telemetry: bool,
    /// Frame injection channel for Riichi City autoplay: the client→server
    /// relay forwards what arrives here to the game server (gated on the
    /// bridge's `in_game` flag). `None` in "log only" mode / tests.
    /// See `autoplay::inject`.
    inject: Option<crate::autoplay::inject::SharedInjectBus>,
}

impl ProxyHandler {
    /// Every collaborator is injected rather than reached for, which is
    /// what lets the integration tests drive the proxy with buses and
    /// autoplay left out. Hence the argument count.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session: Arc<Session>,
        platform: Platform,
        mjai_tx: Option<MjaiBus>,
        notify_tx: Option<NotifyBus>,
        force_close: Arc<Notify>,
        http_policy: HttpCapturePolicy,
        certs: Arc<CertStore>,
        rewrite_cert_report: bool,
        block_telemetry: bool,
        inject: Option<crate::autoplay::inject::SharedInjectBus>,
    ) -> anyhow::Result<Self> {
        let binary = session.binary_logger("proxy")?;
        let inspector = session.inspector();
        Ok(Self {
            session,
            binary,
            platform,
            bridges: Arc::new(StdMutex::new(HashMap::new())),
            next_flow_id: Arc::new(AtomicU64::new(1)),
            mjai_tx,
            notify_tx,
            loopback_notified: Arc::new(AtomicBool::new(false)),
            inspector,
            inspector_flow_ids: Arc::new(StdMutex::new(HashMap::new())),
            force_close,
            http_policy,
            pairing: Arc::new(ExchangePairing::default()),
            certs,
            rewrite_cert_report,
            block_telemetry,
            inject,
        })
    }

    /// Tell the user, once, that their redirector is proxying loopback. The
    /// log line alone is not enough — nobody opens the log while staring at a
    /// game that never finishes loading.
    ///
    /// Returns `true` if this was the first refusal (caller logs at `WARN`;
    /// later ones are `DEBUG` so a chatty client can't flood the file).
    fn notify_loopback_refused(&self, authority: &Uri) -> bool {
        if self.loopback_notified.swap(true, Ordering::Relaxed) {
            return false;
        }
        if let Some(tx) = &self.notify_tx {
            // Sticky: this blocks play until the user fixes the redirector.
            let _ = tx.send(
                Notification::warn("Proxy redirector is sending loopback traffic to Akagi")
                    .body(format!(
                        "Refused CONNECT to {authority}. Games talk to themselves over \
                         loopback, and proxying that hangs them on the loading screen. \
                         Exclude localhost / 127.0.0.1 / ::1 from the rule that redirects \
                         the game — in Proxifier, enable the \"Localhost\" rule (action: \
                         Direct) and move it above the game rule.",
                    ))
                    .sticky()
                    .id("proxy-loopback-connect"),
            );
        }
        true
    }

    /// Stable inspector flow id for `client`. Computed once on first
    /// frame from the per-platform subdir + the same `next_flow_id`
    /// counter the bridge map uses, so two flows from the same socket
    /// share the same id.
    fn inspector_flow_id(&self, client: SocketAddr) -> String {
        let mut map = self
            .inspector_flow_ids
            .lock()
            .expect("inspector_flow_ids mutex poisoned");
        map.entry(client)
            .or_insert_with(|| {
                let n = self.next_flow_id.load(Ordering::Relaxed).saturating_sub(1);
                format!("{}:{:06}", self.platform.subdir(), n)
            })
            .clone()
    }

    /// Record one half of an HTTP exchange.
    ///
    /// `record_all` off keeps only exchanges a recognizer understood, so
    /// the default session file holds the interesting traffic without
    /// accumulating every credential the client sends. See
    /// [`crate::config::HttpCaptureConfig`].
    fn record_http(&self, exchange: HttpExchange) {
        if !self.http_policy.record_all && exchange.annotations.is_empty() {
            return;
        }
        self.inspector.record(InspectorEntry::Http {
            ts_ms: Local::now().timestamp_millis(),
            source: CaptureSource::Mitm,
            exchange,
        });
    }

    /// Drop a telemetry request instead of forwarding it, and record the
    /// drop on the timeline. Two shapes match: Aliyun SLS web-tracking
    /// beacons (the annotator's path-based recognizer) and Riichi City's
    /// plain-HTTP `/game_manager/TslogServlet` tracking endpoint.
    ///
    /// Returns the synthetic response to answer the client with when `req`
    /// is such a request, or `None` for ordinary traffic that must be
    /// forwarded unchanged. The SLS recognizer is the same one the timeline
    /// annotator uses, so anything it would label `sls_beacon` is exactly
    /// what gets blocked. TLS-riding beacons that never reach this layer
    /// are refused earlier, at CONNECT time — see the telemetry CONNECT
    /// refusal in `handle_request`.
    ///
    /// The genuine endpoint answers `200` with an empty body, so that is
    /// what we return — indistinguishable from an ad blocker or the real
    /// server, and the client fires these fire-and-forget so it never reads
    /// the reply anyway. The drop is still recorded (with the beacon's own
    /// annotation plus an `akagi_blocked` marker): an annotated gap is not a
    /// gap, exactly as with the raw-tunnel bypass in `should_intercept`.
    fn block_telemetry_beacon(&self, req: &Request<Body>) -> Option<Response<Body>> {
        // Riichi City's plain-HTTP telemetry endpoint rides ordinary
        // forwarded requests (no tunnel), so it is blocked here alongside
        // the SLS beacons.
        if is_riichi_city_telemetry_path(req.uri().path())
            || annotate::sls::parse_uri(req.uri()).is_some()
        {
            // fall through to the shared block below
        } else {
            return None;
        }

        let method = req.method().to_string();
        let url = req.uri().to_string();
        let host = req
            .uri()
            .host()
            .map(str::to_string)
            .or_else(|| {
                req.headers()
                    .get(hyper::header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .map(|h| h.split(':').next().unwrap_or(h).to_string())
            })
            .unwrap_or_default();
        let headers = httpcap::headers_of(req.headers());

        // Keep the beacon's own annotation so the timeline still shows what
        // it was (logstore, log_category, params), then mark that Akagi
        // dropped it rather than forwarded it.
        let mut annotations =
            annotate::annotate_request(&RequestView::new(&method, &url, &headers));
        annotations.push(HttpAnnotation {
            kind: "akagi_blocked".to_string(),
            summary: "telemetry blocked — not forwarded".to_string(),
            data: serde_json::json!({
                "reason": "Aliyun SLS web-tracking beacon dropped before it left the machine; \
                           many ad blockers block this host too, so a missing beacon is not a signal",
            }),
        });

        info!(
            target: "akagi::proxy::http",
            "blocked telemetry beacon on {host}: {url}",
        );

        self.inspector.record(InspectorEntry::Http {
            ts_ms: Local::now().timestamp_millis(),
            source: CaptureSource::Mitm,
            exchange: HttpExchange {
                exchange_id: None,
                phase: HttpPhase::Request,
                method,
                url,
                host,
                version: format!("{:?}", req.version()),
                status: None,
                headers,
                body: None,
                annotations,
            },
        });

        Some(
            Response::builder()
                .status(StatusCode::OK)
                .body(Body::empty())
                .expect("static telemetry-block response must build"),
        )
    }

    /// Substitute the genuine upstream certificates into a Mahjong Soul
    /// certificate report, in place.
    ///
    /// A no-op unless this request is that beacon and we have observed
    /// the certificates it refers to. The rewritten URI is what gets
    /// recorded on the timeline too — the inspector shows what left the
    /// machine, not what the client wrote, because the former is what the
    /// operator sees.
    fn rewrite_certificate_report(&self, req: &mut Request<Body>) {
        // Both guards live in the rewriter: it declines anything that is
        // not a certificate report, and anything it cannot improve.
        let Some(beacon) = annotate::sls::parse_uri(req.uri()) else {
            return;
        };
        // The raw query is what everything except `content` is copied
        // from verbatim, so the rewriter needs it, not just the decoded
        // view — see its module docs.
        let query = req.uri().query().unwrap_or_default().to_string();
        let Some(out) = majsoul_cert::rewrite(&query, &beacon, &self.certs) else {
            return;
        };

        let mut parts = req.uri().clone().into_parts();
        let path = parts
            .path_and_query
            .as_ref()
            .map(|pq| pq.path().to_string())
            .unwrap_or_else(|| "/".to_string());
        match format!("{path}?{}", out.query).parse() {
            Ok(pq) => parts.path_and_query = Some(pq),
            Err(e) => {
                // Leave the request untouched rather than forward a
                // mangled one.
                warn!(
                    target: "akagi::proxy::rewrite",
                    "certificate report rewrite produced an invalid URI, forwarding as-is: {e}",
                );
                return;
            }
        }
        match Uri::from_parts(parts) {
            Ok(uri) => {
                *req.uri_mut() = uri;
                if out.uncorrected > 0 {
                    warn!(
                        target: "akagi::proxy::rewrite",
                        "certificate report: corrected {} entr{}, but {} still describe{} Akagi's \
                         CA because no upstream certificate was observed for those gateways",
                        out.corrected,
                        if out.corrected == 1 { "y" } else { "ies" },
                        out.uncorrected,
                        if out.uncorrected == 1 { "s" } else { "" },
                    );
                } else {
                    info!(
                        target: "akagi::proxy::rewrite",
                        "certificate report: corrected all {} entries", out.corrected,
                    );
                }
            }
            Err(e) => warn!(
                target: "akagi::proxy::rewrite",
                "certificate report rewrite could not rebuild the URI, forwarding as-is: {e}",
            ),
        }
    }

    /// Buffer a body when the plan says to, and rebuild the message so
    /// forwarding is unaffected.
    ///
    /// Returns the replacement body alongside what to store. Only ever
    /// called when [`httpcap::plan_body`] already approved it from the
    /// headers, so the amount held in memory is bounded by the policy.
    async fn take_body(
        plan: BodyPlan,
        body: Body,
        headers: &hyper::HeaderMap,
    ) -> (Body, Option<crate::schema::HttpBody>) {
        if !matches!(plan, BodyPlan::Capture { .. }) {
            return (body, plan.into_skipped());
        }
        match body.collect().await {
            Ok(collected) => {
                // `Bytes` is refcounted, so handing the same buffer to
                // both the record and the rebuilt body costs no copy.
                let bytes = collected.to_bytes();
                let record = httpcap::body_record(&bytes, headers);
                (Body::from(Full::new(bytes)), Some(record))
            }
            // A body that fails to read is gone either way; say so rather
            // than forward an empty one silently.
            Err(e) => (
                Body::empty(),
                Some(crate::schema::HttpBody {
                    text: None,
                    bytes: None,
                    skipped: Some(format!("body read failed: {e}")),
                }),
            ),
        }
    }

    fn acquire_bridge(&self, client: SocketAddr, uri: &Uri) -> SharedBridge {
        let mut map = self.bridges.lock().expect("bridges mutex poisoned");
        map.entry(client)
            .or_insert_with(|| {
                let flow_id = self.next_flow_id.fetch_add(1, Ordering::Relaxed);
                let path = uri_path_slug(uri);
                let file_name = format!("{flow_id:06}-{path}.log");
                let label = format!("{} {} {}", self.platform.subdir(), client, uri);
                let flow_log =
                    match self
                        .session
                        .flow_logger(self.platform.subdir(), &file_name, label)
                    {
                        Ok(log) => Some(log),
                        Err(e) => {
                            warn!("failed to open flow log for {client}: {e:#}");
                            None
                        }
                    };
                // No time-budget slot and no input watch: the MITM path
                // has no `Page` handle, so click-based autoplay can never
                // run here. The one slot the MITM path DOES wire is Riichi
                // City's frame-injection gate — its autoplay transmits
                // protocol frames rather than clicking a page.
                let hooks = bridge::BridgeHooks {
                    riichi_inject: if self.platform == Platform::RiichiCity {
                        self.inject.clone()
                    } else {
                        None
                    },
                    notify: self.notify_tx.clone(),
                    ..bridge::BridgeHooks::default()
                };
                Arc::new(StdMutex::new(bridge::for_platform(
                    self.platform,
                    flow_log,
                    Some(self.session.clone()),
                    hooks,
                )))
            })
            .clone()
    }

    /// Drop our reference; if no other direction still holds the bridge,
    /// remove it from the map so per-connection state doesn't leak.
    fn release_bridge(&self, client: SocketAddr, bridge: SharedBridge) {
        drop(bridge);
        let mut map = self.bridges.lock().expect("bridges mutex poisoned");
        if let Some(existing) = map.get(&client) {
            // Only the map's own Arc remains → connection fully closed.
            if Arc::strong_count(existing) == 1 {
                map.remove(&client);
            }
        }
    }
}

impl HttpHandler for ProxyHandler {
    async fn handle_request(&mut self, ctx: &HttpContext, req: Request<Body>) -> RequestOrResponse {
        if req.uri().path() == "/ping" {
            return Response::builder()
                .status(StatusCode::OK)
                .body(Body::from("pong"))
                .expect("Failed to build ping response")
                .into();
        }
        // Pull Host header for diagnostics. The CDN routes by SNI/Host,
        // not by URI path, so when URIs come in as IP literals the Host
        // header is the only place the real hostname could survive — log
        // it next to the URI so we can tell at a glance whether a stuck
        // request had a real hostname or just an IP.
        let host_hdr = req
            .headers()
            .get(hyper::header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-");
        info!(
            target: "akagi::proxy::forward",
            "{} uri_host={} host_hdr={} {} v={:?} client={}",
            req.method(),
            req.uri().host().unwrap_or("-"),
            host_hdr,
            req.uri(),
            req.version(),
            ctx.client_addr,
        );

        // A game client never legitimately CONNECTs to loopback — the proxy
        // itself lives there. Seeing one means the user's redirector
        // (Proxifier &c.) matched the game for *any* target host and swept up
        // the game's own internal `socketpair()` self-connect.
        //
        // Refusing here rather than tunneling it is load-bearing. hudsucker's
        // `process_connect` answers 200 and then blocks reading the client's
        // first 4 bytes *before* it opens the upstream socket; a self-connect's
        // client side never speaks first (it is waiting on its own `accept()`),
        // so the tunnel deadlocks and the game wedges on its loading screen.
        // Returning a `Response` short-circuits hudsucker's `proxy()` before
        // that blocking read is ever reached.
        //
        // Tunneling it correctly is not an option either: libcurl's
        // `socketpair()` emulation checks that the address it accepted matches
        // its connecting socket's local address, which a proxied hop breaks.
        if is_loopback_connect(req.method(), req.uri()) {
            if self.notify_loopback_refused(req.uri()) {
                warn!(
                    target: "akagi::proxy::forward",
                    "refusing CONNECT to loopback {} from {}: your proxy redirector is sending the \
                     game's own internal loopback sockets through Akagi, which would hang the game. \
                     Exclude localhost / 127.0.0.1 / ::1 from the rule that redirects the game (in \
                     Proxifier: enable the \"Localhost\" Direct rule and move it above the game rules).",
                    req.uri(),
                    ctx.client_addr,
                );
            } else {
                debug!(
                    target: "akagi::proxy::forward",
                    "refusing CONNECT to loopback {} from {}",
                    req.uri(),
                    ctx.client_addr,
                );
            }
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::empty())
                .expect("Failed to build loopback CONNECT refusal")
                .into();
        }

        // Refuse telemetry CONNECTs outright: Riichi City pins the
        // telemetry host's TLS, so its beacons never reach the
        // request-level block below — refusing the tunnel keeps them off
        // the wire deterministically, pinning or not.
        if self.block_telemetry && is_telemetry_connect(req.method(), req.uri()) {
            info!(
                target: "akagi::proxy::forward",
                "refusing telemetry CONNECT to {} — blocked before a tunnel opens",
                req.uri(),
            );
            // An annotated gap is not a gap: record the refusal.
            self.inspector.record(InspectorEntry::Http {
                ts_ms: Local::now().timestamp_millis(),
                source: CaptureSource::Mitm,
                exchange: HttpExchange {
                    exchange_id: None,
                    phase: HttpPhase::Request,
                    method: "CONNECT".to_string(),
                    url: req.uri().to_string(),
                    host: req.uri().host().unwrap_or_default().to_string(),
                    version: format!("{:?}", req.version()),
                    status: None,
                    headers: httpcap::headers_of(req.headers()),
                    body: None,
                    annotations: vec![HttpAnnotation {
                        kind: "akagi_blocked".to_string(),
                        summary: "telemetry CONNECT refused — tunnel never opened".to_string(),
                        data: serde_json::json!({
                            "reason": "Aliyun SLS web-tracking host refused at the proxy; \
                                       the beacon cannot leave the machine regardless of \
                                       certificate pinning",
                        }),
                    }],
                },
            });
            return Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(Body::empty())
                .expect("static telemetry CONNECT refusal must build")
                .into();
        }

        // Telemetry blocking comes first: a dropped beacon is never
        // forwarded, so there is nothing left for the rewrite below to
        // correct. See `block_telemetry_beacon`.
        if self.block_telemetry {
            if let Some(resp) = self.block_telemetry_beacon(&req) {
                return resp.into();
            }
        }

        // The one place Akagi deliberately changes what it forwards: the
        // client's certificate report otherwise describes Akagi's own CA.
        // See `rewrite::majsoul_cert` for what it substitutes and why.
        let mut req = req;
        if self.rewrite_cert_report && self.platform == Platform::Majsoul {
            self.rewrite_certificate_report(&mut req);
        }

        // Everything else the game asks for, recorded and forwarded
        // unaltered. This is where the analytics beacons surface, but
        // nothing here knows or cares that they are beacons — that is the
        // recognizers' job.
        let method = req.method().to_string();
        let uri = req.uri().clone();
        let url = uri.to_string();
        let host = uri
            .host()
            .map(str::to_string)
            .or_else(|| {
                req.headers()
                    .get(hyper::header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .map(|h| h.split(':').next().unwrap_or(h).to_string())
            })
            .unwrap_or_default();
        let version = format!("{:?}", req.version());
        let headers = httpcap::headers_of(req.headers());
        let annotations = annotate::annotate_request(&RequestView::new(&method, &url, &headers));
        if let Some(a) = annotations.first() {
            info!(
                target: "akagi::proxy::http",
                "recognized {} {} on {host}", a.kind, a.summary,
            );
        }

        let recording = self.http_policy.record_all || !annotations.is_empty();
        let plan = if recording {
            httpcap::plan_body(req.headers(), &self.http_policy)
        } else {
            BodyPlan::None
        };
        let (parts, body) = req.into_parts();
        let (body, body_record) = Self::take_body(plan, body, &parts.headers).await;
        let req = Request::from_parts(parts, body);

        // Only requests that will actually reach `handle_response` may go
        // on the pairing queue. hudsucker answers a CONNECT from
        // `process_connect` and an upgrade from `upgrade_websocket`, and
        // neither calls the response hook — so queueing them desynchronises
        // it, and every later response on the connection gets attributed to
        // the wrong request. That is worse than not attributing it at all.
        let exchange_id = if will_be_answered_by_handle_response(&req) {
            Some(self.pairing.open(ctx.client_addr, &method, &url, &host))
        } else {
            None
        };

        self.record_http(HttpExchange {
            exchange_id,
            phase: HttpPhase::Request,
            method,
            url,
            host,
            version,
            status: None,
            headers,
            body: body_record,
            annotations,
        });

        req.into()
    }

    /// The response half. Paired back to its request where the protocol
    /// permits — see `capture::http` for why HTTP/2 does not.
    async fn handle_response(&mut self, ctx: &HttpContext, res: Response<Body>) -> Response<Body> {
        let multiplexed = !matches!(
            res.version(),
            hyper::Version::HTTP_09 | hyper::Version::HTTP_10 | hyper::Version::HTTP_11
        );
        let claimed = self.pairing.close(ctx.client_addr, multiplexed);
        if !self.http_policy.record_all {
            return res;
        }

        let status = res.status().as_u16();
        let version = format!("{:?}", res.version());
        let headers = httpcap::headers_of(res.headers());
        let plan = httpcap::plan_body(res.headers(), &self.http_policy);
        let (parts, body) = res.into_parts();
        let (body, body_record) = Self::take_body(plan, body, &parts.headers).await;
        let res = Response::from_parts(parts, body);

        let (exchange_id, method, url, host) = match claimed {
            Some(req) => (Some(req.id), req.method, req.url, req.host),
            None => (None, String::new(), String::new(), String::new()),
        };
        let annotations = if exchange_id.is_none() && multiplexed {
            vec![HttpAnnotation {
                kind: "akagi_unpaired".to_string(),
                summary: "response not attributed to a request".to_string(),
                data: serde_json::json!({
                    "reason": "HTTP/2 multiplexes streams over one connection, \
                               and hudsucker's context carries no stream id",
                    "version": version,
                }),
            }]
        } else {
            Vec::new()
        };

        self.record_http(HttpExchange {
            exchange_id,
            phase: HttpPhase::Response,
            method,
            url,
            host,
            version,
            status: Some(status),
            headers,
            body: body_record,
            annotations,
        });
        res
    }

    /// Skip MITM only for the narrow case of `CONNECT <ip-literal>:443` on
    /// **Mahjong Soul** (see [`should_raw_tunnel`]).
    ///
    /// Some game / app clients (catfood-studio Mahjong Soul Steam build)
    /// resolve DNS themselves and connect to a CDN by raw IP, but the CDN
    /// is multi-tenant and routes by SNI / Host. If we MITM, hudsucker's
    /// `normalize_request` strips the original Host header and hyper
    /// rebuilds it from the URI (= IP); rustls uses the URI host (= IP)
    /// as SNI. The CDN can't pick a tenant from that and serves the
    /// default 404 / 403. Raw-tunneling lets the client's own TLS / Host
    /// header reach the CDN untouched.
    ///
    /// We only do this on port 443 because that's where multi-tenant CDN
    /// fronts live. Game-specific gateways and APIs use alt ports (8443
    /// for the maj-soul WS gateway, 7201 for HTTPDNS, …), are
    /// single-tenant, and need MITM to be captured. Without the port
    /// narrowing we'd bypass the WS gateway too and lose all frame capture.
    ///
    /// The bypass is **gated to Mahjong Soul**: Riichi City reaches its
    /// single-tenant game server by raw IP on 443 and presents a normal
    /// (MITM-able) certificate, so raw-tunneling there would silently drop
    /// the gameplay WebSocket — the only `<ip>:443` CONNECT (the WSS gameplay
    /// flow) would be bypassed and no frames would reach the bridge.
    ///
    /// Hostnames are always MITM'd; the maj-soul hostname endpoints
    /// (`mjusgs.mahjongsoul.com`, etc.) don't hit this code path.
    async fn should_intercept(&mut self, _ctx: &HttpContext, req: &Request<Body>) -> bool {
        let Some(host) = req.uri().host() else {
            return true;
        };
        // IPv4 CONNECT URIs always carry an explicit port; default to 443
        // only as a defensive fallback.
        let port = req.uri().port_u16().unwrap_or(443);
        if should_raw_tunnel(self.platform, host, port) {
            info!(
                target: "akagi::proxy::forward",
                "raw-tunneling IP-literal CONNECT to {}",
                req.uri()
            );
            // Record the bypass itself. Everything inside this tunnel is
            // invisible to us, and a timeline that simply omits it looks
            // identical to one where the game never talked to that host.
            // An annotated gap is not a gap.
            self.inspector.record(InspectorEntry::Http {
                ts_ms: Local::now().timestamp_millis(),
                source: CaptureSource::Mitm,
                exchange: HttpExchange {
                    exchange_id: None,
                    phase: HttpPhase::Request,
                    method: "CONNECT".to_string(),
                    url: req.uri().to_string(),
                    host: host.to_string(),
                    version: format!("{:?}", req.version()),
                    status: None,
                    headers: httpcap::headers_of(req.headers()),
                    body: None,
                    annotations: vec![HttpAnnotation {
                        kind: "akagi_bypass".to_string(),
                        summary: "raw-tunneled — contents not visible".to_string(),
                        data: serde_json::json!({
                            "reason": "IP-literal CONNECT on port 443 is tunneled so the \
                                       client's own SNI and Host reach multi-tenant CDNs intact",
                            "platform": format!("{:?}", self.platform),
                            "port": port,
                        }),
                    }],
                },
            });
            return false;
        }
        true
    }

    /// Diagnostic override of the default 502 path. The default just logs
    /// `"Failed to forward request: client error (Kind)"` with no URI and
    /// no inner cause. Walk the source chain so we can see whether a
    /// failed upstream forward was DNS, TCP, TLS handshake, ALPN, …
    /// Paired with the `handle_request` log above (which carries the
    /// host) — the two lines arrive back-to-back per failed flow.
    async fn handle_error(
        &mut self,
        ctx: &HttpContext,
        err: hudsucker::hyper_util::client::legacy::Error,
    ) -> Response<Body> {
        // A failed forward never reaches `handle_response`, so its queued
        // request has to be claimed here or the pairing desynchronises for
        // the rest of the connection.
        let claimed = self.pairing.close(ctx.client_addr, false);

        let mut chain = String::new();
        let mut next: Option<&dyn std::error::Error> = Some(&err);
        let mut depth = 0;
        while let Some(e) = next {
            if depth > 0 {
                chain.push_str(" ← ");
            }
            chain.push_str(&format!("{e}"));
            next = e.source();
            depth += 1;
            // Guard against pathological loops.
            if depth > 16 {
                chain.push_str(" ← [chain truncated]");
                break;
            }
        }
        error!(
            target: "akagi::proxy::forward",
            "upstream forward failed: client={} chain=[{chain}]",
            ctx.client_addr,
        );

        // Record it: "the game asked for this and we could not reach it"
        // is exactly the gap a timeline should show, and it is the usual
        // cause of a client stuck on its loading screen.
        if let Some(req) = claimed {
            self.record_http(HttpExchange {
                exchange_id: Some(req.id),
                phase: HttpPhase::Response,
                method: req.method,
                url: req.url,
                host: req.host,
                version: String::new(),
                status: Some(StatusCode::BAD_GATEWAY.as_u16()),
                headers: Vec::new(),
                body: None,
                annotations: vec![HttpAnnotation {
                    kind: "akagi_forward_failed".to_string(),
                    summary: "upstream forward failed".to_string(),
                    data: serde_json::json!({ "error": chain }),
                }],
            });
        }

        Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Body::empty())
            .expect("Failed to build 502")
    }
}

impl WebSocketHandler for ProxyHandler {
    async fn handle_websocket(
        mut self,
        ctx: WebSocketContext,
        mut stream: impl Stream<Item = Result<Message, tungstenite::Error>> + Unpin + Send + 'static,
        mut sink: impl Sink<Message, Error = tungstenite::Error> + Unpin + Send + 'static,
    ) {
        let client = client_addr(&ctx);
        let server_uri = server_uri(&ctx);
        let bridge = self.acquire_bridge(client, &server_uri);
        let force_close = self.force_close.clone();

        // Riichi City autoplay injects on the client→server leg only — that
        // sink leads to the game server. The bridge's `in_game` gate keeps
        // frames off the client's non-gameplay sockets (the client keeps
        // several flows open; only the one carrying a game should ever see
        // an injected command).
        let mut inject_rx = match (&ctx, &self.inject) {
            (WebSocketContext::ClientToServer { .. }, Some(bus)) => Some(bus.subscribe()),
            _ => None,
        };

        loop {
            tokio::select! {
                biased;
                _ = force_close.notified() => {
                    info!("force-closing WS flow for {client}");
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
                frame = async {
                    match inject_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    let frame = match frame {
                        Ok(f) => f,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            warn!("inject bus lagged by {n}; dropped frames");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    let in_game = self
                        .inject
                        .as_ref()
                        .is_some_and(|bus| bus.in_game());
                    if frame.gameplay && !in_game {
                        debug!("inject: not in a game on {client}; dropping injected gameplay frame");
                        continue;
                    }
                    info!("inject: forwarding injected frame to {server_uri}");
                    match sink.send(Message::Binary(frame.bytes.clone().into())).await {
                        Ok(()) => {
                            // Injected frames bypass the client socket, so
                            // without this record they are invisible on the
                            // wire — timing investigations could not tell
                            // our sends from the client's own.
                            let packets =
                                crate::bridge::riichi_city::packet::WPacket::parse_frame(&frame.bytes);
                            let parsed = packets.first().map(|p| {
                                crate::schema::ParsedFrame {
                                    method: format!("{} (injected)", p.method_label()),
                                    args: p.body.clone(),
                                }
                            });
                            self.inspector.record(InspectorEntry::WsFrame {
                                ts_ms: Local::now().timestamp_millis(),
                                direction: FrameDirection::Up,
                                flow_id: self.inspector_flow_id(client),
                                size: frame.bytes.len(),
                                raw: FrameRaw::Binary(b64(&frame.bytes)),
                                parsed,
                                emitted: 0,
                            });
                        }
                        Err(tungstenite::Error::ConnectionClosed)
                        | Err(tungstenite::Error::AlreadyClosed) => break,
                        Err(e) => {
                            error!("inject: WebSocket send error: {e}");
                            break;
                        }
                    }
                }
                next = stream.next() => {
                    let Some(message) = next else { break };
                    match message {
                        Ok(message) => {
                            let Some(out) = self.handle_message(&ctx, message, &bridge).await else {
                                continue;
                            };
                            match sink.send(out).await {
                                Ok(()) => (),
                                // Peer already gone — normal at end of game / lobby.
                                Err(tungstenite::Error::ConnectionClosed)
                                | Err(tungstenite::Error::AlreadyClosed) => break,
                                Err(e) => {
                                    error!("WebSocket send error: {e}");
                                    break;
                                }
                            }
                        }
                        Err(tungstenite::Error::ConnectionClosed)
                        | Err(tungstenite::Error::AlreadyClosed) => break,
                        Err(e) => {
                            error!("WebSocket recv error: {e}");
                            match sink.send(Message::Close(None)).await {
                                Ok(())
                                | Err(tungstenite::Error::ConnectionClosed)
                                | Err(tungstenite::Error::AlreadyClosed) => (),
                                Err(e) => error!("WebSocket close error: {e}"),
                            }
                            break;
                        }
                    }
                }
            }
        }

        self.release_bridge(client, bridge);
    }
}

impl ProxyHandler {
    async fn handle_message(
        &mut self,
        ctx: &WebSocketContext,
        msg: Message,
        bridge: &SharedBridge,
    ) -> Option<Message> {
        let client = client_addr(ctx);
        let (tag, dir, dir_arrow, uri) = match ctx {
            WebSocketContext::ServerToClient { src, .. } => (
                TAG_SERVER_TO_CLIENT,
                Direction::Down,
                '\u{2193}',
                src.to_string(),
            ),
            WebSocketContext::ClientToServer { dst, .. } => (
                TAG_CLIENT_TO_SERVER,
                Direction::Up,
                '\u{2191}',
                dst.to_string(),
            ),
        };

        match &msg {
            Message::Binary(buf) => {
                debug!("{dir_arrow} {uri} binary len={}", buf.len());
                self.binary.write(tag, buf);
                let result = {
                    let mut b = bridge.lock().expect("bridge mutex poisoned");
                    b.parse(dir, buf)
                };
                self.record_frame(client, dir, FrameRaw::Binary(b64(buf)), buf.len(), &result);
                self.dispatch_events(dir_arrow, &uri, result.events);
            }
            Message::Text(t) => {
                debug!("{dir_arrow} {uri} text len={}", t.len());
                let buf = t.as_bytes();
                self.binary.write(tag, buf);
                let result = {
                    let mut b = bridge.lock().expect("bridge mutex poisoned");
                    b.parse(dir, buf)
                };
                self.record_frame(
                    client,
                    dir,
                    FrameRaw::Text(t.to_string()),
                    buf.len(),
                    &result,
                );
                self.dispatch_events(dir_arrow, &uri, result.events);
            }
            Message::Close(_) => debug!("{dir_arrow} {uri} close"),
            _ => {}
        }

        if let Message::Frame(_) = &msg {
            warn!("unexpected raw frame at {uri}");
        }

        Some(msg)
    }

    fn dispatch_events(&self, dir_arrow: char, uri: &str, events: Vec<crate::schema::MjaiEvent>) {
        if events.is_empty() {
            return;
        }
        debug!("{dir_arrow} {uri} bridge emitted {} event(s)", events.len());
        if let Some(tx) = &self.mjai_tx {
            for ev in events {
                // No subscribers is fine — broadcast just drops.
                let _ = tx.send(ev);
            }
        }
    }

    fn record_frame(
        &self,
        client: SocketAddr,
        dir: Direction,
        raw: FrameRaw,
        size: usize,
        result: &bridge::ParseResult,
    ) {
        let direction = match dir {
            Direction::Down => FrameDirection::Down,
            Direction::Up => FrameDirection::Up,
        };
        self.inspector.record(InspectorEntry::WsFrame {
            ts_ms: Local::now().timestamp_millis(),
            direction,
            flow_id: self.inspector_flow_id(client),
            size,
            raw,
            parsed: result.parsed.clone(),
            emitted: result.events.len(),
        });
    }
}

fn b64(buf: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(buf)
}

fn client_addr(ctx: &WebSocketContext) -> SocketAddr {
    match ctx {
        WebSocketContext::ClientToServer { src, .. } => *src,
        WebSocketContext::ServerToClient { dst, .. } => *dst,
    }
}

fn server_uri(ctx: &WebSocketContext) -> Uri {
    match ctx {
        WebSocketContext::ClientToServer { dst, .. } => dst.clone(),
        WebSocketContext::ServerToClient { src, .. } => src.clone(),
    }
}

/// Sanitize the URI path into a filename-safe slug. `/game-gateway` →
/// `game-gateway`, `/` → `root`, anything outside `[A-Za-z0-9_-]` becomes
/// `_`.
fn uri_path_slug(uri: &Uri) -> String {
    let raw = uri.path().trim_matches('/');
    if raw.is_empty() {
        return "root".into();
    }
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Whether a `CONNECT host:port` should bypass MITM with a raw TCP tunnel.
///
/// True only for the Mahjong Soul multi-tenant CDN case — an IP-literal host
/// on port 443 — where MITM would set SNI = IP and the CDN would serve its
/// default error page. Every other platform (notably Riichi City, whose
/// single-tenant game server is reached by raw IP on 443 and presents a
/// MITM-able cert) is intercepted so its gameplay WebSocket is captured.
fn should_raw_tunnel(platform: Platform, host: &str, port: u16) -> bool {
    platform == Platform::Majsoul && port == 443 && is_ip_literal_host(host)
}

/// IPv6 hosts come back from `Uri::host` wrapped in `[…]` per RFC 3986;
/// `IpAddr::from_str` rejects the brackets, so strip them before parsing.
fn strip_brackets(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host)
}

/// `true` when `host` (as returned by `Uri::host`) is a literal IPv4 or
/// IPv6 address.
fn is_ip_literal_host(host: &str) -> bool {
    strip_brackets(host).parse::<std::net::IpAddr>().is_ok()
}

/// `true` when `host` names the loopback interface: any address in
/// `127.0.0.0/8`, `::1`, or the reserved name `localhost`.
fn is_loopback_host(host: &str) -> bool {
    let host = strip_brackets(host);
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}

/// Whether hudsucker will answer this request through `handle_response`.
///
/// It will not for a `CONNECT` (answered by `process_connect`) nor for a
/// WebSocket upgrade (answered by `upgrade_websocket`), so neither may be
/// queued for response pairing. Mirrors `hyper_tungstenite::is_upgrade_request`,
/// which is what hudsucker itself branches on.
fn will_be_answered_by_handle_response<B>(req: &Request<B>) -> bool {
    if req.method() == Method::CONNECT {
        return false;
    }
    !is_websocket_upgrade(req)
}

fn is_websocket_upgrade<B>(req: &Request<B>) -> bool {
    header_contains(req, hyper::header::CONNECTION, "upgrade")
        && header_contains(req, hyper::header::UPGRADE, "websocket")
}

/// `Connection` is a comma-separated list, so a substring test would be
/// wrong; compare each element.
fn header_contains<B>(req: &Request<B>, name: hyper::header::HeaderName, value: &str) -> bool {
    req.headers().get_all(name).iter().any(|v| {
        v.as_bytes()
            .split(|&c| c == b',')
            .any(|part| part.trim_ascii().eq_ignore_ascii_case(value.as_bytes()))
    })
}

/// `true` for a `CONNECT` whose authority names the loopback interface — the
/// signature of a redirector that is proxying the game's own internal sockets.
/// See the refusal in [`ProxyHandler::handle_request`] for why these must not
/// be tunneled.
fn is_loopback_connect(method: &Method, uri: &Uri) -> bool {
    method == Method::CONNECT && uri.host().is_some_and(is_loopback_host)
}

/// `true` for a `CONNECT` to the Aliyun SLS web-tracking service — the
/// telemetry vendor both Mahjong Soul and Riichi City report through. Any
/// `<project>.<region>.log.aliyuncs.com` host serves nothing but beacons,
/// so the whole service domain is matched, current and future projects
/// alike.
fn is_telemetry_connect(method: &Method, uri: &Uri) -> bool {
    fn is_sls_host(host: &str) -> bool {
        host == "log.aliyuncs.com" || host.ends_with(".log.aliyuncs.com")
    }
    method == Method::CONNECT && uri.host().is_some_and(is_sls_host)
}

/// `true` for Riichi City's plain-HTTP telemetry endpoint (login /
/// resource-download tracking in the shipped Lua, `GameDotHelper`). These
/// arrive as ordinary forwarded requests — no tunnel — so they are blocked
/// at the request level alongside the SLS beacons.
fn is_riichi_city_telemetry_path(path: &str) -> bool {
    path == "/game_manager/TslogServlet"
}

#[cfg(test)]
mod tests {
    use super::{
        is_ip_literal_host, is_loopback_connect, is_loopback_host, is_riichi_city_telemetry_path,
        is_telemetry_connect, should_raw_tunnel, will_be_answered_by_handle_response,
    };
    use crate::config::Platform;
    use hudsucker::hyper::{Method, Request, Uri};

    /// The telemetry CONNECT refusal must catch both games' Aliyun SLS
    /// hosts (current regions and future ones) while leaving every other
    /// tunnel alone — including hosts that merely contain "aliyun" or the
    /// game's own API hosts.
    #[test]
    fn telemetry_connects_are_recognized_by_host() {
        let connect = |authority: &str| {
            is_telemetry_connect(
                &Method::CONNECT,
                &authority.parse::<Uri>().expect("valid authority"),
            )
        };
        assert!(
            connect("hwmaj-client.ap-northeast-1.log.aliyuncs.com:443"),
            "Riichi City's flow-log host"
        );
        assert!(
            connect("example-client.cn-hongkong.log.aliyuncs.com:443"),
            "a Mahjong Soul project"
        );
        assert!(connect("log.aliyuncs.com:443"));
        // Not telemetry.
        assert!(!connect("aga-alb.mahjong-jp.net:443"));
        assert!(!connect("aliyun.example.com:443"));
        assert!(!connect("log.aliyuncs.com.evil.example.com:443"));
        // Plain (non-CONNECT) requests never match, even to the host.
        let uri: Uri = "https://hwmaj-client.ap-northeast-1.log.aliyuncs.com/x"
            .parse()
            .unwrap();
        assert!(!is_telemetry_connect(&Method::GET, &uri));
    }

    #[test]
    fn riichi_city_plain_http_telemetry_path_is_recognized() {
        assert!(is_riichi_city_telemetry_path("/game_manager/TslogServlet"));
        // The lobby API shares the host — only the telemetry path blocks.
        assert!(!is_riichi_city_telemetry_path("/lobbys/startStage"));
        assert!(!is_riichi_city_telemetry_path("/game_manager/Other"));
    }

    /// Regression: only requests hudsucker answers through
    /// `handle_response` may join the pairing queue.
    ///
    /// A live capture showed responses carrying `content-type:
    /// application/json` recorded as `CONNECT` — a CONNECT has no body at
    /// all. hudsucker answers a CONNECT from `process_connect` and an
    /// upgrade from `upgrade_websocket`, neither of which calls the
    /// response hook, so queueing them left an entry that was never
    /// claimed and shifted every later response on that connection onto
    /// the wrong request.
    #[test]
    fn connect_and_upgrades_are_kept_off_the_pairing_queue() {
        let connect = Request::builder()
            .method(Method::CONNECT)
            .uri("route-6.maj-soul.com:443")
            .body(())
            .unwrap();
        assert!(!will_be_answered_by_handle_response(&connect));

        // The game's gateway socket, exactly as the client opens it.
        let upgrade = Request::builder()
            .method(Method::GET)
            .uri("https://route-6.maj-soul.com/gateway")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .body(())
            .unwrap();
        assert!(!will_be_answered_by_handle_response(&upgrade));

        // An ordinary forward is answered by `handle_response`, so it does
        // belong on the queue.
        let plain = Request::builder()
            .method(Method::GET)
            .uri("https://route-6.maj-soul.com/api/clientgate/routes")
            .body(())
            .unwrap();
        assert!(will_be_answered_by_handle_response(&plain));
    }

    /// `Connection` is a comma-separated list and browsers send it with
    /// other tokens alongside, so element-wise matching is load-bearing —
    /// and a lone `Connection: keep-alive` must not read as an upgrade.
    #[test]
    fn upgrade_detection_handles_header_lists_and_casing() {
        let listed = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/ws")
            .header("Connection", "keep-alive, Upgrade")
            .header("Upgrade", "WebSocket")
            .body(())
            .unwrap();
        assert!(!will_be_answered_by_handle_response(&listed));

        let keep_alive = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/api")
            .header("Connection", "keep-alive")
            .body(())
            .unwrap();
        assert!(will_be_answered_by_handle_response(&keep_alive));

        // `Connection: Upgrade` without the websocket token is not one.
        let half = Request::builder()
            .method(Method::GET)
            .uri("https://example.com/api")
            .header("Connection", "Upgrade")
            .header("Upgrade", "h2c")
            .body(())
            .unwrap();
        assert!(will_be_answered_by_handle_response(&half));
    }

    /// Regression: a redirector matching the game for *any* target host sweeps
    /// up its internal `socketpair()` self-connect. Tunneling that deadlocks —
    /// hudsucker blocks reading the client's first bytes before it dials
    /// upstream, and a self-connect's client side never speaks first — which
    /// hung the Mahjong Soul Steam client on its loading screen forever.
    #[test]
    fn loopback_connect_is_refused() {
        let connect = Method::CONNECT;
        // The exact shape seen in the wild: game's own ephemeral listener.
        assert!(is_loopback_connect(
            &connect,
            &"127.0.0.1:65423".parse::<Uri>().unwrap()
        ));
        // The whole 127.0.0.0/8 block, IPv6 loopback, and the reserved name.
        assert!(is_loopback_connect(
            &connect,
            &"127.9.9.9:1234".parse::<Uri>().unwrap()
        ));
        assert!(is_loopback_connect(
            &connect,
            &"[::1]:8080".parse::<Uri>().unwrap()
        ));
        assert!(is_loopback_connect(
            &connect,
            &"localhost:23410".parse::<Uri>().unwrap()
        ));
    }

    /// The refusal must not touch real game traffic, nor non-`CONNECT` methods
    /// (Akagi's own `GET /ping` health probe arrives on loopback).
    #[test]
    fn non_loopback_connect_and_other_methods_pass_through() {
        assert!(!is_loopback_connect(
            &Method::CONNECT,
            &"156.238.128.63:443".parse::<Uri>().unwrap()
        ));
        assert!(!is_loopback_connect(
            &Method::CONNECT,
            &"game.maj-soul.com:443".parse::<Uri>().unwrap()
        ));
        assert!(!is_loopback_connect(
            &Method::GET,
            &"http://127.0.0.1:23410/ping".parse::<Uri>().unwrap()
        ));
        // No authority at all (origin-form request URI).
        assert!(!is_loopback_connect(
            &Method::GET,
            &"/ping".parse::<Uri>().unwrap()
        ));
    }

    #[test]
    fn loopback_hosts_are_detected() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.255.255.254"));
        assert!(is_loopback_host("[::1]"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LocalHost"));
    }

    #[test]
    fn non_loopback_hosts_are_not_detected() {
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("156.238.128.63"));
        assert!(!is_loopback_host("[2001:db8::1]"));
        assert!(!is_loopback_host("game.maj-soul.com"));
        // Hostname that merely starts with the loopback label.
        assert!(!is_loopback_host("localhost.example.com"));
        assert!(!is_loopback_host(""));
    }

    /// Mahjong Soul Steam build: IP-literal CONNECT on 443 is raw-tunneled so
    /// the multi-tenant CDN keeps the client's own SNI.
    #[test]
    fn majsoul_ip_literal_443_is_raw_tunneled() {
        assert!(should_raw_tunnel(Platform::Majsoul, "13.112.183.79", 443));
        assert!(should_raw_tunnel(Platform::Majsoul, "[2001:db8::1]", 443));
    }

    /// Regression: Riichi City's gameplay WSS goes to its single-tenant game
    /// server by raw IP on 443. It must be MITM'd — raw-tunneling silently
    /// dropped every gameplay frame in a real capture.
    #[test]
    fn riichi_city_ip_literal_443_is_intercepted() {
        assert!(!should_raw_tunnel(
            Platform::RiichiCity,
            "13.112.183.79",
            443
        ));
    }

    /// The bypass never applies to hostnames or non-443 ports, even on Majsoul.
    #[test]
    fn raw_tunnel_only_for_ip_literal_443() {
        assert!(!should_raw_tunnel(
            Platform::Majsoul,
            "mjusgs.mahjongsoul.com",
            443
        ));
        assert!(!should_raw_tunnel(Platform::Majsoul, "13.112.183.79", 80));
        assert!(!should_raw_tunnel(Platform::Majsoul, "13.112.183.79", 8443));
        assert!(!should_raw_tunnel(Platform::Tenhou, "13.112.183.79", 443));
    }

    #[test]
    fn ipv4_literal_is_detected() {
        assert!(is_ip_literal_host("156.238.128.60"));
        assert!(is_ip_literal_host("127.0.0.1"));
        assert!(is_ip_literal_host("0.0.0.0"));
    }

    #[test]
    fn ipv6_literal_with_brackets_is_detected() {
        // `Uri::host` returns IPv6 wrapped in brackets.
        assert!(is_ip_literal_host("[::1]"));
        assert!(is_ip_literal_host("[2001:db8::1]"));
        assert!(is_ip_literal_host("[fe80::1234:5678:9abc:def0]"));
    }

    #[test]
    fn hostnames_are_not_ip_literals() {
        // The hosts we explicitly do want MITM'd.
        assert!(!is_ip_literal_host("game.maj-soul.com"));
        assert!(!is_ip_literal_host("mjusgs.mahjongsoul.com"));
        assert!(!is_ip_literal_host("tenhou.net"));
        assert!(!is_ip_literal_host("localhost"));
        // Numeric-leading hostname (real-world: `3839.com` style).
        assert!(!is_ip_literal_host("4399.cn"));
    }

    #[test]
    fn edge_cases_do_not_panic() {
        // Empty / malformed inputs return false rather than panicking.
        assert!(!is_ip_literal_host(""));
        assert!(!is_ip_literal_host("["));
        assert!(!is_ip_literal_host("[]"));
        assert!(!is_ip_literal_host("[not-an-ip]"));
        // Trailing dot on a hostname.
        assert!(!is_ip_literal_host("example.com."));
    }
}
