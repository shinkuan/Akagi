//! What the cursor does between presses: strokes and rests, more of them
//! while our own decision is open.

use super::path::Pt;
use crate::autoplay::context::CanvasRect;
use crate::autoplay::majsoul::coords;
use rand::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Our decision window is open.
    Own,
    /// Everyone else's turn.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub own_active: f64,
    pub other_active: f64,
    /// Canvas widths per second.
    pub speed: [f64; 2],
    /// The controller's multiplier on both shares.
    pub gain: f64,
}

impl Params {
    pub fn active(&self, phase: Phase) -> f64 {
        let base = match phase {
            Phase::Own => self.own_active,
            Phase::Other => self.other_active,
        };
        (base * self.gain).clamp(0.01, 0.95)
    }
}

/// Rest after a stroke of `stroke_ms`, sized so moving time over all time
/// comes out at the phase's share on average.
pub fn rest_ms<R: Rng + ?Sized>(stroke_ms: u32, phase: Phase, p: &Params, rng: &mut R) -> u32 {
    let a = p.active(phase);
    let mean = f64::from(stroke_ms.max(1)) * (1.0 - a) / a;
    let u: f64 = rng.random_range(f64::EPSILON..1.0);
    (-mean * u.ln()).clamp(20.0, 20_000.0) as u32
}

/// The next place to move to, and how long to take.
pub fn next_stroke<R: Rng + ?Sized>(
    at: Pt,
    phase: Phase,
    p: &Params,
    rect: &CanvasRect,
    rng: &mut R,
) -> (Pt, u32) {
    let roll: f64 = rng.random();
    let (hand, centre) = match phase {
        Phase::Own => (0.7, 0.0),
        Phase::Other => (0.3, 0.25),
    };
    let to = if roll < hand {
        let idx = rng.random_range(0..14);
        let (x, y) = coords::get_pai_coord(idx, 13);
        let (px, py) = rect.pixel(
            x + rng.random_range(-0.25..=0.25),
            y + rng.random_range(-0.35..=0.35),
        );
        Pt { x: px, y: py }
    } else if roll < hand + centre {
        let (px, py) = rect.pixel(rng.random_range(5.5..=10.5), rng.random_range(2.0..=6.0));
        Pt { x: px, y: py }
    } else {
        let angle = rng.random_range(0.0..std::f64::consts::TAU);
        let r = rng.random_range(5.0..=40.0);
        Pt {
            x: at.x + r * angle.cos(),
            y: at.y + r * angle.sin(),
        }
    };
    let (lo, hi) = (p.speed[0].min(p.speed[1]), p.speed[0].max(p.speed[1]));
    let speed = rng.random_range(lo.max(0.01)..=hi.max(0.01)) * rect.width.max(1.0);
    let ms = (at.dist(to) / speed * 1000.0).clamp(60.0, 1500.0) as u32;
    (to, ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn params(gain: f64) -> Params {
        Params {
            own_active: 0.5,
            other_active: 0.2,
            speed: [0.5, 1.0],
            gain,
        }
    }

    #[test]
    fn rests_give_the_phase_its_share_of_movement() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        for (phase, want) in [(Phase::Own, 0.5), (Phase::Other, 0.2)] {
            let (mut moving, mut total) = (0.0, 0.0);
            for _ in 0..20_000 {
                let stroke = 300;
                let rest = rest_ms(stroke, phase, &params(1.0), &mut rng);
                moving += f64::from(stroke);
                total += f64::from(stroke + rest);
            }
            assert!(
                (moving / total - want).abs() < 0.02,
                "{phase:?}: {}",
                moving / total
            );
        }
    }

    #[test]
    fn gain_scales_the_share_within_bounds() {
        assert!((params(2.0).active(Phase::Other) - 0.4).abs() < 1e-9);
        assert_eq!(params(10.0).active(Phase::Own), 0.95);
        assert_eq!(params(0.0).active(Phase::Own), 0.01);
    }

    #[test]
    fn strokes_take_time_by_distance_and_speed() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        for _ in 0..500 {
            let at = Pt { x: 800.0, y: 450.0 };
            let (to, ms) = next_stroke(at, Phase::Own, &params(1.0), &rect, &mut rng);
            assert!((60..=1500).contains(&ms));
            let fastest = at.dist(to) / 1600.0 * 1000.0;
            assert!(f64::from(ms) + 1.0 >= fastest.clamp(60.0, 1500.0) - 1.0);
        }
    }
}
