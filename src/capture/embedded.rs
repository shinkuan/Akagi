//! Capture backend for the game running **inside Akagi's own window**.
//!
//! On Windows the embedded game is a WebView2 created by
//! `ipc::embedded_window`. WebView2 is Chromium, and it honours
//! `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` — so `lib::run` passes it
//! `--remote-debugging-port=<p>` before any webview exists, and this backend
//! is then just the Chromium backend *minus the launch*: it waits for the
//! DevTools endpoint the same way, and hands the same `cdp::run` loop the
//! same bridge wiring. Frames, MJAI events and autoplay's `Page` handle all
//! arrive through the identical code path, which is why the drag discard and
//! everything else keep working unchanged.
//!
//! What this backend deliberately does not do: spawn a browser, reclaim a
//! profile, or terminate anything at shutdown. The WebView2's lifetime is
//! the window's — Akagi closing is what ends it.
//!
//! Non-Windows platforms have no WebView2; on those the supervisor refuses
//! the mode up front (see `capture_supervisor`).

use super::{CaptureBackend, CaptureCtx, CaptureDescriptor, CaptureKind, ShutdownToken};
use crate::capture::chromium::launch;
use crate::capture::flow::FlowBridges;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tracing::{info, warn};

/// Where the embedded WebView2's DevTools endpoint lives. Filled by
/// `ipc::embedded_window::configure_webview2` before the first webview is
/// created — the environment variables that make WebView2 open a debugging
/// port only take effect at creation time, so the values are computed once
/// at startup and read here.
#[derive(Debug, Clone)]
pub struct Webview2Endpoint {
    pub port: u16,
    pub user_data_dir: PathBuf,
}

static ENDPOINT: OnceLock<Webview2Endpoint> = OnceLock::new();

pub fn set_endpoint(endpoint: Webview2Endpoint) {
    let _ = ENDPOINT.set(endpoint);
}

pub fn endpoint() -> Option<&'static Webview2Endpoint> {
    ENDPOINT.get()
}

pub struct EmbeddedBackend {
    /// Same meaning as `capture.chromium.block_telemetry` — the embedded
    /// webview is Chromium underneath, so the Aliyun SLS beacons are the
    /// same ones and the fetch-interception layer arms identically.
    block_telemetry: bool,
}

impl EmbeddedBackend {
    pub fn new(block_telemetry: bool) -> Self {
        Self { block_telemetry }
    }
}

#[async_trait]
impl CaptureBackend for EmbeddedBackend {
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()> {
        let endpoint = endpoint().context(
            "the embedded webview's DevTools endpoint was never configured — \
             embedded capture requires the main window to have been built with \
             WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS set",
        )?;
        info!(
            "embedded backend: attaching to webview CDP on port {} (profile {})",
            endpoint.port,
            endpoint.user_data_dir.display()
        );

        // Same discovery the Chromium backend uses: the DevToolsActivePort
        // file WebView2 writes into its user-data folder, falling back to
        // the /json/version probe on the port we chose.
        let cdp_endpoint =
            launch::wait_for_devtools_endpoint(&endpoint.user_data_dir, Some(endpoint.port))
                .await
                .context("reading the embedded webview's CDP endpoint")?;
        info!("embedded backend: CDP endpoint {cdp_endpoint}");

        let mut hooks = ctx
            .autoplay
            .as_ref()
            .map(|a| crate::bridge::BridgeHooks {
                time_budget: Some(a.time_budget.clone()),
                input_watch: Some(a.input_watch.clone()),
                tenhou_state: Some(a.tenhou_state.clone()),
                // No injection relay on the embedded path; Riichi City
                // autoplay is MITM-only, and embedded mode is web-only.
                riichi_inject: None,
                notify: None,
                tenhou_journal: None,
            })
            .unwrap_or_default();
        hooks.notify = Some(ctx.notify_bus.clone());
        hooks.tenhou_journal = Some(crate::bridge::tenhou::journal::new_shared());
        let bridges = Arc::new(FlowBridges::<crate::capture::chromium::cdp::FlowKey>::new(
            ctx.session.clone(),
            ctx.platform,
            hooks,
        ));

        let cdp_run = crate::capture::chromium::cdp::run(
            &cdp_endpoint,
            bridges,
            ctx.mjai_bus.clone(),
            ctx.session.inspector(),
            ctx.autoplay.clone(),
            ctx.http.clone(),
            ctx.notify_bus.clone(),
            self.block_telemetry,
        );
        let mut cdp_fut = Box::pin(cdp_run);
        let shutdown_fut = shutdown.wait();
        tokio::pin!(shutdown_fut);

        // No child process to watch: the webview belongs to the window, so
        // the only two terminal arms are "shutdown asked" and "CDP loop
        // ended on its own".
        tokio::select! {
            biased;
            _ = &mut shutdown_fut => {
                info!("embedded backend: shutdown requested");
                Ok(())
            }
            r = &mut cdp_fut => {
                match &r {
                    Ok(()) => info!("embedded backend: CDP loop exited cleanly"),
                    Err(e) => warn!("embedded backend: CDP loop error: {e:#}"),
                }
                r
            }
        }
    }

    fn descriptor(&self) -> CaptureDescriptor {
        CaptureDescriptor {
            kind: CaptureKind::Embedded,
            label: "embedded webview".to_string(),
        }
    }
}
