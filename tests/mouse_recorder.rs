//! The pointer recorder, in a real browser: it must count the frames our own
//! CDP mouse events move the cursor in, report presses, and leave nothing
//! the page's main world can see.
//!
//! Ignored by default: it needs a Chromium-family browser installed. Run
//! with `cargo test --test mouse_recorder -- --ignored`; set
//! `AKAGI_TEST_CHROME` to pick the binary.

use std::time::Duration;

use akagi::autoplay::cdp_input::{mouse_button, mouse_move};
use akagi::autoplay::mouse::recorder::{self, InputBatch};
use akagi::capture::chromium::{detect, launch};
use akagi::config::ChromiumConfig;
use chromiumoxide::browser::Browser;
use chromiumoxide::cdp::js_protocol::runtime::EventBindingCalled;
use futures_util::StreamExt;
use tempfile::TempDir;

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

const PAGE: &str = "data:text/html,<!doctype html><html><body style='margin:0'>\
<canvas width='800' height='450' style='width:800px;height:450px;display:block'></canvas>\
</body></html>";

#[tokio::test]
#[ignore = "needs a Chromium-family browser"]
async fn recorder_counts_moving_frames_and_stays_out_of_the_page() {
    let tmp = TempDir::new().unwrap();
    let cfg = ChromiumConfig {
        start_url: "about:blank".into(),
        extra_args: vec!["--headless=new".into(), "--no-proxy-server".into()],
        ..ChromiumConfig::default()
    };
    let launched = launch::spawn(&chrome(), tmp.path(), &cfg).expect("spawn chromium");
    let mut child = launched.child;
    let endpoint = launch::wait_for_devtools_endpoint(tmp.path(), launched.remote_debugging_port)
        .await
        .expect("devtools endpoint");
    let (browser, mut handler) = Browser::connect(&endpoint).await.expect("connect");
    let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = browser.new_page(PAGE).await.expect("page");
    let mut calls = page
        .event_listener::<EventBindingCalled>()
        .await
        .expect("listen");
    recorder::install(&page).await.expect("install");
    tokio::time::sleep(Duration::from_millis(300)).await;

    // 60 moves 16 ms apart, then a press and release.
    for i in 0..60 {
        mouse_move(&page, 100.0 + f64::from(i) * 5.0, 200.0)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
    mouse_button(&page, 400.0, 200.0, true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    mouse_button(&page, 400.0, 200.0, false).await.unwrap();

    let mut batches: Vec<InputBatch> = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(600), calls.next()).await {
            Ok(Some(ev)) if ev.name == recorder::BINDING => {
                batches.push(recorder::parse_batch(&ev.payload).expect("batch"));
            }
            Ok(Some(_)) => {}
            _ => {}
        }
    }

    let frames: usize = batches.iter().map(|b| b.frames.len()).sum();
    let moves: Vec<_> = batches.iter().flat_map(|b| b.moves.iter()).collect();
    let buttons: Vec<_> = batches.iter().flat_map(|b| b.buttons.iter()).collect();
    assert!(frames > 60, "animation frames counted: {frames}");
    assert!(
        (20..=60).contains(&moves.len()),
        "one moving frame per frame the cursor moved in: {}",
        moves.len()
    );
    assert!(moves
        .iter()
        .all(|m| (0.0..=1.0).contains(&m.x) && (m.y - 200.0 / 450.0).abs() < 0.01));
    assert_eq!(buttons.len(), 2, "press and release: {buttons:?}");
    assert!(buttons[0].down && !buttons[1].down);
    assert_eq!(batches[0].rect, (800.0, 450.0));

    let leaked: bool = page
        .evaluate(format!(
            "typeof window['{}'] !== 'undefined' || typeof window.__akagiInputInstalled !== 'undefined'",
            recorder::BINDING
        ))
        .await
        .expect("evaluate")
        .into_value()
        .expect("bool");
    assert!(!leaked, "the main world can see the recorder");

    drop(page);
    pump.abort();
    launch::terminate(&mut child).await;
}
