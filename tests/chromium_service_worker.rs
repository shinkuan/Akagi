//! A page the CDP loop is attached to must still be able to install a
//! service worker. Majsoul's WebGL boot awaits
//! `navigator.serviceWorker.register()` before it fetches the Unity build, so
//! a registration that never settles leaves the game on a black screen.
//!
//! The regression: chromiumoxide auto-attached to every page with
//! `waitForDebuggerOnStart: true`, so a new service worker arrived paused, and
//! chromiumoxide resumed and detached it in the same breath. When the worker's
//! main script was still loading, the resume was lost with the session and the
//! worker never left the paused-on-start state. A fast local script hides
//! that race, so the stand-in server holds `sw.js` back for a moment, the way
//! a CDN does.
//!
//! Drives the real CDP loop (`cdp::run`) against a headless Chromium.
//!
//! Ignored by default: it needs a Chromium-family browser installed. Run
//! with `cargo test --test chromium_service_worker -- --ignored`; set
//! `AKAGI_TEST_CHROME` to pick the binary.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use akagi::bridge::BridgeHooks;
use akagi::capture::chromium::{cdp, detect, launch};
use akagi::capture::flow::FlowBridges;
use akagi::config::{ChromiumConfig, HttpCaptureConfig, Platform};
use akagi::event_bus;
use akagi::logger::Session;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// The page waits this long before registering, so the CDP loop has attached
/// to the tab — and auto-attach is armed on it — first.
const REGISTER_DELAY_MS: u64 = 3000;

/// How long the server holds `sw.js` back. The hang reproduced from about
/// 300 ms on.
const SCRIPT_DELAY: Duration = Duration::from_millis(1500);

/// How long the page gives each step before reporting `timeout`.
const STEP_TIMEOUT_MS: u64 = 10_000;

/// Request lines (`METHOD /path?query`) the server has received.
type Hits = Arc<Mutex<Vec<String>>>;

fn page_html() -> String {
    format!(
        r#"<!doctype html><html><body><script>
const settle = (p) => Promise.race([
  p.then(() => 'ok', (e) => 'rejected-' + e.name),
  new Promise((res) => setTimeout(() => res('timeout'), {STEP_TIMEOUT_MS})),
]);
setTimeout(async () => {{
  const register = await settle(navigator.serviceWorker.register('/sw.js'));
  const ready = await settle(navigator.serviceWorker.ready);
  fetch('/report?register=' + register + '&ready=' + ready);
}}, {REGISTER_DELAY_MS});
</script></body></html>"#
    )
}

const SW_JS: &str = "self.addEventListener('install', () => self.skipWaiting());\n\
                     self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));\n";

/// Minimal HTTP/1.1 server: the page at `/`, the worker script at `/sw.js`
/// (held back by `SCRIPT_DELAY`), an empty 200 for anything else. Records
/// each request line.
async fn serve() -> (u16, Hits) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Hits = Arc::default();
    let seen = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let line = head.lines().next().unwrap_or_default();
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let path = parts.next().unwrap_or_default().to_string();
                seen.lock().unwrap().push(format!("{method} {path}"));
                let (content_type, body) = match path.as_str() {
                    "/" => ("text/html", page_html()),
                    "/sw.js" => {
                        tokio::time::sleep(SCRIPT_DELAY).await;
                        ("text/javascript", SW_JS.to_string())
                    }
                    _ => ("text/plain", String::new()),
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n\
                     Cache-Control: no-store\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (port, hits)
}

fn chrome() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("AKAGI_TEST_CHROME") {
        return p.into();
    }
    detect::detect_system_browsers()
        .into_iter()
        .next()
        .expect("no Chromium-family browser found; set AKAGI_TEST_CHROME")
        .path
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Chromium-family browser; run with --ignored"]
async fn attached_page_can_install_a_service_worker() {
    let tmp = TempDir::new().expect("tempdir");
    let session =
        Arc::new(Session::init(&tmp.path().join("logs"), "info", "info", &[]).expect("session"));
    let (page_port, hits) = serve().await;

    let profile = tmp.path().join("profile");
    std::fs::create_dir_all(&profile).expect("profile dir");
    let cfg = ChromiumConfig {
        start_url: "about:blank".into(),
        extra_args: vec![
            "--headless=new".into(),
            "--no-proxy-server".into(),
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1".into(),
        ],
        ..ChromiumConfig::default()
    };
    let block_telemetry = cfg.block_telemetry;
    let launched = launch::spawn(&chrome(), &profile, &cfg).expect("spawn chromium");
    let mut child = launched.child;
    let endpoint = launch::wait_for_devtools_endpoint(&profile, launched.remote_debugging_port)
        .await
        .expect("devtools endpoint");
    let devtools_port = launch::endpoint_port(&endpoint).expect("devtools port");

    let bridges = Arc::new(FlowBridges::<cdp::FlowKey>::new(
        session.clone(),
        Platform::Majsoul,
        BridgeHooks::default(),
    ));
    let endpoint_for_loop = endpoint.clone();
    let session_for_loop = session.clone();
    let cdp_loop = tokio::spawn(async move {
        cdp::run(
            &endpoint_for_loop,
            bridges,
            event_bus::mjai_bus(),
            session_for_loop.inspector(),
            None,
            HttpCaptureConfig::default(),
            event_bus::notify_bus(),
            block_telemetry,
        )
        .await
    });

    // A tab of its own, opened once the loop is connected: the registration
    // then starts under auto-attach, which is the case that hung.
    tokio::time::sleep(Duration::from_millis(500)).await;
    reqwest::Client::new()
        .put(format!(
            "http://127.0.0.1:{devtools_port}/json/new?http://127.0.0.1:{page_port}/"
        ))
        .send()
        .await
        .expect("open tab")
        .error_for_status()
        .expect("open tab status");

    let deadline = Instant::now() + Duration::from_secs(40);
    let report = loop {
        let report = hits
            .lock()
            .unwrap()
            .iter()
            .find_map(|r| r.strip_prefix("GET /report?").map(str::to_string));
        if let Some(report) = report {
            break report;
        }
        assert!(
            Instant::now() < deadline,
            "the page never reported; server saw {:?}",
            hits.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };

    cdp_loop.abort();
    launch::terminate(&mut child).await;

    assert!(
        hits.lock().unwrap().iter().any(|r| r == "GET /sw.js"),
        "the worker script was never requested"
    );
    assert_eq!(
        report, "register=ok&ready=ok",
        "service worker registration must settle while the CDP loop is attached"
    );
}
