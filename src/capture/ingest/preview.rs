//! A stand-in server that only hands out the userscript.
//!
//! "Install userscript" opens the script's URL so a userscript manager can
//! pick it up, and that URL is normally served by the running ingest
//! backend. The backend starts only once the configuration is saved — after
//! the Setup wizard's Finish, or a Settings save — yet the moment someone
//! picks userscript capture is exactly when they want to install it.
//!
//! So while ingest is chosen but not running, the UI asks for this preview:
//! the same address, serving the same rendered script plus `/ping`, and
//! nothing else — no `/ingest`, so no frame reaches any bridge before
//! capture actually starts. A userscript installed from it keeps retrying
//! until the real backend comes up on that address.
//!
//! At most one preview runs. Starting the real backend stops it first
//! ([`stop_any`]), so the two never compete for the port.

use super::userscript;
use anyhow::{Context, Result};
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;
use tracing::{info, warn};

const LOG: &str = "akagi::capture::ingest::preview";

struct Running {
    addr: SocketAddr,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

/// Serialises start/stop, so overlapping calls from the UI cannot leave two
/// servers or a stopped one recorded as running.
fn slot() -> &'static Mutex<Option<Running>> {
    static SLOT: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Serve the userscript on `addr` until stopped. A preview already running
/// on `addr` is left as is; one on another address is replaced.
pub async fn start(addr: SocketAddr) -> Result<()> {
    let mut slot = slot().lock().await;
    if slot
        .as_ref()
        .is_some_and(|r| r.addr == addr && !r.task.is_finished())
    {
        return Ok(());
    }
    if let Some(old) = slot.take() {
        shut_down(old).await;
    }
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("{addr} is already in use"))?;
    let script = Arc::new(userscript::render(addr));
    let app = Router::new()
        .route(
            userscript::USERSCRIPT_PATH,
            get(move || {
                let script = script.clone();
                async move { super::userscript_response(&script) }
            }),
        )
        .route("/ping", get(|| async { "pong" }));
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let served = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await;
        if let Err(e) = served {
            warn!(target: LOG, "userscript preview on {addr} failed: {e}");
        }
    });
    info!(target: LOG, "serving the userscript on {addr} until capture starts");
    *slot = Some(Running { addr, stop, task });
    Ok(())
}

/// Stop the preview if it is the one on `addr`. Scoped to an address so a
/// late stop for an address the UI already moved away from cannot take
/// down the preview it moved to.
pub async fn stop(addr: SocketAddr) {
    let mut slot = slot().lock().await;
    if slot.as_ref().is_some_and(|r| r.addr == addr) {
        if let Some(running) = slot.take() {
            shut_down(running).await;
        }
    }
}

/// Stop whatever preview is running, and wait until its port is free.
/// Called by the ingest backend before it binds.
pub async fn stop_any() {
    if let Some(running) = slot().lock().await.take() {
        shut_down(running).await;
    }
}

async fn shut_down(running: Running) {
    let Running { addr, stop, task } = running;
    let _ = stop.send(());
    if tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .is_err()
    {
        warn!(target: LOG, "userscript preview on {addr} did not stop within 2 s");
    } else {
        info!(target: LOG, "userscript preview on {addr} stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    async fn free_addr() -> SocketAddr {
        TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
    }

    async fn get(addr: SocketAddr, path: &str) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    // One test, since the preview is process-wide state.
    #[tokio::test]
    async fn serves_only_the_userscript_and_frees_the_port_when_stopped() {
        let a = free_addr().await;
        start(a).await.unwrap();
        // Idempotent for the same address.
        start(a).await.unwrap();

        let script = get(a, "/akagi.user.js").await;
        assert!(script.starts_with("HTTP/1.1 200"), "{script}");
        assert!(script.contains(&format!("'ws://{a}/ingest'")));
        assert!(get(a, "/ping").await.ends_with("pong"));
        // No frames before capture starts.
        assert!(get(a, "/ingest").await.starts_with("HTTP/1.1 404"));

        // A stop for another address leaves it running.
        let b = free_addr().await;
        stop(b).await;
        assert!(get(a, "/ping").await.ends_with("pong"));

        // Moving to another address replaces it, and frees the old port.
        start(b).await.unwrap();
        assert!(get(b, "/ping").await.ends_with("pong"));
        TcpListener::bind(a)
            .await
            .expect("old preview released its port");

        // What the backend does before binding.
        stop_any().await;
        TcpListener::bind(b)
            .await
            .expect("port is free after stop_any");

        // A port someone else holds is reported, not ignored.
        let held = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let err = start(held.local_addr().unwrap()).await.unwrap_err();
        assert!(err.to_string().contains("already in use"), "{err}");
    }
}
