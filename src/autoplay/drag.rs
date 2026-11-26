//! The human-like drag trajectory for a discard ("flick a tile out").
//!
//! Pure geometry and timing — no IO. The manager converts the tile's
//! 16:9-normalised position into CSS pixels, picks a release point above
//! the hand, and asks this module for the intermediate mouse path; the
//! CDP dispatcher (`cdp_input::dispatch_drag`) then walks it event by
//! event. Keeping the generator pure is what makes the shape testable:
//! every property a reviewer would ask about (does it go up, is the
//! jitter bounded, does the timing add up) is pinned by a unit test.
//!
//! What makes a drag read as human rather than as a linear tween:
//!
//! - the press point is never the exact centre of the tile (±3 px);
//! - the speed profile is slow → fast → slow (smoothstep), because a
//!   person accelerates out of the pick and decelerates into the release;
//! - the path wobbles (per-point Gaussian jitter, σ ≈ 1.5 px) instead of
//!   running dead straight;
//! - the cursor sometimes hesitates mid-drag (a 30–90 ms micro-pause) and
//!   sometimes overshoots the release point slightly before pulling back
//!   the last few pixels — the two most common human fumbles.

use rand::Rng;
use rand_distr::StandardNormal;

/// One waypoint on the drag path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragStep {
    /// CSS-pixel position of the cursor for this mouse-move event.
    /// Entry 0 is the press point (jittered off the tile centre — the
    /// dispatcher aims the mousedown here); the last entry is the release
    /// point.
    pub point: (f64, f64),
    /// How long to sleep *before* moving to `point`. Entry 0's delay is
    /// the beat between the press and the first real movement; the sum of
    /// all delays is the on-button travel time.
    pub delay_ms: u64,
}

/// Per-point positional jitter, CSS px. Large enough to see on a
/// recorded cursor, small enough that every point stays on the tile
/// sprite at press and above the discard threshold at release.
const JITTER_SIGMA: f64 = 1.5;

/// The press point is offset from the tile centre by up to this much,
/// CSS px — a person never lands on the exact centre.
const START_JITTER: f64 = 3.0;

/// The release point is offset from the caller's target by up to this
/// much, CSS px — the exact release spot is wherever the finger happened
/// to come up.
const END_JITTER: f64 = 6.0;

/// Chance (0..1) of a mid-drag hesitation.
const PAUSE_PROBABILITY: f64 = 0.2;
/// Hesitation length bounds, ms.
const PAUSE_MIN_MS: u64 = 30;
const PAUSE_MAX_MS: u64 = 90;

/// Chance (0..1) of overshooting the release point and pulling back.
const OVERSHOOT_PROBABILITY: f64 = 0.4;
/// Overshoot distance beyond the target, as a fraction of the drag
/// length. A person overshoots a little, not again by a tile's width.
const OVERSHOOT_MIN_FRACTION: f64 = 0.03;
const OVERSHOOT_MAX_FRACTION: f64 = 0.08;
/// How far back the correcting pull lands short of the target, CSS px.
const PULLBACK_MIN_PX: f64 = 2.0;
const PULLBACK_MAX_PX: f64 = 6.0;

/// Target interval between consecutive mouse-move events, ms. Real
/// pointers update at display cadence; CDP tolerates that pace and the
/// engine keeps up with it.
const STEP_INTERVAL_MS: f64 = 14.0;

