//! The ingest capture backend end to end, over real sockets.
//!
//! A source connects the way the userscript does — a WebSocket handshake
//! carrying the game page's `Origin` — announces itself, opens a flow and
//! pushes Tenhou frames. What must come out is what the Chromium backend
//! would have produced from the same frames: mjai events on the bus and
//! `ws_frame` rows on the Inspector timeline.
//!
//! Also pinned: a page from any other site is refused at the handshake, a
//! game page for the wrong platform is turned away, and the server hands
//! out the userscript it was built for.
//!
//! One test, not several: `Session::init` installs a global tracing
//! subscriber, which can only happen once per process.

use std::sync::Arc;
use std::time::Duration;

use akagi::capture::ingest::IngestBackend;
use akagi::capture::{CaptureBackend, CaptureCtx, ShutdownToken};
use akagi::config::{HttpCaptureConfig, IngestConfig, Platform};
use akagi::event_bus::{mjai_bus, notify_bus};
use akagi::logger::Session;
use akagi::schema::MjaiEvent;
use futures_util::{SinkExt, StreamExt};
use hudsucker::tokio_tungstenite::tungstenite::client::IntoClientRequest;
use hudsucker::tokio_tungstenite::tungstenite::http::HeaderValue;
use hudsucker::tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use hudsucker::tokio_tungstenite::tungstenite::{Error as WsError, Message};
use hudsucker::tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const TIMEOUT: Duration = Duration::from_secs(10);

type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_until_listening(port: u16) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("ingest endpoint never bound 127.0.0.1:{port}");
}

async fn connect(port: u16, origin: Option<&str>) -> Result<Client, WsError> {
    let mut req = format!("ws://127.0.0.1:{port}/ingest")
        .into_client_request()
        .unwrap();
    if let Some(o) = origin {
        req.headers_mut()
            .insert("Origin", HeaderValue::from_str(o).unwrap());
    }
    connect_async(req).await.map(|(ws, _)| ws)
}

async fn send(ws: &mut Client, json: String) {
    ws.send(Message::Text(json.into())).await.unwrap();
}

