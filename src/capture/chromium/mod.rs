//! Chromium capture backend.
//!
//! Launches a Chromium-family browser with `--user-data-dir` (so it
//! doesn't collide with the user's existing Chrome) and a remote debugging port,
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
        // A browser we previously launched may still be running with this
        // profile (e.g. the user closed Akagi but left Chrome open). Spawning a
        // second `--user-data-dir` instance while the first is alive only opens
        // a duplicate tab in it and then exits, leaving capture with no DevTools
        // endpoint. (Two Akagi instances against one profile is unsupported.)
        // `reclaim_singleton` finds and terminates that browser before we
        // relaunch — via the `SingletonLock` symlink on Unix, or, on Windows
        // (where Chrome writes no such file), by matching its command-line
        // `--user-data-dir`. It blocks, so run it off the async runtime.
        {
            let pd = profile_dir.clone();
            tokio::task::spawn_blocking(move || profile::reclaim_singleton(&pd))
                .await
                .context("reclaim-singleton task panicked")?
                .with_context(|| {
                    format!("reclaiming chromium profile {}", profile_dir.display())
                })?;
        }

        info!(
            "chromium backend starting: exe={} profile={}",
            exe.display(),
            profile_dir.display()
        );

        let launched =
            launch::spawn(&exe, &profile_dir, &self.cfg).context("launching chromium")?;
        let mut child = launched.child;

        let cdp_endpoint =
            launch::wait_for_devtools_endpoint(&profile_dir, launched.remote_debugging_port)
                .await
                .context("reading chromium CDP endpoint (chromium failed to start?)")?;
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

        // Race shutdown, the CDP loop, and the spawned process's exit. The
        // shutdown and CDP arms are terminal. The child-exit arm may not be:
        // after a launcher handoff (see below) we merely disable that arm via
        // its `if !child_exited` precondition and loop, so shutdown and CDP
        // keep racing — one set of arms instead of a duplicated inner select.
        let mut child_exited = false;
        let result = loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_fut => {
                    info!("chromium backend: shutdown requested");
                    break Ok(());
                }
                r = &mut cdp_fut => {
                    match &r {
                        Ok(()) => info!("chromium backend: CDP loop exited cleanly"),
                        Err(e) => warn!("chromium backend: CDP loop error: {e:#}"),
                    }
                    break r;
                }
                status = child.wait(), if !child_exited => {
                    child_exited = true;
                    // A clean exit of the process we spawned is not necessarily
                    // the browser dying: Edge (and Chrome in some setups) may
                    // relaunch itself with the same arguments and exit the
                    // original process with code 0 — a launcher handoff. The
                    // relaunched browser keeps serving our CDP endpoint, so probe
                    // it before declaring the browser dead.
                    let handoff = matches!(&status, Ok(s) if s.success())
                        && match launch::endpoint_port(&cdp_endpoint) {
                            Some(port) => launch::devtools_http_alive(port).await,
                            None => false,
                        };
                    if handoff {
                        info!(
                            "chromium backend: spawned process exited but the CDP \
                             endpoint is still alive (launcher handoff); continuing"
                        );
                        // Loop again with the child arm disabled.
                    } else {
                        break match status {
                            Ok(s) => {
                                warn!("chromium backend: browser exited (status {s})");
                                Err(anyhow::anyhow!("browser exited unexpectedly: {s}"))
                            }
                            Err(e) => Err(anyhow::anyhow!("child wait error: {e}")),
                        };
                    }
                }
            }
        };

        // The capture session is over — clear the autoplay page handle (and
        // its cached canvas rect). The page-poll reap can't cover teardown:
        // it needs a live `browser.pages()` call, which is exactly what is
        // gone here, and a dead-but-Some handle would defeat the downstream
        // "no page" guards (autoplay silently skipping, `autostart_start`
        // refusing) until the next capture session binds a fresh one.
        if let Some(ap) = &ctx.autoplay {
            *ap.page.write().await = None;
            *ap.canvas_rect.write().await = None;
        }

        // Best-effort shutdown of the process we spawned. After a launcher
        // handoff this only reaps the (already dead) launcher and the real
        // browser is deliberately left running: the user may be mid-match,
        // and a surviving browser keeps the game connected even when Akagi
        // itself stops or crashes. The next capture start reclaims it (see
        // `reclaim_singleton` above) before relaunching.
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
