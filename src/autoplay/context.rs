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
    /// The centred 16:9 game area within the (possibly letterboxed) canvas, as
    /// `(x, y, width, height)` in CSS pixels.
    ///
    /// Majsoul always renders the game at 16:9 and pads with black bars when
    /// the `<canvas>` element's aspect ratio differs (i.e. whenever the window
    /// isn't 16:9). Normalised `LOCATION` coordinates therefore must be mapped
    /// against this sub-rect, NOT the raw element rect — otherwise every click
    /// (and the vision fingerprint) drifts off target as the window is resized.
    /// When the canvas is already 16:9 this returns the full rect unchanged.
    pub fn game_area(&self) -> (f64, f64, f64, f64) {
        const TARGET: f64 = 16.0 / 9.0;
        if self.width <= 0.0 || self.height <= 0.0 {
            return (self.x, self.y, self.width, self.height);
        }
        let aspect = self.width / self.height;
        if aspect > TARGET {
            // Wider than 16:9 → pillarbox (left/right bars).
            let gw = self.height * TARGET;
            let gx = self.x + (self.width - gw) / 2.0;
            (gx, self.y, gw, self.height)
        } else {
            // Taller than 16:9 → letterbox (top/bottom bars).
            let gh = self.width / TARGET;
            let gy = self.y + (self.height - gh) / 2.0;
            (self.x, gy, self.width, gh)
        }
    }

    /// Translate a 16:9 normalised point (the coordinate system used by
    /// `LOCATION` tables ported from the Python reference) to CSS pixels,
    /// mapping against the centred 16:9 [`game_area`](Self::game_area) so it
    /// stays correct at any window aspect ratio.
    pub fn pixel(&self, x_norm: f64, y_norm: f64) -> (f64, f64) {
        let (gx, gy, gw, gh) = self.game_area();
        (gx + (x_norm / 16.0) * gw, gy + (y_norm / 9.0) * gh)
    }

    /// Sanity check for a pixel point — rejects off-canvas requests before we
    /// hand them to CDP. Checks the full element rect (the 16:9 game area is a
    /// subset of it, so any valid mapped point passes).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_translation_centre() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert_eq!(rect.pixel(8.0, 4.5), (800.0, 450.0));
    }

    #[test]
    fn pixel_translation_with_offset() {
        let rect = CanvasRect {
            x: 100.0,
            y: 50.0,
            width: 1280.0,
            height: 720.0,
        };
        let (px, py) = rect.pixel(8.0, 4.5);
        assert!((px - (100.0 + 640.0)).abs() < 1e-9);
        assert!((py - (50.0 + 360.0)).abs() < 1e-9);
    }

    #[test]
    fn pixel_pillarbox_wider_than_16_9() {
        // 1920×900 canvas (aspect 2.13 > 16:9) → 1600-wide game area centred:
        // left bar = (1920-1600)/2 = 160. Centre (8, 4.5) → (160+800, 450).
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 900.0,
        };
        let (px, py) = rect.pixel(8.0, 4.5);
        assert!((px - 960.0).abs() < 1e-6, "px={px}");
        assert!((py - 450.0).abs() < 1e-6, "py={py}");
        // Top-left of the 16:9 area sits at the left bar edge.
        let (x0, _) = rect.pixel(0.0, 0.0);
        assert!((x0 - 160.0).abs() < 1e-6, "x0={x0}");
    }

    #[test]
    fn pixel_letterbox_taller_than_16_9() {
        // 1600×1000 canvas (aspect 1.6 < 16:9) → 900-tall game area centred:
        // top bar = (1000-900)/2 = 50. Centre (8, 4.5) → (800, 50+450).
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 1000.0,
        };
        let (px, py) = rect.pixel(8.0, 4.5);
        assert!((px - 800.0).abs() < 1e-6, "px={px}");
        assert!((py - 500.0).abs() < 1e-6, "py={py}");
    }

    #[test]
    fn game_area_is_identity_for_16_9() {
        let rect = CanvasRect {
            x: 10.0,
            y: 20.0,
            width: 1600.0,
            height: 900.0,
        };
        let (gx, gy, gw, gh) = rect.game_area();
        assert!((gx - 10.0).abs() < 1e-6);
        assert!((gy - 20.0).abs() < 1e-6);
        assert!((gw - 1600.0).abs() < 1e-6);
        assert!((gh - 900.0).abs() < 1e-6);
    }

    #[test]
    fn contains_inside() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert!(rect.contains(800.0, 450.0));
    }

    #[test]
    fn contains_outside() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        assert!(!rect.contains(-1.0, 0.0));
        assert!(!rect.contains(0.0, 1000.0));
    }
}
