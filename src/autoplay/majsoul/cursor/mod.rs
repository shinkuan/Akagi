//! The Majsoul autoplay cursor: the one place mouse events are sent from.
//!
//! Between presses it moves the way the user's calibrated profile says they
//! do — strokes and rests, more of them while our own decision is open — and
//! keeps each kyoku's moving-frame rate on a target drawn from the user's own
//! distribution ([`control`]). A press is an approach stroke onto the target,
//! a rest, then the button. Only runs while Majsoul autoplay is allowed (see
//! `calibration::majsoul_gate`) and a kyoku is being played.

pub mod control;
pub mod idle;
pub mod path;

use crate::autoplay::cdp_input::{dispatch_click_shaped, evaluate_canvas_rect};
use crate::autoplay::context::{AutoplayContext, CanvasRect};
use crate::autoplay::majsoul::calibration::{majsoul_gate, service::now_ms};
use crate::config::{AppConfig, MouseProfile, Platform};
use anyhow::{anyhow, Result};
use chromiumoxide::layout::Point;
use chromiumoxide::page::Page;
use control::Controller;
use idle::{Params, Phase};
use path::{Bounds, Pt, STEP_MS};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::Instant;

const RECT_TTL: Duration = Duration::from_secs(30);
const CONFIG_TTL: Duration = Duration::from_millis(500);
const RETRY_LATER: Duration = Duration::from_secs(1);

/// How one press is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClickShape {
    pub approach_ms: u32,
    pub hover_ms: u32,
    pub hold_ms: u32,
    /// Nudge the cursor a pixel mid-press and back (see
    /// `cdp_input::dispatch_click_shaped`).
    pub jiggle: bool,
    /// A double-click discard: the gap before the second press.
    pub double_gap_ms: Option<u32>,
}

struct ClickCmd {
    x: f64,
    y: f64,
    shape: ClickShape,
    done: oneshot::Sender<Result<()>>,
}

#[derive(Clone)]
pub struct Cursor {
    tx: mpsc::Sender<ClickCmd>,
    pos: Arc<Mutex<Option<Pt>>>,
}

impl Cursor {
    pub fn spawn(cfg: Arc<RwLock<AppConfig>>, ctx: Arc<AutoplayContext>) -> Self {
        let (tx, rx) = mpsc::channel(4);
        let pos = Arc::new(Mutex::new(None));
        let actor = Actor {
            cfg,
            ctx,
            rx,
            pos: pos.clone(),
            rng: StdRng::from_os_rng(),
            controller: Controller::default(),
            idle: Idle::Rest {
                until: Instant::now(),
            },
            last_stroke_ms: None,
            rect: None,
            snap: None,
        };
        tauri::async_runtime::spawn(actor.run());
        Self { tx, pos }
    }

    /// How long the approach onto `(x, y)` will take from where the cursor is.
    pub fn approach_ms<R: Rng + ?Sized>(
        &self,
        profile: &MouseProfile,
        rect: &CanvasRect,
        x: f64,
        y: f64,
        rng: &mut R,
    ) -> u32 {
        let from = self
            .pos
            .lock()
            .ok()
            .and_then(|p| *p)
            .unwrap_or_else(|| centre(rect));
        approach_for(profile, rect, from, Pt { x, y }, rng)
    }

    /// Move onto `(x, y)` and press there. Resolves once the button is up.
    pub async fn click(&self, x: f64, y: f64, shape: ClickShape) -> Result<()> {
        let (done, rx) = oneshot::channel();
        self.tx
            .send(ClickCmd { x, y, shape, done })
            .await
            .map_err(|_| anyhow!("cursor stopped"))?;
        rx.await.map_err(|_| anyhow!("cursor dropped the click"))?
    }
}

fn centre(rect: &CanvasRect) -> Pt {
    Pt {
        x: rect.x + rect.width / 2.0,
        y: rect.y + rect.height / 2.0,
    }
}