/// Build the mouse path for one drag from `start` to `end`.
///
/// `duration_ms` is the intended total travel time (sum of the returned
/// delays, within rounding); the caller draws it from
/// `MajsoulAutoplayConfig::drag_duration_ms` like every other per-decision
/// timing. `start`/`end` are CSS pixels; both are nudged a little inside
/// this function, so callers must not rely on landing exactly on either.
pub fn drag_path<R: Rng + ?Sized>(
    start: (f64, f64),
    end: (f64, f64),
    duration_ms: u32,
    rng: &mut R,
) -> Vec<DragStep> {
    let duration = f64::from(duration_ms.max(60));
    // Event count follows the duration so the per-event interval stays in
    // the pointer-cadence ballpark rather than turning into a slideshow.
    let n = ((duration / STEP_INTERVAL_MS).round() as usize).clamp(8, 36);

    // Nudge both endpoints off their exact values first: the pullback at
    // the end must not be able to cross below the (jittered) start, so the
    // end gets the same order of nudge the start does.
    let start = (
        start.0 + rng.random_range(-START_JITTER..=START_JITTER),
        start.1 + rng.random_range(-START_JITTER..=START_JITTER),
    );
    let end = (
        end.0 + rng.random_range(-END_JITTER..=END_JITTER),
        end.1 + rng.random_range(-END_JITTER..=END_JITTER),
    );

    // Optional overshoot: travel a little past the target, then pull back
    // the correcting few pixels. The overshoot waypoint is the one the
    // release would otherwise sit at — so with overshoot on, the path ends
    // at the pullback point, not at `end`.
    let overshoots = rng.random_bool(OVERSHOOT_PROBABILITY);
    let segment = (end.0 - start.0, end.1 - start.1);
    let overshoot_end = if overshoots {
        let k = 1.0 + rng.random_range(OVERSHOOT_MIN_FRACTION..=OVERSHOOT_MAX_FRACTION);
        (start.0 + segment.0 * k, start.1 + segment.1 * k)
    } else {
        end
    };
    let pullback_end = if overshoots {
        let back = rng.random_range(PULLBACK_MIN_PX..=PULLBACK_MAX_PX);
        let len = segment.0.hypot(segment.1).max(1.0);
        let (ux, uy) = (segment.0 / len, segment.1 / len);
        (overshoot_end.0 - ux * back, overshoot_end.1 - uy * back)
    } else {
        overshoot_end
    };

    // Waypoint positions: smoothstep-eased interpolation, jittered per
    // point. t runs 0..1 over the whole travel; both overshoot and
    // pullback endpoints share it so the timing stays one gesture.
    //
    // With overshoot, the arc travels toward the point past the target and
    // only the last waypoint is the pullback — so the last visible motion
    // is the correcting move, which is what makes the release look
    // deliberate. Without it, pullback_end *is* the target and the arc is
    // the ordinary start→end tween.
    let ease = |t: f64| t * t * (3.0 - 2.0 * t);
    let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let jitter = |rng: &mut R| rng.sample::<f64, _>(StandardNormal) * JITTER_SIGMA;
    let mut points: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let t = ease(i as f64 / n as f64);
            (
                lerp(start.0, overshoot_end.0, t),
                lerp(start.1, overshoot_end.1, t),
            )
        })
        .collect();
    points.push(pullback_end);
    // The jitter skips index 0: that entry is the press point, where the
    // cursor already sits — jittering it would make the first recorded
    // move disagree with the press.
    for p in &mut points[1..] {
        p.0 += jitter(rng);
        p.1 += jitter(rng);
    }

    // Timing: one draw per step around the even share, then rescaled so
    // the travel sums to (about) the requested duration. The pause, if
    // drawn, rides on top — a hesitation is extra time, not a relabelled
    // slice of the smooth part.
    let base = duration / points.len() as f64;
    let delays: Vec<f64> = points
        .iter()
        .map(|_| (base * rng.random_range(0.7..=1.3)).clamp(4.0, 60.0))
        .collect();
    let drawn: f64 = delays.iter().sum();
    let scale = duration / drawn.max(1.0);
    let pause_index = if rng.random_bool(PAUSE_PROBABILITY) {
        Some(rng.random_range(1..points.len().saturating_sub(1).max(1)))
    } else {
        None
    };
    let mut steps = Vec::with_capacity(points.len());
    let mut paused = false;
    for (i, point) in points.into_iter().enumerate() {
        let mut delay = (delays[i] * scale).round().max(3.0) as u64;
        if Some(i) == pause_index && !paused {
            paused = true;
            delay += rng.random_range(PAUSE_MIN_MS..=PAUSE_MAX_MS);
        }
        steps.push(DragStep {
            point,
            delay_ms: delay,
        });
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn sum_delay(path: &[DragStep]) -> u64 {
        path.iter().map(|s| s.delay_ms).sum()
    }

    /// The whole point of the gesture: the cursor ends up meaningfully
    /// above where it started, and the travel is coarsely monotonic (the
    /// jitter may wiggle a pixel or two between neighbours, but the path
    /// as a whole must climb).
    #[test]
    fn path_goes_up_and_never_backtracks_far() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let path = drag_path((400.0, 700.0), (390.0, 320.0), 300, &mut rng);
        assert!(path.len() >= 8, "too few events: {}", path.len());
        let first = path.first().unwrap().point;
        let last = path.last().unwrap().point;
        assert!(
            last.1 < first.1 - 300.0,
            "did not travel up: {first:?} → {last:?}"
        );
        let quarter = path.len() / 4;
        let three_quarters = path.len() * 3 / 4;
        assert!(
            path[three_quarters].point.1 < path[quarter].point.1,
            "y at 3/4 ({:?}) not above y at 1/4 ({:?})",
            path[three_quarters].point,
            path[quarter].point
        );
        // No single jump moves downward by more than the jitter bound.
        for w in path.windows(2) {
            assert!(
                w[1].point.1 - w[0].point.1 <= 3.0 * JITTER_SIGMA,
                "backtracked more than jitter between {:?} and {:?}",
                w[0].point,
                w[1].point
            );
        }
    }

    /// Same seed, same path — a humanised gesture that changes between
    /// identical decisions is fine, but one that changes between
    /// identical *seeds* means the generator reads hidden state.
    #[test]
    fn same_seed_same_path() {
        let a = drag_path(
            (100.0, 500.0),
            (90.0, 200.0),
            250,
            &mut rand::rngs::StdRng::seed_from_u64(42),
        );
        let b = drag_path(
            (100.0, 500.0),
            (90.0, 200.0),
            250,
            &mut rand::rngs::StdRng::seed_from_u64(42),
        );
        assert_eq!(a, b);
    }

    /// The travel time is what the caller asked for, within rounding —
    /// the delay model budgets the whole decision, so a drag that silently
    /// takes twice its configured time would eat the window.
    #[test]
    fn travel_time_matches_the_request() {
        for duration in [60, 120, 220, 450, 900] {
            let mut rng = rand::rngs::StdRng::seed_from_u64(duration as u64);
            let path = drag_path((300.0, 700.0), (350.0, 300.0), duration, &mut rng);
            let total = sum_delay(&path);
            let target = duration.max(60) as u64;
            assert!(
                (total as f64).approximately(target, 0.30),
                "duration {duration}: travel summed to {total} ms"
            );
        }
    }

    /// Every point stays in a corridor around the straight start→end
    /// line: jitter keeps it inside ±12 px sideways, overshoot/pullback
    /// stay within 15% of the segment length along it. The press point is
    /// within the start jitter of the requested start.
    #[test]
    fn corridor_around_the_requested_segment() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(9);
        let start = (500.0, 700.0);
        let end = (480.0, 300.0);
        let path = drag_path(start, end, 400, &mut rng);
        let seg = (end.0 - start.0, end.1 - start.1);
        let len = seg.0.hypot(seg.1);
        for step in &path {
            let rel = (step.point.0 - start.0, step.point.1 - start.1);
            // Along-track position via projection; sideways via the
            // cross product magnitude.
            let along = (rel.0 * seg.0 + rel.1 * seg.1) / len;
            let side = (rel.0 * seg.1 - rel.1 * seg.0).abs() / len;
            assert!(
                side <= 12.0,
                "point {:?} strays {} px sideways",
                step.point,
                side
            );
            assert!(
                along >= -12.0 && along <= len * 1.15,
                "point {:?} at {} px along-track is outside the corridor",
                step.point,
                along
            );
        }
        let press = path[0].point;
        assert!(
            (press.0 - start.0).abs() <= START_JITTER && (press.1 - start.1).abs() <= START_JITTER,
            "press point {press:?} not near the requested start {start:?}"
        );
    }

    /// Both human fumbles actually occur across seeds — otherwise the
    /// constants above are decorative and the gesture is a straight tween
    /// with noise.
    #[test]
    fn pauses_and_overshoots_both_happen() {
        let mut pauses = 0;
        let mut overshoots = 0;
        for seed in 0..80 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            let path = drag_path((100.0, 700.0), (100.0, 300.0), 350, &mut rng);
            let total = sum_delay(&path);
            let avg = total as f64 / path.len() as f64;
            // A pause rides on top of the even share — the paused step's
            // delay is the share plus 30-90 ms, well over 3x the average.
            if path.iter().any(|s| s.delay_ms as f64 >= avg * 3.0) {
                pauses += 1;
            }
            // An overshoot pulls back towards the start: some point before
            // the last sits farther along the travel than the last one.
            let last_y = path.last().unwrap().point.1;
            if path.iter().rev().skip(1).any(|s| s.point.1 < last_y - 1.0) {
                overshoots += 1;
            }
        }
        assert!(pauses >= 6, "pause rate suspiciously low: {pauses}/80");
        assert!(
            overshoots >= 12,
            "overshoot rate suspiciously low: {overshoots}/80"
        );
    }

    trait Approximately {
        fn approximately(&self, other: u64, tolerance: f64) -> bool;
    }
    impl Approximately for f64 {
        fn approximately(&self, other: u64, tolerance: f64) -> bool {
            (self - other as f64).abs() <= other as f64 * tolerance
        }
    }
}
