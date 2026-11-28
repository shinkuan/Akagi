//! Turn a calibration's raw kyoku into a [`MouseProfile`].
//!
//! A field with too few samples keeps the value of `base` (the profile in
//! force, or the defaults) and is named in [`Fit::kept`].

use super::session::RawKyoku;
use crate::config::{ClickMode, MouseProfile, MoveRate, MsRange};

/// z of the 10th / 90th percentile of a standard normal.
const Z90: f64 = 1.281_551_6;
/// Gap between moving frames that ends a stroke: this many frame intervals,
/// but never under `STROKE_GAP_MS` — at a high refresh rate a hand moving
/// steadily still leaves frames without a new position.
const STROKE_GAP_FRAMES: f64 = 2.5;
const STROKE_GAP_MS: f64 = 60.0;
/// The client starts a drag past 20 px of travel with the button down.
const DRAG_PX: f64 = 20.0;
const DOUBLE_PX: f64 = 15.0;
const DOUBLE_MAX_MS: f64 = 1500.0;
const APPROACH_LOOKBACK_MS: f64 = 1500.0;
const MIN_RATIO_FRAMES: usize = 50;
const MIN_KYOKU: usize = 3;
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
    p.sample_kyoku = raw.len() as u32;

    match move_rate(raw) {
        Some(r) => p.move_rate = r,
        None => kept.push("move_rate"),
    }
    let ratios: Vec<f64> = raw
        .iter()
        .filter(|k| k.moves.len() >= MIN_RATIO_FRAMES)
        .map(|k| f64::from(k.d) / k.moves.len() as f64)
        .collect();
    match quantile(&ratios, 0.5) {
        Some(r) => p.frame_ratio = r.clamp(0.1, 2.0),
        None => kept.push("frame_ratio"),
    }
    let split = raw.iter().map(phase_split).fold([0usize; 4], |a, s| {
        [a[0] + s[0], a[1] + s[1], a[2] + s[2], a[3] + s[3]]
    });
    let share = |moving: usize, frames: usize| (frames > 0).then(|| moving as f64 / frames as f64);
    match share(split[1], split[0]) {
        Some(s) => p.own_active = s.clamp(0.02, 0.98),
        None => kept.push("own_active"),
    }
    match share(split[3], split[2]) {
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

    let double_kyoku = raw.iter().filter(|k| k.m == 1).count();
    p.click_mode = if double_kyoku * 2 > raw.len() {
        ClickMode::Double
    } else {
        ClickMode::Single
    };
    if p.click_mode == ClickMode::Double {
        let from_client = |f: fn(&RawKyoku) -> u32| {
            let v: Vec<f64> = raw
                .iter()
                .map(f)
                .filter(|v| *v > 0)
                .map(f64::from)
                .collect();
            quantile(&v, 0.5)
        };
        let gap = iqr(&doubles, MIN_DOUBLES)
            .or_else(|| Some((from_client(|k| k.t_ms)?, from_client(|k| k.g_ms)?)));
        match gap {
            Some((a, b)) => p.double_click_ms = ms_range(a, b, 60, 1500),
            None => kept.push("double_click_ms"),
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
fn move_rate(raw: &[RawKyoku]) -> Option<MoveRate> {
    let logs: Vec<f64> = raw
        .iter()
        .filter(|k| k.duration_s > 0.0)
        .map(|k| (f64::from(k.d.max(1)) / k.duration_s).ln())
        .collect();
    if logs.len() < MIN_KYOKU {
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

/// `[own frames, own moving, other frames, other moving]` of one kyoku.
fn phase_split(k: &RawKyoku) -> [usize; 4] {
    let own = |t: f64| k.own.iter().any(|w| t >= w[0] && t < w[1]);
    let own_frames = k.frames.iter().filter(|t| own(**t)).count();
    let own_moving = k.moves.iter().filter(|m| own(m[0])).count();
    [
        own_frames,
        own_moving,
        k.frames.len() - own_frames,
        k.moves.len() - own_moving,
    ]
}

/// Continuous movements in one kyoku's raw input.
pub fn strokes(k: &RawKyoku) -> Vec<Stroke> {
    let frame = frame_interval(&k.frames).unwrap_or(1000.0 / 60.0);
    let aspect = if k.rect.0 > 0.0 {
        k.rect.1 / k.rect.0
    } else {
        9.0 / 16.0
    };
    let gap = (STROKE_GAP_FRAMES * frame).max(STROKE_GAP_MS);
    let mut out = Vec::new();
    let mut cur: Option<(Stroke, [f64; 3])> = None;
    for m in &k.moves {
        cur = Some(match cur {
            Some((mut s, prev)) if m[0] - prev[0] <= gap => {
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

/// Linear-interpolated quantile; `None` for no values.
fn quantile(values: &[f64], q: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let pos = q.clamp(0.0, 1.0) * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(v[lo] + (v[hi] - v[lo]) * (pos - lo as f64))
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

    /// 100 frames 16 ms apart with our window over the first 50; `own` and
    /// `other` of them moving in each half; `d` client frames over 100 s.
    fn kyoku(d: u32, own: usize, other: usize) -> RawKyoku {
        let frames: Vec<f64> = (0..100).map(|i| f64::from(i) * 16.0).collect();
        let moves = frames[..own]
            .iter()
            .chain(&frames[50..50 + other])
            .map(|&t| [t, 0.5, 0.5])
            .collect();
        RawKyoku {
            duration_s: 100.0,
            d,
            rect: (1600.0, 900.0),
            own: vec![[0.0, 800.0]],
            frames,
            moves,
            ..RawKyoku::default()
        }
    }

    /// A raw kyoku of `n` presses: each a 288 ms stroke across 0.3 of the
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
    fn rate_ratio_and_shares_come_from_the_kyoku() {
        // Rates 10, 20, 40 /s; 25 + 10 moving frames each.
        let raw: Vec<RawKyoku> = [1000, 2000, 4000]
            .iter()
            .map(|d| kyoku(*d, 25, 10))
            .collect();
        let f = fit(&raw, &MouseProfile::default(), 2.0, now());
        let r = f.profile.move_rate;
        assert!((r.median - 20.0).abs() < 1e-6, "geometric mean");
        assert!(r.low < 10.0 && r.high > 40.0);
        assert!((f.profile.own_active - 0.5).abs() < 1e-9);
        assert!((f.profile.other_active - 0.2).abs() < 1e-9);
        assert!(
            f.kept.contains(&"frame_ratio"),
            "35 moving frames is too few"
        );
        assert!(f.profile.calibrated);
        assert_eq!(f.profile.sample_kyoku, 3);

        let raw: Vec<RawKyoku> = (0..3).map(|_| kyoku(40, 40, 40)).collect();
        let f = fit(&raw, &MouseProfile::default(), 2.0, now());
        assert!((f.profile.frame_ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn press_timing_comes_from_the_raw_input() {
        let f = fit(&[raw_presses(24)], &MouseProfile::default(), 2.0, now());
        let a = f.profile.approach_ms;
        assert!((a.min as f64 - 288.0).abs() <= 1.0 && (a.max as f64 - 288.0).abs() <= 1.0);
        assert_eq!(f.hover_ms, Some(MsRange::fixed(150)));
        assert_eq!(f.hold_ms, Some(MsRange::fixed(90)));
        // 17 of the 18 steps are seen (the first frame's origin is not), in 288 ms.
        let speed = 0.3 * 17.0 / 18.0 / 0.288;
        assert!((f.profile.speed[0] - speed).abs() < 0.01);
    }

    #[test]
    fn too_few_samples_keep_the_base() {
        let base = MouseProfile::default();
        let f = fit(&[raw_presses(2)], &base, 2.0, now());
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
        let mut raw: Vec<RawKyoku> = (0..4).map(|_| kyoku(1000, 10, 10)).collect();
        for k in &mut raw {
            k.m = 1;
            k.t_ms = 150;
            k.g_ms = 300;
        }
        // Without pairs on the same spot, the client's own min / max stand in.
        let f = fit(&raw, &MouseProfile::default(), 2.0, now());
        assert_eq!(f.profile.click_mode, ClickMode::Double);
        assert_eq!(f.profile.double_click_ms, MsRange::new(150, 300));
        for i in 0..6 {
            let t = f64::from(i) * 3000.0;
            raw[0].buttons.push([t, 1.0, 0.0, 0.5, 0.8]);
            raw[0].buttons.push([t + 60.0, 0.0, 0.0, 0.5, 0.8]);
            raw[0].buttons.push([t + 200.0, 1.0, 0.0, 0.5005, 0.8]);
            raw[0].buttons.push([t + 260.0, 0.0, 0.0, 0.5005, 0.8]);
        }
        let f = fit(&raw, &MouseProfile::default(), 2.0, now());
        assert_eq!(f.profile.double_click_ms, MsRange::fixed(200));
    }

    #[test]
    fn a_drag_is_not_a_hold() {
        let mut raw = raw_presses(10);
        for b in raw.buttons.iter_mut().filter(|b| b[1] == 0.0) {
            b[3] += 0.1; // released 160 px away
        }
        assert!(fit(&[raw], &MouseProfile::default(), 2.0, now())
            .hold_ms
            .is_none());
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

    /// Regression: at 240 Hz a steady move skips a frame or two now and
    /// then; a frame-count gap cut it into slivers and the approach fit
    /// collapsed to its floor.
    #[test]
    fn a_high_refresh_stroke_with_skipped_frames_stays_whole() {
        let frame = 1000.0 / 240.0;
        let frames: Vec<f64> = (0..120).map(|i| f64::from(i) * frame).collect();
        let moves = frames
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 8 < 5)
            .map(|(i, &t)| [t, 0.1 + i as f64 * 0.002, 0.5])
            .collect();
        let k = RawKyoku {
            rect: (1600.0, 900.0),
            frames,
            moves,
            ..RawKyoku::default()
        };
        let s = strokes(&k);
        assert_eq!(s.len(), 1, "{s:?}");
        assert!(s[0].duration_ms() > 480.0);
    }

    #[test]
    fn quantiles_interpolate() {
        assert_eq!(quantile(&[3.0, 1.0, 2.0], 0.5), Some(2.0));
        assert_eq!(quantile(&[0.0, 10.0], 0.25), Some(2.5));
        assert_eq!(quantile(&[], 0.5), None);
    }
}
