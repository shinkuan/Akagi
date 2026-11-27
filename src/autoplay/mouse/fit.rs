//! Turn a calibration session into a [`MouseProfile`].
//!
//! A field with too few samples keeps the value of `base` (the profile in
//! force, or the defaults) and is named in [`Fit::kept`].

use super::session::{quantile, KyokuRecord, RawKyoku};
use crate::config::{ClickMode, MouseProfile, MoveRate, MsRange};

/// z of the 10th / 90th percentile of a standard normal.
const Z90: f64 = 1.281_551_6;
/// Gap between moving frames that ends a stroke, in frame intervals.
const STROKE_GAP_FRAMES: f64 = 2.5;
/// The client starts a drag past 20 px of travel with the button down.
const DRAG_PX: f64 = 20.0;
const DOUBLE_PX: f64 = 15.0;
const DOUBLE_MAX_MS: f64 = 1500.0;
const APPROACH_LOOKBACK_MS: f64 = 1500.0;
const MIN_STROKES: usize = 20;
const MIN_PRESSES: usize = 8;
const MIN_DOUBLES: usize = 5;

#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    pub profile: MouseProfile,
    pub hover_ms: Option<MsRange>,
    pub hold_ms: Option<MsRange>,
    /// Fields left at `base` for want of samples.
    pub kept: Vec<&'static str>,
}

/// One continuous movement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub start_ms: f64,
    pub end_ms: f64,
    /// Path length in canvas widths.
    pub length: f64,
}

impl Stroke {
    pub fn duration_ms(&self) -> f64 {
        self.end_ms - self.start_ms
    }
}

pub fn fit(
    kyoku: &[KyokuRecord],
    raw: &[RawKyoku],
    base: &MouseProfile,
    games: f64,
    now: chrono::DateTime<chrono::Local>,
) -> Fit {
    let mut kept = Vec::new();
    let mut p = base.clone();
    p.calibrated = true;
    p.calibrated_at = now.to_rfc3339();
    p.sample_games = games;
    p.sample_kyoku = kyoku.len() as u32;

    match move_rate(kyoku) {
        Some(r) => p.move_rate = r,
        None => kept.push("move_rate"),
    }
    let ratios: Vec<f64> = kyoku
        .iter()
        .filter(|k| k.page_moving >= 50)
        .map(|k| f64::from(k.d) / f64::from(k.page_moving))
        .collect();
    match quantile(&ratios, 0.5) {
        Some(r) => p.frame_ratio = r.clamp(0.1, 2.0),
        None => kept.push("frame_ratio"),
    }
    let share =
        |moving: u32, frames: u32| (frames > 0).then(|| f64::from(moving) / f64::from(frames));
    let own: (u32, u32) = kyoku
        .iter()
        .fold((0, 0), |a, k| (a.0 + k.own_moving, a.1 + k.own_frames));
    let other: (u32, u32) = kyoku
        .iter()
        .fold((0, 0), |a, k| (a.0 + k.other_moving, a.1 + k.other_frames));
    match share(own.0, own.1) {
        Some(s) => p.own_active = s.clamp(0.02, 0.98),
        None => kept.push("own_active"),
    }
    match share(other.0, other.1) {
        Some(s) => p.other_active = s.clamp(0.02, 0.98),
        None => kept.push("other_active"),
    }

    let mut speeds = Vec::new();
    let mut approaches = Vec::new();
    let mut hovers = Vec::new();
    let mut holds = Vec::new();
    let mut doubles = Vec::new();
    for k in raw {
        let strokes = strokes(k);
        speeds.extend(
            strokes
                .iter()
                .filter(|s| s.duration_ms() > 0.0 && s.length > 0.005)
                .map(|s| s.length / (s.duration_ms() / 1000.0)),
        );
        for (down, up) in presses(k) {
            if let Some(s) = strokes
                .iter()
                .rev()
                .find(|s| s.end_ms <= down[0] && s.end_ms >= down[0] - APPROACH_LOOKBACK_MS)
            {
                approaches.push(s.duration_ms());
                hovers.push(down[0] - s.end_ms);
            }
            if let Some(up) = up {
                if px_dist(k.rect, down, up) <= DRAG_PX {
                    holds.push(up[0] - down[0]);
                }
            }
        }
        doubles.extend(double_gaps(k));
    }

    match iqr(&speeds, MIN_STROKES) {
        Some((a, b)) => p.speed = [a.clamp(0.05, 10.0), b.clamp(0.05, 10.0)],
        None => kept.push("speed"),
    }
    match iqr(&approaches, MIN_PRESSES) {
        Some((a, b)) => p.approach_ms = ms_range(a, b, 60, 2000),
        None => kept.push("approach_ms"),
    }
    let hover_ms = iqr(&hovers, MIN_PRESSES).map(|(a, b)| ms_range(a, b, 100, 1500));
    let hold_ms = iqr(&holds, MIN_PRESSES).map(|(a, b)| ms_range(a, b, 30, 600));
    if hover_ms.is_none() {
        kept.push("hover_delay_ms");
    }
    if hold_ms.is_none() {
        kept.push("click_hold_ms");
    }

    let doubles_seen = kyoku.iter().filter(|k| k.m == 1).count();
    p.click_mode = if doubles_seen * 2 > kyoku.len() {
        ClickMode::Double
    } else {
        ClickMode::Single
    };
    if p.click_mode == ClickMode::Double {
        match iqr(&doubles, MIN_DOUBLES) {
            Some((a, b)) => p.double_click_ms = ms_range(a, b, 60, 1500),
            None => {
                let t: Vec<f64> = kyoku
                    .iter()
                    .filter(|k| k.t_ms > 0)
                    .map(|k| f64::from(k.t_ms))
                    .collect();
                let g: Vec<f64> = kyoku
                    .iter()
                    .filter(|k| k.g_ms > 0)
                    .map(|k| f64::from(k.g_ms))
                    .collect();
                match (quantile(&t, 0.5), quantile(&g, 0.5)) {
                    (Some(a), Some(b)) => p.double_click_ms = ms_range(a, b, 60, 1500),
                    _ => kept.push("double_click_ms"),
                }
            }
        }
    }

    Fit {
        profile: p,
        hover_ms,
        hold_ms,
        kept,
    }
}

