//! Typed errors for the in-app update flow. Frontend pattern-matches on
//! the discriminant so it can choose between "show a generic toast" and
//! "fall back to opening the release page in a browser".

use serde::Serialize;
use std::path::PathBuf;

/// Surfaced to the frontend by `apply_update`. `check_for_update`
/// returns `Result<Option<UpdateInfo>, String>` instead — failures
/// there just bubble up as a plain anyhow chain because the frontend
/// has no per-case fallback for a check.
#[derive(Debug, thiserror::Error, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateError {
    /// Running on a target triple Akagi doesn't publish a binary for
    /// (e.g. linux/aarch64 today). The UI should suppress the toast
    /// and only surface a "check releases manually" link in Settings.
    #[error("running platform has no published release artifact")]
    UnsupportedPlatform,

    /// Install directory isn't writable — AppImage, system-wide install,
    /// macOS `.app` bundle without admin. UI falls back to "Open release
    /// page".
    #[error("install directory {path} is not writable")]
    ReadOnlyInstall { path: PathBuf },

    /// Asset has a `digest` field but the downloaded bytes don't match.
    /// Could be CDN corruption or MITM; we refuse to swap.
    #[error("downloaded asset SHA-256 digest does not match release metadata")]
    DigestMismatch,

    /// The matching asset for the running triple isn't in the release.
    /// Either the workflow didn't upload it yet, or the version pre-dates
    /// the current platform support.
    #[error("no release asset matches the running platform")]
    NoMatchingAsset,

    /// Catch-all for anyhow-wrapped reqwest/IO/zip errors. The string
    /// is already formatted (`{e:#}` style).
    #[error("{message}")]
    Other { message: String },
}

impl From<anyhow::Error> for UpdateError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other {
            message: format!("{e:#}"),
        }
    }
}
