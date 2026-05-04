//! Chromium capture backend.
//!
//! Launches a Chromium-family browser with `--user-data-dir` (so it
//! doesn't collide with the user's existing Chrome) and `--remote-debugging-port=0`,
//! then connects to it via the Chrome DevTools Protocol and intercepts
//! `Network.webSocketFrameReceived/Sent` for binary frames. Frames are
//! routed into the platform [`crate::bridge::Bridge`] just as the
//! hudsucker backend does.
//!
//! No CA cert. No system proxy. The user just plays the game in the
//! Akagi-spawned browser window.

pub mod cdp;
pub mod cft;
pub mod detect;
pub mod launch;
pub mod profile;

use super::{CaptureBackend, CaptureCtx, CaptureDescriptor, CaptureKind, ShutdownToken};
use crate::capture::flow::FlowBridges;
use crate::config::ChromiumConfig;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

pub struct ChromiumBackend {
    cfg: ChromiumConfig,
}

impl ChromiumBackend {
    pub fn new(cfg: ChromiumConfig) -> Self {
        Self { cfg }
    }

    /// Resolve the chrome executable. Resolution order:
    /// 1. Explicit `cfg.executable` if set (must exist).
    /// 2. If `force_cft = false`: first auto-detected system browser.
    /// 3. Installed Chrome-for-Testing (latest version, or pinned via
    ///    `cfg.cft_channel` if a literal version is installed).
    /// 4. Error pointing the user at the Settings UI to install CfT.
    fn resolve_executable(&self) -> Result<PathBuf> {
        if !self.cfg.executable.is_empty() {
            let p = PathBuf::from(&self.cfg.executable);
            if !p.exists() {
                anyhow::bail!(
                    "configured chromium executable does not exist: {}",
                    p.display()
                );
            }
            return Ok(p);
        }

        if !self.cfg.force_cft {
            if let Some(b) = detect::detect_system_browsers().into_iter().next() {
                return Ok(b.path);
            }
        }

        let pinned = cft::Channel::parse(&self.cfg.cft_channel);
        if let Some(exe) = cft::installed_executable(&pinned) {
            return Ok(exe);
        }

        anyhow::bail!(
            "no Chromium-family browser detected and no Chrome-for-Testing installed. \
             Open Settings → Capture and click Download to install Chrome for Testing, \
             or set capture.chromium.executable explicitly."
        )
    }
}

#[async_trait]
impl CaptureBackend for ChromiumBackend {
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()> {
        let exe = self
            .resolve_executable()
            .context("resolving chromium executable")?;
        let profile_dir = profile::resolve_profile_dir(&self.cfg.user_data_dir)?;
        std::fs::create_dir_all(&profile_dir)
            .with_context(|| format!("creating chromium profile dir {}", profile_dir.display()))?;
        if let Err(e) = profile::clear_stale_singleton(&profile_dir) {
            warn!(
                "singleton-lock cleanup at {} failed: {e:#}",
                profile_dir.display()
            );
        }

        info!(
            "chromium backend starting: exe={} profile={}",
            exe.display(),
            profile_dir.display()
        );

        let mut child =
            launch::spawn(&exe, &profile_dir, &self.cfg).context("launching chromium")?;

        let cdp_endpoint = launch::wait_for_devtools_port(&profile_dir)
            .await
            .context("reading DevToolsActivePort (chromium failed to start?)")?;
        info!("chromium CDP endpoint: {cdp_endpoint}");

        let bridges = Arc::new(FlowBridges::<cdp::FlowKey>::new(
            ctx.session.clone(),
            ctx.platform,
        ));

        let cdp_run = cdp::run(
            &cdp_endpoint,
            bridges.clone(),
            ctx.mjai_bus.clone(),
            ctx.session.inspector(),
            ctx.autoplay.clone(),
        );
        let mut cdp_fut = Box::pin(cdp_run);
        let shutdown_fut = shutdown.wait();
        tokio::pin!(shutdown_fut);

        let result = tokio::select! {
            biased;
            _ = &mut shutdown_fut => {
                info!("chromium backend: shutdown requested");
                Ok(())
            }
            r = &mut cdp_fut => {
                match &r {
                    Ok(()) => info!("chromium backend: CDP loop exited cleanly"),
                    Err(e) => warn!("chromium backend: CDP loop error: {e:#}"),
                }
                r
            }
            status = child.wait() => {
                match status {
                    Ok(s) => {
                        warn!("chromium backend: browser exited (status {s})");
                        Err(anyhow::anyhow!("browser exited unexpectedly: {s}"))
                    }
                    Err(e) => Err(anyhow::anyhow!("child wait error: {e}")),
                }
            }
        };

        // Best-effort browser shutdown.
        launch::terminate(&mut child).await;
        result
    }

    fn descriptor(&self) -> CaptureDescriptor {
        let label = if self.cfg.executable.is_empty() {
            "auto-detect".to_string()
        } else {
            self.cfg.executable.clone()
        };
        CaptureDescriptor {
            kind: CaptureKind::Chromium,
            label,
        }
    }
}