/// Log-normal fit of the per-kyoku moving-frame rate.
fn move_rate(kyoku: &[KyokuRecord]) -> Option<MoveRate> {
    let logs: Vec<f64> = kyoku
        .iter()
        .filter(|k| k.duration_s > 0.0)
        .map(|k| (f64::from(k.d.max(1)) / k.duration_s).ln())
        .collect();
    if logs.len() < 3 {
        return None;
    }
    let n = logs.len() as f64;
    let mu = logs.iter().sum::<f64>() / n;
    let sigma = (logs.iter().map(|l| (l - mu).powi(2)).sum::<f64>() / (n - 1.0))
        .sqrt()
        .max(0.05);
    Some(MoveRate {
        median: mu.exp(),
        low: (mu - Z90 * sigma).exp(),
        high: (mu + Z90 * sigma).exp(),
    })
}

/// Continuous movements in one kyoku's raw input.
pub fn strokes(k: &RawKyoku) -> Vec<Stroke> {
    let frame = frame_interval(&k.frames).unwrap_or(1000.0 / 60.0);
    let aspect = if k.rect.0 > 0.0 {
        k.rect.1 / k.rect.0
    } else {
        9.0 / 16.0
    };
    let mut out = Vec::new();
    let mut cur: Option<(Stroke, [f64; 3])> = None;
    for m in &k.moves {
        cur = Some(match cur {
            Some((mut s, prev)) if m[0] - prev[0] <= STROKE_GAP_FRAMES * frame => {
                s.end_ms = m[0];
                s.length += ((m[1] - prev[1]).powi(2) + ((m[2] - prev[2]) * aspect).powi(2)).sqrt();
                (s, *m)
            }
            other => {
                if let Some((s, _)) = other {
                    out.push(s);
                }
                // The first moving frame already moved: one frame of travel.
                (
                    Stroke {
                        start_ms: m[0] - frame,
                        end_ms: m[0],
                        length: 0.0,
                    },
                    *m,
                )
            }
        });
    }
    if let Some((s, _)) = cur {
        out.push(s);
    }
    out
}

