//! The drag discard dispatches a real mouse gesture through CDP:
//! mousedown on the tile, a multi-event travel upward with the button
//! down, then a mouseup above the press point.
//!
//! Drives a headless Chromium against a local page that records every
//! document-level mouse event, then replays the recording and asserts the
//! gesture's shape. The page needs no game code — CDP mouse events reach
//! document listeners exactly as they would reach Laya's canvas input
//! stack — so what this proves is the *event sequence*: press before any
//! move, moves only while the button is down, release above the press,
//! and a realistic cadence (many small moves, not one jump).
//!
//! Ignored by default: it needs a Chromium-family browser installed. Run
//! with `cargo test --test cdp_drag -- --ignored`; set `AKAGI_TEST_CHROME`
//! to pick the binary.

use std::time::Duration;

use akagi::autoplay::cdp_input::{dispatch_drag, evaluate_canvas_rect};
use akagi::autoplay::drag::drag_path;
use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Headless viewport (CSS px), kept modest: the assertions are all
/// relative to the canvas rect the page reports, so the exact size does
/// not matter — this only needs to be big enough to plan a gesture in.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

fn chrome() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("AKAGI_TEST_CHROME") {
        return p.into();
    }
    akagi::capture::chromium::detect::detect_system_browsers()
        .into_iter()
        .next()
        .expect("no Chromium-family browser found; set AKAGI_TEST_CHROME")
        .path
}

/// Minimal HTTP/1.1 server serving `body` for every request, so the
/// browser has an origin to load (a `data:` URL would work too, but a
/// local origin keeps CSP and about:-scheme quirks out of the picture).
async fn serve(body: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let _ = sock.read(&mut buf).await; // request head; contents irrelevant
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.flush().await;
            });
        }
    });
    port
}

/// Page that records every document-level mouse event into
/// `window.__events` as `{type, x, y, buttons}`.
const RECORDER_PAGE: &str = r#"<!doctype html><html><head><style>
html,body{margin:0}canvas{width:100vw;height:100vh;display:block}
</style></head><body><canvas id="game"></canvas><script>
window.__events = [];
for (const type of ['mousedown', 'mousemove', 'mouseup']) {
  document.addEventListener(type, (e) => {
    window.__events.push({type: type, x: e.clientX, y: e.clientY, buttons: e.buttons});
  });
}
</script></body></html>"#;

async fn recorded_events(page: &chromiumoxide::page::Page) -> Vec<(String, f64, f64, f64)> {
    let result = page
        .evaluate("JSON.stringify(window.__events)")
        .await
        .expect("read events");
    let raw = result.into_value::<String>().expect("stringify events");
    serde_json::from_str::<Vec<serde_json::Value>>(&raw)
        .expect("parse events")
        .into_iter()
        .map(|e| {
            (
                e["type"].as_str().unwrap().to_string(),
                e["x"].as_f64().unwrap(),
                e["y"].as_f64().unwrap(),
                e["buttons"].as_f64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
#[ignore = "needs a real browser binary via AKAGI_TEST_CHROME"]
async fn drag_dispatches_a_press_travel_release_gesture() {
    let port = serve(RECORDER_PAGE).await;

    let config = BrowserConfig::builder()
        .chrome_executable(chrome())
        .arg("--no-proxy-server")
        .window_size(WIDTH, HEIGHT)
        .build()
        .expect("browser config");
    let (browser, mut handler) = Browser::launch(config).await.expect("launch chromium");
    // chromiumoxide only processes CDP messages while its handler is
    // polled; without this the page hangs on every command.
    let _handler_task = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = browser
        .new_page(format!("http://127.0.0.1:{port}/"))
        .await
        .expect("open page");
    // Give the document a beat to install its listeners before dispatching.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // The rect the manager would see, and the coordinate translation it
    // would apply — the gesture is planned in the same 16:9-normalised
    // terms regardless of the viewport the headless browser picks.
    let rect = evaluate_canvas_rect(&page).await.expect("canvas rect");
    assert!(
        rect.width >= 300.0 && rect.height >= 200.0,
        "canvas has a real size, got {rect:?}"
    );
    // A discard-shaped gesture: press on a hand tile (y ≈ 6.9), release
    // above the rack (y ≈ 3.4), drifted slightly left — the same shape
    // the manager plans.
    let press = rect.pixel(8.0, 6.9);
    let release = rect.pixel(7.7, 3.4);
    let path = drag_path(press, release, 300, &mut rand::rng());
    assert!(path.len() >= 8, "a real gesture has many waypoints");

    dispatch_drag(&page, 50, 60, &path)
        .await
        .expect("dispatch drag");

    let events = recorded_events(&page).await;
    // The dispatcher's internal sleeps (hover 50ms + hold 60ms + travel
    // ~300ms) are done; the recorder must have seen the whole gesture.
    assert!(!events.is_empty(), "no mouse events recorded");

    // 1. Exactly one press, before every move, at the path's first point.
    let presses: Vec<_> = events
        .iter()
        .enumerate()
        .filter(|(_, (t, ..))| t == "mousedown")
        .collect();
    assert_eq!(
        presses.len(),
        1,
        "expected exactly one mousedown: {events:?}"
    );
    let (press_idx, _) = presses[0];
    let want = path[0].point;
    let (_, mx, my, _) = events[press_idx];
    assert!(
        (mx - want.0).abs() <= 1.0 && (my - want.1).abs() <= 1.0,
        "mousedown at ({mx},{my}), wanted the jittered press point ({:?})",
        want
    );
    assert!(
        events[..press_idx].iter().all(|(t, ..)| t == "mousemove"),
        "hover moves may precede the press, nothing else"
    );

    // 2. Every move and the release happen while the button is down;
    //    the release is the LAST event.
    assert_eq!(
        events.last().unwrap().0,
        "mouseup",
        "the release must be the final event: {events:?}"
    );
    for (t, _, _, buttons) in &events[press_idx..events.len() - 1] {
        assert_eq!(*buttons, 1.0, "{t} fired with the button already up");
    }

    // 3. The gesture travels: the release sits above the press by the
    //    planned fraction of the canvas (y 6.9 → 3.4 ≈ 39% of its
    //    height), reached through many small moves — not one teleport.
    let (_, rx, ry, _) = *events.last().unwrap();
    assert!(
        ry < my - 0.30 * rect.height,
        "release y {ry} should be far above press y {my}"
    );
    let moves: Vec<_> = events[press_idx + 1..events.len() - 1]
        .iter()
        .filter(|(t, ..)| t == "mousemove")
        .collect();
    assert!(
        moves.len() >= 6,
        "the travel should be many small moves, got {}",
        moves.len()
    );
    let mut max_step: f64 = 0.0;
    for w in moves.windows(2) {
        let step = (w[1].1 - w[0].1).hypot(w[1].2 - w[0].2);
        max_step = max_step.max(step);
    }
    assert!(
        max_step < (my - ry) / 2.0,
        "no single move may cover half the travel (max step {max_step}px) — that is a teleport, not a drag"
    );

    // 4. The release lands near the planned release point (the pullback
    //    correction is within a few px of it).
    assert!(
        (rx - release.0).abs() <= 12.0 && (ry - release.1).abs() <= 12.0,
        "release at ({rx},{ry}), planned near ({:?})",
        release
    );
}
