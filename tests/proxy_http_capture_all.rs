//! `record_all` is the opt-in policy: everything intercepted is recorded,
//! not just the exchanges a recognizer understood. Its own binary because
//! `Session::init` installs a process-global tracing subscriber — see
//! `tests/common/mod.rs`.

mod common;

use akagi::config::HttpCaptureConfig;
use common::{get_through_proxy, Harness, UPSTREAM_BODY};

/// `record_all` keeps the traffic that used to be discarded — and pairs
/// each response back to its request.
#[tokio::test(flavor = "multi_thread")]
async fn record_all_captures_both_halves_and_pairs_them() {
    let h = Harness::start(HttpCaptureConfig {
        record_all: true,
        ..Default::default()
    })
    .await;

    let routes = h.url("/api/clientgate/routes?platform=Steam_Win");
    let res = get_through_proxy(h.proxy_port, &routes, &h.host()).await;
    assert!(res.contains(UPSTREAM_BODY), "forwarded body was altered");

    let (shareable, entries) = h.finish_both().await;
    // Unrecognized exchanges can carry cookies and tokens: full copy only.
    assert!(
        shareable.iter().all(|e| e["kind"] != "http"),
        "unrecognized HTTP leaked into the shareable file: {shareable:?}"
    );
    let http: Vec<&serde_json::Value> = entries.iter().filter(|e| e["kind"] == "http").collect();

    let req = http
        .iter()
        .find(|e| e["phase"] == "request")
        .expect("request half must be recorded");
    let resp = http
        .iter()
        .find(|e| e["phase"] == "response")
        .expect("response half must be recorded");

    // Over HTTP/1.1 the pairing is exact, so the response knows which
    // request it answers — including its method and URL.
    assert_eq!(req["exchange_id"], resp["exchange_id"]);
    assert_eq!(resp["method"], "GET");
    assert!(resp["url"]
        .as_str()
        .unwrap()
        .contains("/api/clientgate/routes"));
    assert_eq!(resp["status"], 200);

    // A small JSON response is exactly what we want kept.
    assert_eq!(resp["body"]["text"], UPSTREAM_BODY);
    assert_eq!(resp["body"]["bytes"], UPSTREAM_BODY.len());
    assert!(resp["body"].get("skipped").is_none());

    // Header order is preserved — it is a client fingerprint in its own
    // right, and the WS upgrade request is where that matters most.
    let names: Vec<&str> = req["headers"]
        .as_array()
        .expect("headers")
        .iter()
        .map(|h| h["name"].as_str().unwrap_or_default())
        .collect();
    assert!(names.contains(&"host"), "got: {names:?}");

    // Nothing was recognized here, and that is fine: an unannotated row
    // is still a recorded row once record_all is on.
    assert!(req["annotations"].as_array().is_none_or(|a| a.is_empty()));
}
