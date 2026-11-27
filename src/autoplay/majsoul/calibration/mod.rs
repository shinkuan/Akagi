//! Majsoul mouse calibration: the user's own games recorded and fitted into
//! `MouseProfile`, plus the per-kyoku counters the cursor steers by.
//!
//! See `README.md` in this directory.

pub mod fit;
pub mod recorder;
pub mod report;
pub mod service;
pub mod session;

pub use recorder::InputBatch;
pub use report::LogReportSample;
pub use session::{CalibrationStatus, LiveRate, Session};

use crate::config::{AppConfig, CaptureMode, MouseProfile, Platform};
use std::sync::{Mutex, PoisonError};
use tokio::sync::{broadcast, RwLock};

/// Shared by the capture backend (page input), the bridge (reports), the
/// service, the cursor, the manager and IPC.
pub struct MouseFeed {
    pub input: broadcast::Sender<InputBatch>,
    pub reports: broadcast::Sender<LogReportSample>,
    /// Status for the UI (`mouse-calibration` event).
    pub status: broadcast::Sender<CalibrationStatus>,
    session: Mutex<Session>,
}

impl Default for MouseFeed {
    fn default() -> Self {
        Self {
            input: broadcast::channel(256).0,
            reports: broadcast::channel(64).0,
            status: broadcast::channel(16).0,
            session: Mutex::new(Session::default()),
        }
    }
}

impl MouseFeed {
    pub fn with<R>(&self, f: impl FnOnce(&mut Session) -> R) -> R {
        f(&mut self.session.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn calibrating(&self) -> bool {
        self.with(|s| s.active())
    }

    pub fn publish(&self) -> CalibrationStatus {
        let status = self.with(|s| s.status());
        let _ = self.status.send(status.clone());
        status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    NotCalibrated,
    /// The user is the one playing.
    Calibrating,
}

/// Whether Majsoul autoplay may act.
pub fn majsoul_gate(profile: &MouseProfile, calibrating: bool) -> Result<(), Blocked> {
    if calibrating {
        Err(Blocked::Calibrating)
    } else if !profile.calibrated {
        Err(Blocked::NotCalibrated)
    } else {
        Ok(())
    }
}

pub async fn start(cfg: &RwLock<AppConfig>, feed: &MouseFeed) -> Result<CalibrationStatus, String> {
    {
        let cfg = cfg.read().await;
        if cfg.platform.kind != Platform::Majsoul {
            return Err("mouse calibration is for Mahjong Soul only".into());
        }
        if cfg.capture.mode != CaptureMode::Chromium {
            return Err("mouse calibration needs the Chromium capture mode".into());
        }
    }
    feed.with(|s| {
        s.start(chrono::Local::now());
        s.save()
    })
    .map_err(|e| format!("save calibration state: {e}"))?;
    Ok(feed.publish())
}

/// Drop the session and its recorded input.
pub fn cancel(feed: &MouseFeed) -> Result<CalibrationStatus, String> {
    feed.with(|s| {
        s.cancel();
        s.save()
    })
    .map_err(|e| format!("save calibration state: {e}"))?;
    Ok(feed.publish())
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FinishedCalibration {
    pub config: AppConfig,
    /// Profile fields left as they were for want of samples.
    pub kept: Vec<String>,
}

/// Fit the recorded games into the profile, save the config with `persist`,
/// and end the session.
pub async fn finish(
    cfg: &RwLock<AppConfig>,
    feed: &MouseFeed,
    persist: impl FnOnce(&AppConfig) -> Result<(), String>,
) -> Result<FinishedCalibration, String> {
    let (raw_files, games) = feed.with(|s| {
        if !s.active() {
            return Err("no calibration in progress".to_string());
        }
        if s.games() < session::REQUIRED_GAMES {
            return Err(format!(
                "{} of {} games recorded",
                s.games(),
                session::REQUIRED_GAMES
            ));
        }
        Ok((s.raw_files(), s.games()))
    })?;
    let raw = tokio::task::spawn_blocking(move || {
        raw_files
            .iter()
            .flat_map(|p| session::read_raw(p))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("read recorded input: {e}"))?;
    let finished = {
        let mut cfg = cfg.write().await;
        let majsoul = &mut cfg.autoplay.majsoul;
        let fit = fit::fit(&raw, &majsoul.mouse, games, chrono::Local::now());
        majsoul.mouse = fit.profile;
        if let Some(hover) = fit.hover_ms {
            majsoul.hover_delay_ms = hover;
        }
        if let Some(hold) = fit.hold_ms {
            majsoul.click_hold_ms = hold;
        }
        persist(&cfg)?;
        FinishedCalibration {
            config: cfg.clone(),
            kept: fit.kept.into_iter().map(str::to_string).collect(),
        }
    };
    feed.with(|s| {
        s.finish();
        s.save()
    })
    .map_err(|e| format!("save calibration state: {e}"))?;
    feed.publish();
    Ok(finished)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_calibrated_profile_outside_a_session_passes() {
        let mut p = MouseProfile::default();
        assert_eq!(majsoul_gate(&p, false), Err(Blocked::NotCalibrated));
        p.calibrated = true;
        assert_eq!(majsoul_gate(&p, false), Ok(()));
        assert_eq!(majsoul_gate(&p, true), Err(Blocked::Calibrating));
    }
}
