//! Capture transports — the layer that supplies WebSocket frames to the
//! [`crate::bridge::Bridge`] parser.
//!
//! Three backends today:
//! - [`hudsucker_backend::HudsuckerBackend`] — MITM proxy (legacy, requires
//!   system proxy + CA cert install).
//! - [`chromium::ChromiumBackend`] — controlled Chromium browser, intercepts
//!   WebSocket frames via CDP. No proxy/CA setup.
//! - [`ingest::IngestBackend`] — loopback WebSocket that a userscript in the
//!   user's own browser pushes the game's frames to. No proxy/CA setup.
//!
//! All implement [`CaptureBackend`] and feed frames into the platform bridge,
//! which emits mjai events on [`crate::event_bus::MjaiBus`]. Chromium and
//! ingest share [`flow::FlowBridges`]; the MITM handler keeps its own map.

pub mod chromium;
pub mod flow;
pub(crate) mod frame;
pub mod http;
pub mod hudsucker_backend;
pub mod ingest;

use crate::autoplay::AutoplayContext;
use crate::config::Platform;
use crate::event_bus::{MjaiBus, NotifyBus};
use crate::logger::Session;
use anyhow::Result;
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Notify;

/// Discriminant exposed in [`CaptureStatus`] / IPC payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureKind {
    Mitm,
    Chromium,
    Ingest,
}

impl CaptureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CaptureKind::Mitm => "mitm",
            CaptureKind::Chromium => "chromium",
            CaptureKind::Ingest => "ingest",
        }
    }
}

/// Static per-backend descriptor. The supervisor surfaces this in
/// status messages so the UI can show "Running (Chromium @ /usr/bin/google-chrome)"
/// or similar.
#[derive(Debug, Clone)]
pub struct CaptureDescriptor {
    pub kind: CaptureKind,
    /// Human-readable description (listen addr for MITM, executable path
    /// for Chromium). Surface in UI; do not parse.
    pub label: String,
}

/// Long-lived shutdown signal. The supervisor holds the `Notify` and
/// fires it when the user (or a config swap) wants the backend gone.
/// Backends should treat the future returned by [`ShutdownToken::wait`]
/// as a cooperative cancellation point.
#[derive(Clone)]
pub struct ShutdownToken(Arc<Notify>);

impl ShutdownToken {
    pub fn new() -> (Self, Arc<Notify>) {
        let n = Arc::new(Notify::new());
        (Self(n.clone()), n)
    }

    /// Resolves once shutdown has been requested.
    pub fn wait(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            self.0.notified().await;
        })
    }
}

/// Everything a backend needs to push parsed mjai events into the rest of
/// the app. Cheap to clone (all `Arc`/`Sender`/`Notify`).
#[derive(Clone)]
pub struct CaptureCtx {
    pub session: Arc<Session>,
    pub platform: Platform,
    pub mjai_bus: MjaiBus,
    pub notify_bus: NotifyBus,
    /// Shared with the autoplay manager. The chromium backend writes the
    /// per-tab `Page` handle here when it observes a Majsoul WS;
    /// autoplay reads it to dispatch `Input.dispatchMouseEvent`. The
    /// MITM backend simply ignores this — it has no `Page`.
    pub autoplay: Option<Arc<AutoplayContext>>,
    /// What to record of the HTTP traffic this backend intercepts. Both
    /// backends honour it; `static_assets` is chromium-only.
    pub http: crate::config::HttpCaptureConfig,
}

/// A capture transport. Implementors run a long-lived I/O loop until
/// `shutdown` resolves or a fatal error occurs, pushing frames into the
/// bridge layer along the way.
#[async_trait]
pub trait CaptureBackend: Send {
    /// Run the backend. Must respect `shutdown` for graceful exit.
    /// Returns `Ok(())` on clean exit, `Err` otherwise.
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()>;

    /// Static description for status reporting. Called once at spawn time
    /// (and again after restart) — must not block.
    fn descriptor(&self) -> CaptureDescriptor;
}
