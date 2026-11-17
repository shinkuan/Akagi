//! Regression: a request hudsucker does not answer through
//! `handle_response` must not desynchronise the request/response pairing.
//!
//! Its own binary because `Session::init` installs a process-global
//! tracing subscriber — see `tests/common/mod.rs`.

mod common;

use akagi::config::HttpCaptureConfig;
use common::{pipeline_through_proxy, Harness, UPSTREAM_BODY};

/// Regression: a request hudsucker does **not** answer through
/// `handle_response` must not desynchronise the pairing queue.
///
/// A failed forward is answered from `handle_error`, so its queued entry
/// has to be claimed there. When it was not, every later response on that
/// connection was attributed to the previous request — a live capture
/// showed JSON responses recorded as `CONNECT`, which has no body at all.
/// A wrongly attributed response is worse than an unattributed one.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_forward_does_not_misattribute_later_responses() {
    let h = Harness::start(HttpCaptureConfig {
        record_all: true,
        ..Default::default()
    })
    .await;

    // Port 1 on loopback refuses immediately, so the first forward fails;
    // the second is an ordinary success on the same connection.
    let dead = (
        "http://127.0.0.1:1/api/dead".to_string(),
        "127.0.0.1:1".to_string(),
    );
    let live = (h.url("/api/clientgate/routes"), h.host());
    let raw = pipeline_through_proxy(h.proxy_port, &[dead, live]).await;
    assert!(
        raw.contains(UPSTREAM_BODY),
        "the second request must still be served"
    );

    // `record_all` traffic is in the full copy under `sensitive/`.
    let entries = h.finish_both().await.1;
    let http: Vec<&serde_json::Value> = entries.iter().filter(|e| e["kind"] == "http").collect();

    // The failure is on the timeline rather than being a silent gap: the
    // game asking for something we could not reach is the usual cause of
    // a client stuck on its loading screen.
    let failed = http
        .iter()
        .find(|e| e["annotations"][0]["kind"] == "akagi_forward_failed")
        .expect("the failed forward must be recorded");
    assert_eq!(failed["status"], 502);
    assert!(failed["url"].as_str().unwrap().contains("/api/dead"));

    // And the successful response is attributed to the request that
    // actually produced it, not to the failed one before it.
    let ok = http
        .iter()
        .find(|e| e["phase"] == "response" && e["status"] == 200)
        .expect("the successful response must be recorded");
    assert!(
        ok["url"]
            .as_str()
            .unwrap()
            .contains("/api/clientgate/routes"),
        "response was attributed to the wrong request: {ok}"
    );
    assert_eq!(ok["body"]["text"], UPSTREAM_BODY);
}