/// The profile's approach time, stretched for a longer way and shrunk for a
/// shorter one.
pub fn approach_for<R: Rng + ?Sized>(
    profile: &MouseProfile,
    rect: &CanvasRect,
    from: Pt,
    to: Pt,
    rng: &mut R,
) -> u32 {
    let base = f64::from(profile.approach_ms.draw(rng).ms());
    let reference = 0.3 * rect.width.max(1.0);
    let factor = (from.dist(to) / reference).sqrt().clamp(0.6, 1.6);
    (base * factor).clamp(60.0, 2500.0) as u32
}

enum Idle {
    Rest {
        until: Instant,
    },
    Moving {
        page: Page,
        pts: Vec<(u32, Pt)>,
        next: usize,
        started: Instant,
        ms: u32,
    },
}

#[derive(Clone)]
struct Snapshot {
    allowed: bool,
    profile: MouseProfile,
}

struct Actor {
    cfg: Arc<RwLock<AppConfig>>,
    ctx: Arc<AutoplayContext>,
    rx: mpsc::Receiver<ClickCmd>,
    pos: Arc<Mutex<Option<Pt>>>,
    rng: StdRng,
    controller: Controller,
    idle: Idle,
    /// Length of the stroke just finished; the next rest is sized from it.
    last_stroke_ms: Option<u32>,
    rect: Option<(CanvasRect, Instant)>,
    snap: Option<(Snapshot, Instant)>,
}

