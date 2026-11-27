//! Majsoul mouse habits: recording the user's own games (calibration), and
//! the live measurements the autoplay cursor steers by.
//!
//! See `README.md` in this directory.

pub mod fit;
pub mod gate;
pub mod recorder;
pub mod report;
pub mod service;
pub mod session;

pub use gate::{majsoul_gate, Blocked};
pub use recorder::InputBatch;
pub use report::LogReportSample;
pub use session::{CalibrationStatus, Session};

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, watch};

pub type SharedSession = Arc<Mutex<Session>>;

/// The kyoku in progress as the cursor controller sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LiveRate {
    /// Whether a kyoku is being played (between its start and its end).
    pub in_kyoku: bool,
    /// Epoch ms the kyoku started.
    pub started_ms: f64,
    pub page_frames: u32,
    pub page_moving: u32,
    /// Client frames per page frame, smoothed over recent kyoku.
    pub frame_ratio: Option<f64>,
}

const CLICK_MODE_UNKNOWN: u8 = u8::MAX;

/// Slots shared by the capture backend, the bridge, the service and the
/// cursor.
pub struct MouseFeed {
    /// Page input batches, from the chromium backend.
    pub input: broadcast::Sender<InputBatch>,
    /// `logReport`s, from the Majsoul bridge.
    pub reports: broadcast::Sender<LogReportSample>,
    pub live: watch::Sender<LiveRate>,
    calibrating: AtomicBool,
    click_mode: AtomicU8,
}

impl Default for MouseFeed {
    fn default() -> Self {
        Self {
            input: broadcast::channel(256).0,
            reports: broadcast::channel(64).0,
            live: watch::channel(LiveRate::default()).0,
            calibrating: AtomicBool::new(false),
            click_mode: AtomicU8::new(CLICK_MODE_UNKNOWN),
        }
    }
}

impl MouseFeed {
    pub fn calibrating(&self) -> bool {
        self.calibrating.load(Ordering::SeqCst)
    }

    pub fn set_calibrating(&self, on: bool) {
        self.calibrating.store(on, Ordering::SeqCst);
    }

    /// The click mode the client last reported (`m`), if any kyoku has ended.
    pub fn reported_click_mode(&self) -> Option<crate::config::ClickMode> {
        match self.click_mode.load(Ordering::SeqCst) {
            0 => Some(crate::config::ClickMode::Single),
            1 => Some(crate::config::ClickMode::Double),
            _ => None,
        }
    }

    pub fn set_reported_click_mode(&self, m: u32) {
        let v = match m {
            0 => 0,
            1 => 1,
            _ => CLICK_MODE_UNKNOWN,
        };
        self.click_mode.store(v, Ordering::SeqCst);
    }
}