fn frame_interval(frames: &[f64]) -> Option<f64> {
    let gaps: Vec<f64> = frames
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|g| *g > 0.0)
        .collect();
    quantile(&gaps, 0.5)
}

/// Left-button presses with their release, if one followed.
fn presses(k: &RawKyoku) -> Vec<([f64; 5], Option<[f64; 5]>)> {
    let left: Vec<[f64; 5]> = k.buttons.iter().copied().filter(|b| b[2] == 0.0).collect();
    left.iter()
        .enumerate()
        .filter(|(_, b)| b[1] == 1.0)
        .map(|(i, &down)| {
            let up = left[i + 1..]
                .iter()
                .find(|b| b[1] == 0.0 && b[0] - down[0] <= 2000.0)
                .copied();
            (down, up)
        })
        .collect()
}

/// Gaps between two left presses on the same spot.
fn double_gaps(k: &RawKyoku) -> Vec<f64> {
    let downs: Vec<[f64; 5]> = k
        .buttons
        .iter()
        .copied()
        .filter(|b| b[2] == 0.0 && b[1] == 1.0)
        .collect();
    downs
        .windows(2)
        .filter(|w| w[1][0] - w[0][0] <= DOUBLE_MAX_MS && px_dist(k.rect, w[0], w[1]) <= DOUBLE_PX)
        .map(|w| w[1][0] - w[0][0])
        .collect()
}

fn px_dist(rect: (f64, f64), a: [f64; 5], b: [f64; 5]) -> f64 {
    ((a[3] - b[3]) * rect.0).hypot((a[4] - b[4]) * rect.1)
}

fn iqr(values: &[f64], min: usize) -> Option<(f64, f64)> {
    if values.len() < min {
        return None;
    }
    Some((quantile(values, 0.25)?, quantile(values, 0.75)?))
}