impl Actor {
    async fn run(mut self) {
        let mut tick = tokio::time::interval(Duration::from_millis(u64::from(STEP_MS)));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                cmd = self.rx.recv() => {
                    let Some(cmd) = cmd else { return };
                    let r = self.press(cmd.x, cmd.y, cmd.shape).await;
                    let _ = cmd.done.send(r);
                    let rest = self.rng.random_range(80..400);
                    self.idle = Idle::Rest { until: Instant::now() + Duration::from_millis(rest) };
                    self.last_stroke_ms = None;
                }
                _ = tick.tick() => self.idle_step().await,
            }
        }
    }

    fn set_pos(&self, p: Pt) {
        if let Ok(mut g) = self.pos.lock() {
            *g = Some(p);
        }
    }

    fn current_pos(&self) -> Option<Pt> {
        self.pos.lock().ok().and_then(|g| *g)
    }

    async fn snapshot(&mut self) -> Snapshot {
        if let Some((s, at)) = &self.snap {
            if at.elapsed() < CONFIG_TTL {
                return s.clone();
            }
        }
        let cfg = self.cfg.read().await;
        let profile = cfg.autoplay.majsoul.mouse.clone();
        let allowed = cfg.autoplay.enabled
            && cfg.platform.kind == Platform::Majsoul
            && majsoul_gate(&profile, self.ctx.majsoul_mouse.calibrating()).is_ok();
        drop(cfg);
        let s = Snapshot { allowed, profile };
        self.snap = Some((s.clone(), Instant::now()));
        s
    }

    async fn rect(&mut self, page: &Page) -> Option<CanvasRect> {
        if let Some((r, at)) = self.rect {
            if at.elapsed() < RECT_TTL {
                return Some(r);
            }
        }
        let r = match *self.ctx.canvas_rect.read().await {
            Some(r) => r,
            None => evaluate_canvas_rect(page).await.ok()?,
        };
        self.rect = Some((r, Instant::now()));
        Some(r)
    }

    fn rest_for(&mut self, d: Duration) {
        self.idle = Idle::Rest {
            until: Instant::now() + d,
        };
        self.last_stroke_ms = None;
    }

    async fn idle_step(&mut self) {
        let snap = self.snapshot().await;
        let live = self.ctx.majsoul_mouse.with(|s| s.live());
        if !snap.allowed || !live.in_kyoku {
            if matches!(self.idle, Idle::Moving { .. }) {
                self.rest_for(Duration::ZERO);
            }
            return;
        }
        self.controller
            .follow(&live, &snap.profile.move_rate, &mut self.rng);
        let ratio = live.frame_ratio.unwrap_or(snap.profile.frame_ratio);
        self.controller.update(&live, ratio, now_ms());

        let now = Instant::now();
        match std::mem::replace(&mut self.idle, Idle::Rest { until: now }) {
            Idle::Moving {
                page,
                pts,
                mut next,
                started,
                ms,
            } => {
                let elapsed = started.elapsed().as_millis() as u32;
                let mut due = None;
                while next < pts.len() && pts[next].0 <= elapsed {
                    due = Some(pts[next].1);
                    next += 1;
                }
                if let Some(p) = due {
                    if page.move_mouse(Point::new(p.x, p.y)).await.is_err() {
                        self.rest_for(RETRY_LATER);
                        return;
                    }
                    self.set_pos(p);
                }
                if next >= pts.len() {
                    self.last_stroke_ms = Some(ms);
                } else {
                    self.idle = Idle::Moving {
                        page,
                        pts,
                        next,
                        started,
                        ms,
                    };
                }
            }
            Idle::Rest { until } => {
                if now < until {
                    self.idle = Idle::Rest { until };
                    return;
                }
                let own = self.ctx.time_budget.read().ok().and_then(|g| *g).is_some();
                let phase = if own { Phase::Own } else { Phase::Other };
                let params = Params {
                    own_active: snap.profile.own_active,
                    other_active: snap.profile.other_active,
                    speed: snap.profile.speed,
                    gain: self.controller.gain(),
                };
                if let Some(ms) = self.last_stroke_ms.take() {
                    let rest = idle::rest_ms(ms, phase, &params, &mut self.rng);
                    self.idle = Idle::Rest {
                        until: now + Duration::from_millis(u64::from(rest)),
                    };
                    return;
                }
                let Some(page) = self.ctx.page.read().await.clone() else {
                    self.rest_for(RETRY_LATER);
                    return;
                };
                let Some(rect) = self.rect(&page).await else {
                    self.rest_for(RETRY_LATER);
                    return;
                };
                let at = self.current_pos().unwrap_or_else(|| centre(&rect));
                let (to, ms) = idle::next_stroke(at, phase, &params, &rect, &mut self.rng);
                let pts = path::stroke(at, to, ms, Bounds::of(&rect), &mut self.rng);
                self.idle = Idle::Moving {
                    page,
                    pts,
                    next: 0,
                    started: now,
                    ms,
                };
            }
        }
    }

    async fn press(&mut self, x: f64, y: f64, shape: ClickShape) -> Result<()> {
        let page = self
            .ctx
            .page
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow!("no page handle"))?;
        let target = Pt { x, y };
        match self.rect(&page).await {
            Some(rect) => {
                let from = self.current_pos().unwrap_or(target);
                let pts = path::approach(
                    from,
                    target,
                    shape.approach_ms,
                    Bounds::of(&rect),
                    &mut self.rng,
                );
                let start = Instant::now();
                for (t, p) in pts {
                    tokio::time::sleep_until(start + Duration::from_millis(u64::from(t))).await;
                    page.move_mouse(Point::new(p.x, p.y)).await?;
                    self.set_pos(p);
                }
            }
            None => self.set_pos(target),
        }
        // Already on the target, so its opening move moves nothing; the
        // second press of a double click waits out the gap as its "hover".
        self.set_pos(target);
        dispatch_click_shaped(&page, x, y, shape.hover_ms, shape.hold_ms, shape.jiggle).await?;
        if let Some(gap) = shape.double_gap_ms {
            dispatch_click_shaped(&page, x, y, gap, shape.hold_ms, false).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MsRange;
    use rand::SeedableRng;

    #[test]
    fn approach_time_scales_with_the_way_to_go() {
        let mut rng = StdRng::seed_from_u64(1);
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 562.5,
        };
        let profile = MouseProfile {
            approach_ms: MsRange::fixed(300),
            ..MouseProfile::default()
        };
        let from = Pt { x: 500.0, y: 300.0 };
        let near = approach_for(&profile, &rect, from, Pt { x: 510.0, y: 300.0 }, &mut rng);
        let reference = approach_for(&profile, &rect, from, Pt { x: 800.0, y: 300.0 }, &mut rng);
        let far = approach_for(
            &profile,
            &rect,
            Pt { x: 0.0, y: 0.0 },
            Pt {
                x: 1000.0,
                y: 560.0,
            },
            &mut rng,
        );
        assert_eq!(near, 180);
        assert_eq!(reference, 300);
        assert_eq!(far, 480);
    }
}
