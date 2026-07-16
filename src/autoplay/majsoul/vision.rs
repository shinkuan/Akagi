//! Minimal visual detection for the auto-start return-to-lobby transition.
//!
//! The post-game result screens vs the lobby home are invisible to BOTH the
//! WS protocol and Majsoul's in-page JS state (the WebGL canvas exposes no
//! DOM/scene to read). A screenshot CAN tell them apart. Rather than classify
//! the whole screen, we match one small region around a static graphic that
//! appears only on the lobby home (the Ranked label) against a reference
//! captured on the lobby home.
//!
//! Matching = normalized cross-correlation (NCC) of a downscaled grayscale
//! TEMPLATE of the region. NCC compares the actual pattern (not just average
//! brightness) and is invariant to brightness/contrast, so it separates "label
//! present" from "label absent" far better than a coarse average -- a plain
//! 8x8 brightness fingerprint throws the glyph shape away and the two classes
//! overlap. If NCC still can't separate them, the next step is a small learned
//! classifier (the repo already links `candle` for the native bot).

use crate::autoplay::context::CanvasRect;
use anyhow::{anyhow, Context, Result};
use chromiumoxide::cdp::browser_protocol::page::{CaptureScreenshotFormat, Viewport};
use chromiumoxide::page::{Page, ScreenshotParams};

/// Template grid the region is downscaled to. Deliberately COARSE (~2.9:1 to
/// keep the wide Ranked label shape): each cell block-averages a large patch, so
/// its value is the ink-density of that block, which is stable across window
/// scale/resolution. A fine grid (e.g. 64x22) instead samples individual
/// calligraphy strokes, whose per-cell alignment shifts wildly with scale --
/// that made NCC collapse (1.0 -> 0.2) after a resize even though the region was
/// correct. Coarse block-averages fix that while still separating the label
/// from any other screen (whose block structure is entirely different).
pub const TPL_W: usize = 24;
pub const TPL_H: usize = 8;

/// Default NCC match threshold (1.0 = identical pattern). With the coarse grid
/// the true lobby stays high across resizes while other screens sit far lower;
/// the check command reports the raw NCC so this can be tuned per client.
pub const DEFAULT_HOME_NCC: f32 = 0.65;

/// Screenshot a rectangular region -- centred on the 16:9-normalised point
/// `(x_norm, y_norm)`, sized `w_norm` x `h_norm` -- and reduce it to a
/// `TPL_W` x `TPL_H` grayscale template (row-major, 0..=255). Pick a box that
/// bounds a STATIC graphic (the Ranked label) and excludes animated elements
/// (the player character on the left).
pub async fn region_fingerprint(
    page: &Page,
    rect: &CanvasRect,
    x_norm: f64,
    y_norm: f64,
    w_norm: f64,
    h_norm: f64,
) -> Result<Vec<u8>> {
    // The canvas element rect IS the 16:9 game area (Majsoul letterboxes
    // outside the canvas), so sizing against it keeps the captured region the
    // same physical patch at any window aspect ratio.
    let (cx, cy) = rect.pixel(x_norm, y_norm);
    let half_w = (w_norm / 16.0) * rect.width / 2.0;
    let half_h = (h_norm / 9.0) * rect.height / 2.0;
    let clip = Viewport {
        x: (cx - half_w).max(0.0),
        y: (cy - half_h).max(0.0),
        width: (half_w * 2.0).max(1.0),
        height: (half_h * 2.0).max(1.0),
        scale: 1.0,
    };
    let params = ScreenshotParams::builder()
        .format(CaptureScreenshotFormat::Png)
        .clip(clip)
        .build();
    let png_bytes = page
        .screenshot(params)
        .await
        .context("CDP screenshot (region)")?;
    template_from_png(&png_bytes)
}

/// Decode an 8-bit PNG and block-average it into a `TPL_W` x `TPL_H` grayscale
/// template.
pub fn template_from_png(png_bytes: &[u8]) -> Result<Vec<u8>> {
    let decoder = png::Decoder::new(png_bytes);
    let mut reader = decoder.read_info().context("png read_info")?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).context("png next_frame")?;
    let (w, h) = (info.width as usize, info.height as usize);
    if w == 0 || h == 0 {
        return Err(anyhow!("empty screenshot region"));
    }
    if info.bit_depth != png::BitDepth::Eight {
        return Err(anyhow!("unexpected png bit depth {:?}", info.bit_depth));
    }
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        other => return Err(anyhow!("unsupported png color type {other:?}")),
    };
    let gray_at = |x: usize, y: usize| -> u32 {
        let idx = (y * w + x) * channels;
        if channels <= 2 {
            buf[idx] as u32
        } else {
            let r = buf[idx] as u32;
            let g = buf[idx + 1] as u32;
            let b = buf[idx + 2] as u32;
            (r * 299 + g * 587 + b * 114) / 1000 // Rec.601 luma
        }
    };
    let mut tpl = vec![0u8; TPL_W * TPL_H];
    for ty in 0..TPL_H {
        let y0 = ty * h / TPL_H;
        let y1 = (((ty + 1) * h / TPL_H).max(y0 + 1)).min(h);
        for tx in 0..TPL_W {
            let x0 = tx * w / TPL_W;
            let x1 = (((tx + 1) * w / TPL_W).max(x0 + 1)).min(w);
            let mut sum = 0u32;
            let mut count = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += gray_at(x, y);
                    count += 1;
                }
            }
            tpl[ty * TPL_W + tx] = sum.checked_div(count).unwrap_or(0) as u8;
        }
    }
    Ok(tpl)
}

/// Normalized cross-correlation between two equal-length grayscale templates.
/// Returns 1.0 for an identical pattern, ~0 for uncorrelated, down to -1.0 for
/// inverted. Invariant to brightness/contrast (mean-subtracted, variance-
/// normalized). `-1.0` on a length mismatch so a stale reference can't match;
/// `0.0` if either side is flat (no variance to correlate).
pub fn ncc_similarity(a: &[u8], b: &[u8]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return -1.0;
    }
    let n = a.len() as f64;
    let ma = a.iter().map(|&v| v as f64).sum::<f64>() / n;
    let mb = b.iter().map(|&v| v as f64).sum::<f64>() / n;
    let (mut num, mut da, mut db) = (0.0f64, 0.0f64, 0.0f64);
    for (&av, &bv) in a.iter().zip(b) {
        let x = av as f64 - ma;
        let y = bv as f64 - mb;
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    (num / (da.sqrt() * db.sqrt())) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_templates_correlate_to_one() {
        let a: Vec<u8> = (0..TPL_W * TPL_H).map(|i| (i % 256) as u8).collect();
        assert!((ncc_similarity(&a, &a) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn length_mismatch_is_minus_one() {
        assert_eq!(ncc_similarity(&[1, 2, 3], &[1, 2]), -1.0);
        assert_eq!(ncc_similarity(&[], &[]), -1.0);
    }

    #[test]
    fn brightness_shift_is_invariant() {
        // A uniform brightness/contrast change must not lower NCC.
        let a: Vec<u8> = (0..64).map(|i| (i * 3 % 200) as u8).collect();
        let b: Vec<u8> = a.iter().map(|&v| (v / 2).saturating_add(40)).collect();
        assert!(ncc_similarity(&a, &b) > 0.99);
    }

    #[test]
    fn flat_region_is_zero() {
        let a = vec![10u8; 64];
        let b: Vec<u8> = (0..64).map(|i| i as u8).collect();
        assert_eq!(ncc_similarity(&a, &b), 0.0);
    }
}
