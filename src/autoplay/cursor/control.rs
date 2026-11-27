//! Keeps each kyoku's moving-frame rate on a target drawn from the user's
//! own distribution, by scaling how much of the time the cursor moves.

use crate::autoplay::mouse::LiveRate;
use crate::config::MoveRate;
use rand::Rng;
use rand_distr::{Distribution, Normal};

const Z90: f64 = 1.281_551_6;
/// No correction before this much of the kyoku has been seen.
const SETTLE_MS: f64 = 10_000.0;
const ADJUST_EVERY_MS: f64 = 5_000.0;
pub const GAIN_MIN: f64 = 0.3;
pub const GAIN_MAX: f64 = 3.0;

#[derive(Debug, Clone)]
pub struct Controller {
    target: f64,
    gain: f64,
    kyoku_started_ms: f64,
    last_adjust_ms: f64,
}

impl Default for Controller {
    fn default() -> Self {
        Self {
            target: 0.0,
            gain: 1.0,
            kyoku_started_ms: f64::NAN,
            last_adjust_ms: 0.0,
        }
    }
}

impl Controller {
    pub fn gain(&self) -> f64 {
        self.gain
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    /// Draw the new kyoku's target if `live` is a kyoku not seen before.
    /// The gain carries over: it is the correction for this machine.
    pub fn follow<R: Rng + ?Sized>(&mut self, live: &LiveRate, rate: &MoveRate, rng: &mut R) {
        if live.in_kyoku && live.started_ms != self.kyoku_started_ms {
            self.kyoku_started_ms = live.started_ms;
            self.last_adjust_ms = live.started_ms;
            self.target = draw_rate(rate, rng);
        }
    }

    /// Compare the kyoku so far with the target and nudge the gain.
    pub fn update(&mut self, live: &LiveRate, frame_ratio: f64, now_ms: f64) {
        if !live.in_kyoku || self.target <= 0.0 {
            return;
        }
        let elapsed = now_ms - live.started_ms;
        if elapsed < SETTLE_MS || now_ms - self.last_adjust_ms < ADJUST_EVERY_MS {
            return;
        }
        self.last_adjust_ms = now_ms;
        let measured = f64::from(live.page_moving) * frame_ratio / (elapsed / 1000.0);
        let factor = if measured > 0.01 {
            (self.target / measured).sqrt().clamp(0.67, 1.5)
        } else {
            1.5
        };
        self.gain = (self.gain * factor).clamp(GAIN_MIN, GAIN_MAX);
    }
}

/// One kyoku's target rate from the log-normal the profile describes.
pub fn draw_rate<R: Rng + ?Sized>(rate: &MoveRate, rng: &mut R) -> f64 {
    let median = rate.median.max(0.01);
    let sigma = if rate.high > median {
        (rate.high / median).ln() / Z90
    } else {
        0.3
    };
    let z: f64 = Normal::new(0.0, 1.0).map_or(0.0, |n| n.sample(rng));
    let lo = rate.low.min(median) * 0.8;
    let hi = rate.high.max(median) * 1.2;
    (median * (sigma * z).exp()).clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn live(started: f64, moving: u32) -> LiveRate {
        LiveRate {
            in_kyoku: true,
            started_ms: started,
            page_frames: 0,
            page_moving: moving,
            frame_ratio: None,
        }
    }

    #[test]
    fn targets_follow_the_profile() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let rate = MoveRate {
            median: 15.0,
            low: 8.0,
            high: 30.0,
        };
        let mut v: Vec<f64> = (0..4000).map(|_| draw_rate(&rate, &mut rng)).collect();
        v.sort_by(f64::total_cmp);
        let med = v[v.len() / 2];
        assert!((med - 15.0).abs() < 1.0, "{med}");
        assert!(v.iter().all(|r| (6.4..=36.0).contains(r)));
    }

    /// A plant whose moving frames are proportional to the gain: the
    /// controller settles where the rate meets the target.
    #[test]
    fn the_gain_converges_and_stays_bounded() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(2);
        let rate = MoveRate {
            median: 20.0,
            low: 20.0,
            high: 20.0,
        };
        let mut c = Controller::default();
        c.follow(&live(0.0, 0), &rate, &mut rng);
        let target = c.target();
        // At gain 1 this machine produces 5 frames/s.
        let mut frames = 0.0;
        let mut t = 0.0;
        while t < 600_000.0 {
            t += 1000.0;
            frames += 5.0 * c.gain();
            c.update(&live(0.0, frames as u32), 1.0, t);
        }
        assert!((c.gain() - GAIN_MAX).abs() < 1e-9, "capped: {}", c.gain());

        let mut c = Controller::default();
        c.follow(&live(0.0, 0), &rate, &mut rng);
        let (mut frames, mut t) = (0.0, 0.0);
        while t < 900_000.0 {
            t += 1000.0;
            frames += 10.0 * c.gain();
            c.update(&live(0.0, frames as u32), 1.0, t);
        }
        let achieved = frames / (t / 1000.0);
        assert!(
            (achieved - target).abs() / target < 0.1,
            "{achieved} vs {target}"
        );
    }

    #[test]
    fn nothing_happens_outside_a_kyoku_or_before_it_settles() {
        let mut c = Controller::default();
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        c.follow(&live(0.0, 0), &MoveRate::default(), &mut rng);
        c.update(&live(0.0, 0), 1.0, 5_000.0);
        assert_eq!(c.gain(), 1.0);
        let off = LiveRate::default();
        c.update(&off, 1.0, 60_000.0);
        assert_eq!(c.gain(), 1.0);
    }
}
