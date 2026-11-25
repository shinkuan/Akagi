//! `CaptureBackend` impl that delegates to the existing hudsucker MITM
//! proxy. Zero behaviour change from previous releases — this is just an
//! adapter so the supervisor can multiplex MITM and Chromium uniformly.

use super::{CaptureBackend, CaptureCtx, CaptureDescriptor, CaptureKind, ShutdownToken};
use crate::config::{HttpCaptureConfig, ProxyConfig};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Notify;
use tracing::info;

pub struct HudsuckerBackend {
    proxy_cfg: ProxyConfig,
    http_cfg: HttpCaptureConfig,
    /// Shared with `AppState.capture_control.force_close` — `notify_waiters`
    /// kicks every in-flight WS so existing flows actually disconnect (not
    /// just drain naturally) when the supervisor stops the backend.
    pub force_close: Arc<Notify>,
}

impl HudsuckerBackend {
    pub fn new(
        proxy_cfg: ProxyConfig,
        http_cfg: HttpCaptureConfig,
        force_close: Arc<Notify>,
    ) -> Self {
        Self {
            proxy_cfg,
            http_cfg,
            force_close,
        }
    }
}

#[async_trait]
impl CaptureBackend for HudsuckerBackend {
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()> {
        let addr = self.proxy_cfg.addr.clone();
        info!("hudsucker backend starting on {addr}");

        // hudsucker takes a `Future<Output = ()>` graceful-shutdown signal.
        // Bridge the supervisor's `ShutdownToken` into a oneshot-like future.
        let shutdown_fut = async move {
            shutdown.wait().await;
        };

        crate::proxy::start_proxy(
            self.proxy_cfg,
            self.http_cfg,
            ctx.platform,
            ctx.session,
            Some(ctx.mjai_bus),
            Some(ctx.notify_bus),
            self.force_close,
            // Riichi City and HoloMahjong autoplay send through the relays;
            // the click platforms' slots go unused here (no page).
            ctx.autoplay.clone(),
            shutdown_fut,
        )
        .await
    }

    fn descriptor(&self) -> CaptureDescriptor {
        CaptureDescriptor {
            kind: CaptureKind::Mitm,
            label: self.proxy_cfg.addr.clone(),
        }
    }
}
