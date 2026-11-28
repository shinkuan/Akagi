mod ca;
pub mod certstore;
pub mod h2c;
mod handler;
pub mod rewrite;
mod upstream;

pub use handler::ProxyHandler;

use crate::{
    config::{HttpCaptureConfig, Platform, ProxyConfig},
    event_bus::{MjaiBus, NotifyBus},
    logger::Session,
    util::resolve_dir,
};
use anyhow::{Context, Result};
use hudsucker::Proxy;
use std::{future::Future, net::SocketAddr, str::FromStr, sync::Arc};
use tokio::sync::Notify;
use tracing::info;

/// Build and run the MITM proxy until `shutdown` resolves.
///
/// The argument list is long because this is the composition root for the
/// proxy: every collaborator it needs is injected rather than reached for,
/// which is what lets the integration tests drive it over a real socket
/// with buses and autoplay left out.
#[allow(clippy::too_many_arguments)]
pub async fn start_proxy<F>(
    config: ProxyConfig,
    http_cfg: HttpCaptureConfig,
    platform: Platform,
    session: Arc<Session>,
    mjai_tx: Option<MjaiBus>,
    notify_tx: Option<NotifyBus>,
    force_close: Arc<Notify>,
    autoplay: Option<Arc<crate::autoplay::AutoplayContext>>,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let ca_dir = resolve_dir(&config.ca_dir);
    info!("Using CA dir: {}", ca_dir.display());

    let ca = ca::load_or_generate(&ca_dir)?;
    // Shared between the TLS verifier (which records what origins serve)
    // and the handler (which puts those values back into the client's
    // certificate report). See `certstore` and `rewrite::majsoul_cert`.
    let certs = Arc::new(certstore::CertStore::default());
    let addr = SocketAddr::from_str(&config.addr)
        .with_context(|| format!("Invalid proxy addr: {}", config.addr))?;

    let handler = ProxyHandler::new(
        session.clone(),
        platform,
        mjai_tx,
        notify_tx,
        force_close,
        http_cfg.policy(),
        certs.clone(),
        config.rewrite_certificate_report,
        config.block_telemetry,
        autoplay,
    )?;

    info!("Starting proxy on {addr}");

    let proxy = Proxy::builder()
        .with_addr(addr)
        .with_ca(ca)
        .with_http_connector(upstream::http_connector(certs.clone()))
        .with_http_handler(handler.clone())
        .with_websocket_handler(handler)
        .with_websocket_connector(upstream::websocket_connector(certs.clone()))
        .with_graceful_shutdown(shutdown)
        .build()
        .context("Failed to build proxy")?;

    proxy.start().await.context("Proxy stopped with error")
}