fn ms_range(a: f64, b: f64, lo: u32, hi: u32) -> MsRange {
    let c = |v: f64| (v.round().max(0.0) as u32).clamp(lo, hi);
    MsRange::new(c(a), c(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local::now()
    }

    fn kyoku(d: u32, duration_s: f64) -> KyokuRecord {
        KyokuRecord {
            duration_s,
            d,
            page_frames: 6000,
            page_moving: d * 2,
            own_frames: 2000,
            own_moving: 1000,
            other_frames: 4000,
            other_moving: 800,
            ..KyokuRecord::default()
        }
    }

    /// A raw kyoku of `n` presses: each a 300 ms stroke across 0.3 of the
    /// canvas, a 150 ms rest, a 90 ms press, then 1 s of nothing.
    fn raw_presses(n: usize) -> RawKyoku {
        let mut k = RawKyoku {
            rect: (1600.0, 900.0),
            ..RawKyoku::default()
        };
        let mut t = 0.0;
        for _ in 0..n {
            for i in 0..=18 {
                let ft = t + f64::from(i) * 16.0;
                k.frames.push(ft);
                if i > 0 {
                    k.moves.push([ft, 0.2 + 0.3 * f64::from(i) / 18.0, 0.8]);
                }
            }
            let end = t + 18.0 * 16.0;
            k.buttons.push([end + 150.0, 1.0, 0.0, 0.5, 0.8]);
            k.buttons.push([end + 240.0, 0.0, 0.0, 0.5, 0.8]);
            t = end + 1240.0;
        }
        k
    }

    #[test]
    fn move_rate_is_log_normal_over_kyoku() {
        let ks: Vec<KyokuRecord> = [10.0, 20.0, 40.0]
            .iter()
            .map(|r| kyoku((r * 100.0) as u32, 100.0))
            .collect();
        let f = fit(&ks, &[], &MouseProfile::default(), 2.0, now());
        let r = f.profile.move_rate;
        assert!((r.median - 20.0).abs() < 1e-6, "geometric mean");
        assert!(r.low < 10.0 && r.high > 40.0);
        assert!((f.profile.frame_ratio - 0.5).abs() < 1e-9);
        assert!((f.profile.own_active - 0.5).abs() < 1e-9);
        assert!((f.profile.other_active - 0.2).abs() < 1e-9);
        assert!(f.profile.calibrated);
        assert_eq!(f.profile.sample_kyoku, 3);
    }

    #[test]
    fn press_timing_comes_from_the_raw_input() {
        let raw = vec![raw_presses(24)];
        let f = fit(&[], &raw, &MouseProfile::default(), 2.0, now());
        let a = f.profile.approach_ms;
        assert!((a.min as f64 - 288.0).abs() <= 1.0 && (a.max as f64 - 288.0).abs() <= 1.0);
        assert_eq!(f.hover_ms, Some(MsRange::fixed(150)));
        assert_eq!(f.hold_ms, Some(MsRange::fixed(90)));
        // 17 of the 18 steps are seen (the first frame's origin is not), in 288 ms.
        let speed = 0.3 * 17.0 / 18.0 / 0.288;
        assert!((f.profile.speed[0] - speed).abs() < 0.01);
        assert!(!f.kept.contains(&"speed"));
    }

    #[test]
    fn too_few_samples_keep_the_base() {
        let base = MouseProfile::default();
        let f = fit(&[kyoku(100, 60.0)], &[raw_presses(2)], &base, 2.0, now());
        assert_eq!(f.profile.move_rate, base.move_rate);
        assert_eq!(f.profile.speed, base.speed);
        assert_eq!(f.profile.approach_ms, base.approach_ms);
        assert!(f.hover_ms.is_none() && f.hold_ms.is_none());
        for field in [
            "move_rate",
            "speed",
            "approach_ms",
            "hover_delay_ms",
            "click_hold_ms",
        ] {
            assert!(f.kept.contains(&field), "{field}");
        }
    }

    #[test]
    fn double_click_users_get_their_gap() {
        let mut ks: Vec<KyokuRecord> = (0..4).map(|_| kyoku(1000, 100.0)).collect();
        for k in &mut ks {
            k.m = 1;
            k.t_ms = 150;
            k.g_ms = 300;
        }
        let mut raw = RawKyoku {
            rect: (1600.0, 900.0),
            ..RawKyoku::default()
        };
        for i in 0..6 {
            let t = f64::from(i) * 3000.0;
            raw.buttons.push([t, 1.0, 0.0, 0.5, 0.8]);
            raw.buttons.push([t + 60.0, 0.0, 0.0, 0.5, 0.8]);
            raw.buttons.push([t + 200.0, 1.0, 0.0, 0.5005, 0.8]);
            raw.buttons.push([t + 260.0, 0.0, 0.0, 0.5005, 0.8]);
        }
        let f = fit(&ks, &[raw], &MouseProfile::default(), 2.0, now());
        assert_eq!(f.profile.click_mode, ClickMode::Double);
        assert_eq!(f.profile.double_click_ms, MsRange::fixed(200));
        // Without raw pairs the client's own min/max stand in.
        let f = fit(&ks, &[], &MouseProfile::default(), 2.0, now());
        assert_eq!(f.profile.double_click_ms, MsRange::new(150, 300));
    }

    #[test]
    fn a_drag_is_not_a_hold() {
        let mut raw = raw_presses(10);
        for b in raw.buttons.iter_mut().filter(|b| b[1] == 0.0) {
            b[3] += 0.1; // released 160 px away
        }
        let f = fit(&[], &[raw], &MouseProfile::default(), 2.0, now());
        assert!(f.hold_ms.is_none());
    }

    #[test]
    fn strokes_break_on_a_pause() {
        let k = RawKyoku {
            rect: (1600.0, 900.0),
            frames: (0..20).map(|i| f64::from(i) * 16.0).collect(),
            moves: vec![
                [16.0, 0.1, 0.1],
                [32.0, 0.2, 0.1],
                [200.0, 0.3, 0.1],
                [216.0, 0.4, 0.1],
            ],
            ..RawKyoku::default()
        };
        let s = strokes(&k);
        assert_eq!(s.len(), 2);
        assert!((s[0].length - 0.1).abs() < 1e-9);
        assert!((s[0].duration_ms() - 32.0).abs() < 1e-9);
    }
}
