//! Shared harness for the proxy HTTP-capture tests.
//!
//! Each test lives in its own binary rather than sharing one: `Session::init`
//! installs a **global** tracing subscriber, which can only be set once per
//! process, so two `#[tokio::test]`s in one binary would have the second fail
//! at startup. Splitting the binaries is cheaper than making the logger
//! re-entrant for the sake of tests.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use akagi::config::{HttpCaptureConfig, Platform, ProxyConfig};
use akagi::logger::Session;
use akagi::proxy::start_proxy;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Notify};

const TIMEOUT: Duration = Duration::from_secs(10);

/// Body the stub upstream returns — the shape of the route-topology
/// response the game actually fetches, which is one of the things that
/// used to be thrown away.
pub const UPSTREAM_BODY: &str = r#"{"data":{"routes":[{"id":"route-1","ssl":false}]}}"#;

/// A `certificate_info` beacon, shaped like the real thing but with
/// fabricated identifiers and a fabricated CA. This is the category that
/// matters: it is where a standalone client reports the certificate chain
/// it was served.
pub const BEACON_QUERY: &str = "APIVersion=0.6.0&level=info&log_category=certificate_info\
&account_id=10000001&device_id=00000000-0000-4000-8000-000000000001\
&content=%5B%7B%22issuer%22%3A%22CN%3DExample%20Test%20CA%2C%20O%3DExample%22%2C\
%22subject%22%3A%22CN%3D*.example.com%22%7D%5D";

/// Minimal HTTP/1.1 origin server. Answers every request with the same
/// JSON, keeping the connection alive, so the proxy's response path is
/// exercised for real — `handle_response` is never called on the error
/// path, so a dead upstream would not do.
pub async fn stub_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind stub");
    let port = listener.local_addr().expect("stub addr").port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let Ok(n) = sock.read(&mut chunk).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    // One response per set of request headers.
                    while let Some(end) = find_headers_end(&buf) {
                        buf.drain(..end);
                        let res = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\n\r\n{UPSTREAM_BODY}",
                            UPSTREAM_BODY.len()
                        );
                        if sock.write_all(res.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                }
            });
        }
    });
    port
}

pub fn find_headers_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

pub async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .expect("probe bind")
        .local_addr()
        .expect("probe addr")
        .port()
}

pub async fn wait_until_listening(port: u16) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("proxy never bound 127.0.0.1:{port}");
}

/// Send an absolute-form request through the proxy, the way a client
/// configured to use one does, and read the whole response.
/// Send several requests down **one** proxy connection, keep-alive, and
/// return the raw bytes of everything that came back.
///
/// One connection is the point: the request/response pairing is per
/// connection, so a desynchronising request only misattributes the
/// responses that follow it on the same socket.
pub async fn pipeline_through_proxy(proxy_port: u16, requests: &[(String, String)]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port))
        .await
        .expect("reach the proxy");
    for (i, (url, host)) in requests.iter().enumerate() {
        let last = i + 1 == requests.len();
        let conn = if last { "close" } else { "keep-alive" };
        let req = format!("GET {url} HTTP/1.1\r\nHost: {host}\r\nConnection: {conn}\r\n\r\n");
        stream.write_all(req.as_bytes()).await.expect("write");
    }
    let mut out = Vec::new();
    tokio::time::timeout(TIMEOUT, stream.read_to_end(&mut out))
        .await
        .expect("proxy never finished answering")
        .expect("read");
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn get_through_proxy(proxy_port: u16, url: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port))
        .await
        .expect("reach the proxy");
    let req = format!("GET {url} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).await.expect("write");

    let mut out = Vec::new();
    tokio::time::timeout(TIMEOUT, stream.read_to_end(&mut out))
        .await
        .expect("proxy never answered")
        .expect("read");
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Harness {
    _tmp: TempDir,
    pub proxy_port: u16,
    pub upstream_port: u16,
    inspector: std::path::PathBuf,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl Harness {
    pub async fn start(http_cfg: HttpCaptureConfig) -> Self {
        // Existing capture tests want beacons forwarded and recorded, so the
        // default harness leaves blocking off. `start_with` opts in.
        Self::start_with(http_cfg, false).await
    }

    pub async fn start_with(http_cfg: HttpCaptureConfig, block_telemetry: bool) -> Self {
        let tmp = TempDir::new().expect("tempdir");
        let session = Arc::new(
            Session::init(&tmp.path().join("logs"), "info", "info", &[]).expect("session"),
        );
        let inspector = session.dir().join("inspector.jsonl");
        let upstream_port = stub_upstream().await;
        let proxy_port = free_port().await;
        let config = ProxyConfig {
            enabled: true,
            addr: format!("127.0.0.1:{proxy_port}"),
            ca_dir: tmp.path().join("ca"),
            rewrite_certificate_report: true,
            block_telemetry,
        };
        let (stop, stop_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(start_proxy(
            config,
            http_cfg,
            Platform::Majsoul,
            session,
            None,
            None,
            Arc::new(Notify::new()),
            None,
            async move {
                stop_rx.await.unwrap_or_default();
            },
        ));
        wait_until_listening(proxy_port).await;
        Self {
            _tmp: tmp,
            proxy_port,
            upstream_port,
            inspector,
            stop: Some(stop),
            task,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.upstream_port)
    }

    pub fn host(&self) -> String {
        format!("127.0.0.1:{}", self.upstream_port)
    }

    /// Stop the proxy and read back the shareable timeline.
    pub async fn finish(self) -> Vec<serde_json::Value> {
        self.finish_both().await.0
    }

    /// Stop the proxy and read back the shareable and the full timeline.
    pub async fn finish_both(mut self) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        let _ = tokio::time::timeout(TIMEOUT, self.task).await;
        let read = |path: &std::path::Path| -> Vec<serde_json::Value> {
            std::fs::read_to_string(path)
                .unwrap_or_default()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| serde_json::from_str(l).expect("inspector line must be valid JSON"))
                .collect()
        };
        let full = self
            .inspector
            .parent()
            .unwrap()
            .join("sensitive")
            .join("inspector.jsonl");
        (read(&self.inspector), read(&full))
    }
}
