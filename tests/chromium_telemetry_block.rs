//! With `[capture.chromium] block_telemetry` on, the game's Aliyun SLS
//! beacons must be failed inside a real browser — none may reach the wire —
//! while the page's ordinary traffic still goes through.
//!
//! Drives the real CDP loop (`cdp::run`) against a headless Chromium. The
//! browser resolves every `*.log.aliyuncs.com` host to a local sink and
//! every other name to nothing, so a beacon that escapes lands in the sink
//! (never on the real service) and the sink's request count is the proof.
//! A control phase with blocking off shows the same page *does* reach the
//! sink, which rules out "the mapping was broken" as the reason the
//! blocking phase saw nothing.
//!
//! The page fires XHR (with a CORS preflight — which carries the whole
//! payload in its URL), `<img>`, `sendBeacon` and a dedicated-worker
//! `fetch`: every route a beacon can take out of a page.
//!
//! Ignored by default: it needs a Chromium-family browser installed. Run
//! with `cargo test --test chromium_telemetry_block -- --ignored`; set
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

/// The page waits this long before firing, so the CDP loop (which polls
/// for new tabs every second) has attached and armed interception first.
const FIRE_DELAY_MS: u64 = 3000;

/// Request lines (`METHOD /path?query`) a local server has received.
type Hits = Arc<Mutex<Vec<String>>>;

/// Minimal HTTP/1.1 server: records each request line, answers `body` with
/// permissive CORS headers so a control-phase preflight succeeds, closes.
async fn serve(content_type: &'static str, body: String) -> (u16, Hits) {
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
            let body = body.clone();
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
                let request = format!(
                    "{} {}",
                    parts.next().unwrap_or_default(),
                    parts.next().unwrap_or_default()
                );
                seen.lock().unwrap().push(request);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\n\
                     Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: *\r\n\
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

/// The game page stand-in. Fake identifiers only.
fn page_html() -> String {
    format!(
        r#"<!doctype html><html><body><script>
setTimeout(() => {{
  const base = 'http://example-client.cn-hongkong.log.aliyuncs.com/logstores/client/track';
  const x = new XMLHttpRequest();
  x.open('GET', base + '?log_category=login_stats&device_id=00000000-0000-4000-8000-000000000001');
  x.setRequestHeader('x-log-apiversion', '0.6.0');
  x.send();
  new Image().src = base + '?log_category=img_beacon';
  navigator.sendBeacon(base + '?log_category=send_beacon');
  new Worker(URL.createObjectURL(new Blob([
    "fetch('" + base + "?log_category=worker_beacon', {{headers: {{'x-log-apiversion': '0.6.0'}}}})"
  ])));
  fetch('/control');
}}, {FIRE_DELAY_MS});
</script></body></html>"#
    )
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

/// Launch a headless browser, run the CDP loop on it, open the page in a
/// new tab, and wait until the page has fired. Returns the sink's hits.
async fn run_phase(session: &Arc<Session>, tmp: &TempDir, block_telemetry: bool) -> Vec<String> {
    let (sink_port, sink) = serve("text/plain", String::new()).await;
    let (page_port, page) = serve("text/html", page_html()).await;

    let profile = tmp
        .path()
        .join(format!("profile-{}", u8::from(block_telemetry)));
    std::fs::create_dir_all(&profile).expect("profile dir");
    let cfg = ChromiumConfig {
        start_url: "about:blank".into(),
        extra_args: vec![
            "--headless=new".into(),
            "--no-proxy-server".into(),
            format!(
                "--host-resolver-rules=MAP *.log.aliyuncs.com 127.0.0.1:{sink_port}, \
                 MAP log.aliyuncs.com 127.0.0.1:{sink_port}, MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"
            ),
        ],
        block_telemetry,
        ..ChromiumConfig::default()
    };
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

    // Open the page in a tab of its own: the CDP loop finds it on its next
    // poll, well inside `FIRE_DELAY_MS`.
    reqwest::Client::new()
        .put(format!(
            "http://127.0.0.1:{devtools_port}/json/new?http://127.0.0.1:{page_port}/"
        ))
        .send()
        .await
        .expect("open tab")
        .error_for_status()
        .expect("open tab status");

    // `/control` is the page's last act, so once it lands every beacon has
    // been attempted. A grace period after it lets stragglers (the worker,
    // `sendBeacon`) arrive if they are going to.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !page.lock().unwrap().iter().any(|r| r == "GET /control") {
        assert!(
            Instant::now() < deadline,
            "the page never fired (block_telemetry={block_telemetry}); \
             page server saw {:?}",
            page.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;

    cdp_loop.abort();
    launch::terminate(&mut child).await;
    let hits = sink.lock().unwrap().clone();
    hits
}

fn inspector_entries(session: &Session) -> Vec<serde_json::Value> {
    std::fs::read_to_string(session.dir().join("inspector.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("inspector line must be valid JSON"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a Chromium-family browser; run with --ignored"]
async fn beacons_never_leave_the_browser_while_ordinary_traffic_does() {
    let tmp = TempDir::new().expect("tempdir");
    let session =
        Arc::new(Session::init(&tmp.path().join("logs"), "info", "info", &[]).expect("session"));

    // Blocking on: nothing reaches the sink — not the preflight, not the
    // image, not the worker's request.
    let blocked = run_phase(&session, &tmp, true).await;
    assert!(
        blocked.is_empty(),
        "telemetry left the browser with blocking on: {blocked:?}"
    );

    // The blocks are on the timeline, each with the beacon's own annotation
    // (so we still see what it was) and the `akagi_blocked` marker.
    let entries = inspector_entries(&session);
    let beacons: Vec<_> = entries
        .iter()
        .filter(|e| {
            e["kind"] == "http"
                && e["url"]
                    .as_str()
                    .is_some_and(|u| u.contains(".log.aliyuncs.com/"))
        })
        .collect();
    assert!(
        !beacons.is_empty(),
        "blocked beacons must be on the timeline"
    );
    for row in &beacons {
        let kinds: Vec<&str> = row["annotations"]
            .as_array()
            .expect("annotations array")
            .iter()
            .filter_map(|a| a["kind"].as_str())
            .collect();
        assert!(kinds.contains(&"sls_beacon"), "{row}");
        assert!(kinds.contains(&"akagi_blocked"), "{row}");
    }
    let categories: Vec<&str> = beacons.iter().filter_map(|e| e["url"].as_str()).collect();
    for category in ["login_stats", "img_beacon", "send_beacon"] {
        assert!(
            categories.iter().any(|u| u.contains(category)),
            "{category} missing from the timeline: {categories:?}"
        );
    }

    // Control: the same page with blocking off reaches the sink, so the
    // empty sink above was the block, not a broken mapping.
    let forwarded = run_phase(&session, &tmp, false).await;
    for category in ["login_stats", "img_beacon", "send_beacon", "worker_beacon"] {
        assert!(
            forwarded.iter().any(|r| r.contains(category)),
            "{category} should reach the sink with blocking off: {forwarded:?}"
        );
    }
    assert!(
        forwarded.iter().any(|r| r.starts_with("OPTIONS ")),
        "the XHR's CORS preflight should reach the sink with blocking off: {forwarded:?}"
    );
}