fn hello(origin: &str) -> String {
    format!(r#"{{"t":"hello","v":1,"agent":"test","origin":"{origin}","url":"{origin}/4/"}}"#)
}

fn text_frame(flow: u32, data: &str) -> String {
    serde_json::json!({"t": "frame", "flow": flow, "dir": "down", "kind": "text", "data": data})
        .to_string()
}

async fn http_get(port: u16, path: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    tokio::time::timeout(TIMEOUT, s.read_to_string(&mut out))
        .await
        .expect("http response")
        .unwrap();
    out
}

#[tokio::test]
async fn frames_pushed_by_a_game_page_reach_the_bridge() {
    let tmp = TempDir::new().unwrap();
    let session =
        Arc::new(Session::init(&tmp.path().join("logs"), "info", "info", &[]).expect("session"));
    let inspector_path = session.dir().join("inspector.jsonl");
    let mjai = mjai_bus();
    let notify = notify_bus();
    let mut events = mjai.subscribe();
    let mut toasts = notify.subscribe();

    let port = free_port().await;
    let backend = Box::new(IngestBackend::new(IngestConfig {
        addr: format!("127.0.0.1:{port}"),
    }));
    let ctx = CaptureCtx {
        session,
        platform: Platform::Tenhou,
        mjai_bus: mjai.clone(),
        notify_bus: notify.clone(),
        autoplay: None,
        http: HttpCaptureConfig::default(),
    };
    let (token, stop) = ShutdownToken::new();
    let task = tokio::spawn(backend.run(ctx, token));
    wait_until_listening(port).await;

    // ---- The server answers plain HTTP: liveness and the userscript. ----
    assert!(http_get(port, "/ping").await.ends_with("pong"));
    let script = http_get(port, "/akagi.user.js").await;
    assert!(script.contains("// ==UserScript=="));
    assert!(script.contains(&format!("'ws://127.0.0.1:{port}/ingest'")));
    assert!(script.contains("// @match        https://*.tenhou.net/*"));

    // ---- Any other site is refused before a WebSocket exists. ----
    match connect(port, Some("https://evil.example")).await {
        Err(WsError::Http(resp)) => assert_eq!(resp.status(), 403),
        other => panic!("a foreign origin must get 403, got {other:?}"),
    }

    // ---- A game page for another platform is turned away. ----
    let mut majsoul = connect(port, Some("https://game.maj-soul.com"))
        .await
        .expect("a game origin passes the handshake");
    send(&mut majsoul, hello("https://game.maj-soul.com")).await;
    let closed = tokio::time::timeout(TIMEOUT, majsoul.next())
        .await
        .expect("close frame");
    match closed {
        Some(Ok(Message::Close(Some(frame)))) => {
            assert_eq!(frame.code, CloseCode::Policy);
            assert_eq!(frame.reason.as_str(), "platform mismatch");
        }
        other => panic!("expected a policy close, got {other:?}"),
    }

    // ---- The Tenhou page: hello, open, frames. ----
    let mut tenhou = connect(port, Some("https://tenhou.net")).await.unwrap();
    send(&mut tenhou, hello("https://tenhou.net")).await;
    send(
        &mut tenhou,
        r#"{"t":"open","flow":1,"url":"wss://example.mjv.jp/"}"#.to_string(),
    )
    .await;
    for frame in [
        r#"{"tag":"GO","type":"9","lobby":"0"}"#,
        r#"{"tag":"UN","n0":"us","n1":"shimocha","n2":"toimen","n3":"kamicha"}"#,
        r#"{"tag":"TAIKYOKU","oya":"0"}"#,
        r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"4,8,12,36,40,44,72,76,80,108,112,116,120"}"#,
    ] {
        send(&mut tenhou, text_frame(1, frame)).await;
    }

    let mut saw_start_game = false;
    let mut saw_start_kyoku = false;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !(saw_start_game && saw_start_kyoku) {
        let ev = tokio::time::timeout_at(deadline, events.recv())
            .await
            .expect("mjai events from the pushed frames")
            .unwrap();
        match ev {
            MjaiEvent::StartGame { .. } => saw_start_game = true,
            MjaiEvent::StartKyoku { .. } => saw_start_kyoku = true,
            _ => {}
        }
    }

    // The wrong-platform page and the Tenhou page each got a toast saying so.
    let mut toast_ids = Vec::new();
    while let Ok(t) = toasts.try_recv() {
        toast_ids.extend(t.id);
    }
    assert!(
        toast_ids.contains(&"ingest-platform".to_string()),
        "{toast_ids:?}"
    );
    assert!(
        toast_ids.contains(&"ingest-source".to_string()),
        "{toast_ids:?}"
    );

    // ---- Shutdown closes the source and ends the backend cleanly. ----
    stop.notify_waiters();
    let closed = tokio::time::timeout(TIMEOUT, tenhou.next())
        .await
        .expect("the source is closed on shutdown");
    assert!(
        matches!(closed, Some(Ok(Message::Close(_))) | None),
        "{closed:?}"
    );
    tokio::time::timeout(TIMEOUT, task)
        .await
        .expect("backend stops")
        .unwrap()
        .expect("clean exit");

    // ---- Every frame landed on the Inspector timeline, on its flow. ----
    let rows: Vec<serde_json::Value> = std::fs::read_to_string(&inspector_path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let frames: Vec<&serde_json::Value> = rows.iter().filter(|r| r["kind"] == "ws_frame").collect();
    assert_eq!(frames.len(), 4, "{rows:#?}");
    assert!(frames.iter().all(|f| f["direction"] == "down"));
    assert!(frames
        .iter()
        .all(|f| f["flow_id"].as_str().unwrap().starts_with("ingest:")));
}
