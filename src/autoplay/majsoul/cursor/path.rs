//! Stroke shapes: where the cursor is at each step of one movement.

use crate::autoplay::context::CanvasRect;
use rand::Rng;
use std::f64::consts::PI;

/// Time between emitted points. At or below a 60 Hz frame, so every frame of
/// a stroke sees the cursor somewhere new.
pub const STEP_MS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn dist(&self, o: Pt) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }
}

/// Where strokes may go: the canvas, inset so no point lands on its edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Bounds {
    pub fn of(rect: &CanvasRect) -> Self {
        let inset = 2.0_f64.min(rect.width / 4.0).min(rect.height / 4.0);
        Self {
            x0: rect.x + inset,
            y0: rect.y + inset,
            x1: rect.x + rect.width - inset,
            y1: rect.y + rect.height - inset,
        }
    }

    pub fn clamp(&self, p: Pt) -> Pt {
        Pt {
            x: p.x.clamp(self.x0, self.x1),
            y: p.y.clamp(self.y0, self.y1),
        }
    }
}

/// One movement from `from` to `to` over `duration_ms`: a minimum-jerk
/// profile along a slight bow, with sub-pixel noise. Points are `(ms since
/// the stroke began, position)`; the last is exactly `to` (clamped).
pub fn stroke<R: Rng + ?Sized>(
    from: Pt,
    to: Pt,
    duration_ms: u32,
    bounds: Bounds,
    rng: &mut R,
) -> Vec<(u32, Pt)> {
    let steps = (duration_ms / STEP_MS).max(1);
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let dist = from.dist(to);
    let (nx, ny) = if dist > 0.0 {
        (-dy / dist, dx / dist)
    } else {
        (0.0, 0.0)
    };
    let bow = rng.random_range(-0.12..=0.12) * dist;
    (1..=steps)
        .map(|i| {
            let tau = f64::from(i) / f64::from(steps);
            let t = (f64::from(duration_ms) * tau).round() as u32;
            if i == steps {
                return (t, bounds.clamp(to));
            }
            let s = tau.powi(3) * (10.0 - 15.0 * tau + 6.0 * tau * tau);
            let off = bow * (PI * s).sin();
            let p = Pt {
                x: from.x + dx * s + nx * off + rng.random_range(-0.4..=0.4),
                y: from.y + dy * s + ny * off + rng.random_range(-0.4..=0.4),
            };
            (t, bounds.clamp(p))
        })
        .collect()
}

/// The movement onto a press target: one stroke, or now and then one that
/// overshoots a little and corrects.
pub fn approach<R: Rng + ?Sized>(
    from: Pt,
    to: Pt,
    duration_ms: u32,
    bounds: Bounds,
    rng: &mut R,
) -> Vec<(u32, Pt)> {
    let dist = from.dist(to);
    if dist > 40.0 && duration_ms >= 120 && rng.random_bool(0.25) {
        let over = rng.random_range(0.02..=0.06) * dist;
        let past = Pt {
            x: to.x + (to.x - from.x) / dist * over,
            y: to.y + (to.y - from.y) / dist * over,
        };
        let main = duration_ms * 17 / 20;
        let mut out = stroke(from, past, main, bounds, rng);
        out.extend(
            stroke(past, to, duration_ms - main, bounds, rng)
                .into_iter()
                .map(|(t, p)| (t + main, p)),
        );
        out
    } else {
        stroke(from, to, duration_ms, bounds, rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn rect() -> CanvasRect {
        CanvasRect {
            x: 10.0,
            y: 20.0,
            width: 1280.0,
            height: 720.0,
        }
    }

    #[test]
    fn a_stroke_ends_on_its_target_inside_the_canvas() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let b = Bounds::of(&rect());
        for _ in 0..200 {
            let from = Pt {
                x: rng.random_range(10.0..1290.0),
                y: rng.random_range(20.0..740.0),
            };
            let to = Pt {
                x: rng.random_range(10.0..1290.0),
                y: rng.random_range(20.0..740.0),
            };
            let pts = approach(from, to, rng.random_range(60..900), b, &mut rng);
            assert_eq!(pts.last().unwrap().1, b.clamp(to));
            for (_, p) in &pts {
                assert_eq!(*p, b.clamp(*p), "outside the canvas: {p:?}");
            }
        }
    }

    #[test]
    fn points_are_spaced_by_the_step_and_end_on_time() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(4);
        let b = Bounds::of(&rect());
        let pts = stroke(
            Pt { x: 100.0, y: 100.0 },
            Pt { x: 700.0, y: 400.0 },
            400,
            b,
            &mut rng,
        );
        assert_eq!(pts.len(), 50);
        assert_eq!(pts.last().unwrap().0, 400);
        for w in pts.windows(2) {
            assert!(w[1].0 > w[0].0);
            assert!(w[1].0 - w[0].0 <= STEP_MS + 1);
        }
    }

    #[test]
    fn minimum_jerk_is_slow_at_both_ends() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(5);
        let b = Bounds::of(&rect());
        let from = Pt { x: 100.0, y: 360.0 };
        let pts = stroke(
            from,
            Pt {
                x: 1100.0,
                y: 360.0,
            },
            800,
            b,
            &mut rng,
        );
        let step = |i: usize| pts[i + 1].1.dist(pts[i].1);
        let mid = pts.len() / 2;
        assert!(step(0) < step(mid) / 3.0);
        assert!(step(pts.len() - 2) < step(mid) / 3.0);
    }

    #[test]
    fn a_zero_length_stroke_stays_put() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(6);
        let b = Bounds::of(&rect());
        let p = Pt { x: 300.0, y: 300.0 };
        let pts = stroke(p, p, 5, b, &mut rng);
        assert_eq!(pts, vec![(5, p)]);
    }
}
