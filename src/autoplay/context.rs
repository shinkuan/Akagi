//! Shared state between the chromium capture backend and the autoplay
//! manager.
//!
//! - `page`: the [`chromiumoxide::page::Page`] handle for the tab where
//!   Majsoul (or another supported platform) is loaded. Written by
//!   `src/capture/chromium/cdp.rs` when it observes a WebSocket whose URL
//!   host matches a known platform; cleared when that WS closes. Read by
//!   `AutoplayManager` whenever it needs to dispatch input.
//! - `canvas_rect`: cached `getBoundingClientRect()` of the game canvas,
//!   used to translate 16:9-normalised coordinates into CSS pixels.
//!   Filled lazily by the autoplay manager (one `Runtime.evaluate` per
//!   refresh) and invalidated on round transitions.
//!
//! Both fields are populated only when the chromium capture backend is
//! active. The MITM backend leaves the context untouched, so reads return
//! `None` and the manager skips the click.

use chromiumoxide::page::Page;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct AutoplayContext {
    pub page: Arc<RwLock<Option<Page>>>,
    pub canvas_rect: Arc<RwLock<Option<CanvasRect>>>,
}

impl AutoplayContext {
    pub fn new() -> Self {
        Self::default()
    }
}

/// CSS-pixel bounding rect for the game canvas, as reported by
/// `Element.getBoundingClientRect()`. `(x, y)` is the top-left of the
/// canvas relative to the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl CanvasRect {
    /// Translate a 16:9 normalised point (the coordinate system used by
    /// `LOCATION` tables ported from the Python reference) to CSS pixels.
    pub fn pixel(&self, x_norm: f64, y_norm: f64) -> (f64, f64) {
        (
            self.x + (x_norm / 16.0) * self.width,
            self.y + (y_norm / 9.0) * self.height,
        )
    }

    /// Sanity check for a normalised point — clamps off-canvas requests
    /// before we hand them to CDP.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x
            && x <= self.x + self.width
            && y >= self.y
            && y <= self.y + self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_translation_centre() {
        let rect = CanvasRect { x: 0.0, y: 0.0, width: 1600.0, height: 900.0 };
        assert_eq!(rect.pixel(8.0, 4.5), (800.0, 450.0));
    }

    #[test]
    fn pixel_translation_with_offset() {
        let rect = CanvasRect { x: 100.0, y: 50.0, width: 1280.0, height: 720.0 };
        let (px, py) = rect.pixel(8.0, 4.5);
        assert!((px - (100.0 + 640.0)).abs() < 1e-9);
        assert!((py - (50.0 + 360.0)).abs() < 1e-9);
    }

    #[test]
    fn contains_inside() {
        let rect = CanvasRect { x: 0.0, y: 0.0, width: 1600.0, height: 900.0 };
        assert!(rect.contains(800.0, 450.0));
    }

    #[test]
    fn contains_outside() {
        let rect = CanvasRect { x: 0.0, y: 0.0, width: 1600.0, height: 900.0 };
        assert!(!rect.contains(-1.0, 0.0));
        assert!(!rect.contains(0.0, 1000.0));
    }
}
