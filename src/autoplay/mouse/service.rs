//! Long-lived task feeding the calibration session and the cursor's live
//! measurements. Runs whatever `autoplay.enabled` says: calibration is the
//! user playing with autoplay off.

use super::{majsoul_gate, CalibrationStatus, LiveRate, SharedSession};
use crate::autoplay::AutoplayContext;
use crate::config::{AppConfig, Platform};
use crate::event_bus::MjaiBus;
use crate::schema::mjai::GameEndReason;
use crate::schema::MjaiEvent;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio::sync::RwLock;
use tracing::warn;

/// How often our decision window is sampled for the phase split.
const PHASE_POLL: Duration = Duration::from_millis(50);
/// Weight of the newest kyoku in the smoothed frame ratio.
const RATIO_SMOOTHING: f64 = 0.3;

pub struct Service {
    pub cfg: Arc<RwLock<AppConfig>>,
    pub ctx: Arc<AutoplayContext>,
    pub mjai_bus: MjaiBus,
    pub session: SharedSession,
    pub status_bus: broadcast::Sender<CalibrationStatus>,
}

pub fn now_ms() -> f64 {
    chrono::Local::now().timestamp_millis() as f64
}

/// Push the session's state to everyone who shows or obeys it.
pub fn publish(
    session: &SharedSession,
    ctx: &AutoplayContext,
    status_bus: &broadcast::Sender<CalibrationStatus>,
) -> CalibrationStatus {
    let status = session.lock().map(|s| s.status()).unwrap_or_default();
    ctx.mouse.set_calibrating(status.active);
    let _ = status_bus.send(status.clone());
    status
}

impl Service {
    pub async fn run(self) {
        let mut mjai_rx = self.mjai_bus.subscribe();
        let mut input_rx = self.ctx.mouse.input.subscribe();
        let mut report_rx = self.ctx.mouse.reports.subscribe();
        let mut tick = tokio::time::interval(PHASE_POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut ratio: Option<f64> = None;
        publish(&self.session, &self.ctx, &self.status_bus);

        loop {
            tokio::select! {
                ev = mjai_rx.recv() => match ev {
                    Ok(ev) => self.on_mjai(&ev, ratio),
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => return,
                },
                batch = input_rx.recv() => match batch {
                    Ok(b) => {
                        if let Ok(mut s) = self.session.lock() {
                            s.on_input(&b);
                        }
                        self.push_live(ratio);
                    }
                    Err(RecvError::Lagged(n)) => warn!("mouse: input lagged {n}"),
                    Err(RecvError::Closed) => return,
                },
                report = report_rx.recv() => match report {
                    Ok(r) => {
                        let autoplay = self.autoplay_running().await;
                        self.ctx.mouse.set_reported_click_mode(r.m);
                        let got = self.session.lock().ok().and_then(|mut s| {
                            let got = s.on_report(&r, autoplay);
                            if let Err(e) = s.save() {
                                warn!("mouse calibration: could not save state: {e}");
                            }
                            got
                        });
                        if let Some(g) = got {
                            ratio = Some(ratio.map_or(g, |p| p + RATIO_SMOOTHING * (g - p)));
                        }
                        self.push_live(ratio);
                        publish(&self.session, &self.ctx, &self.status_bus);
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => return,
                },
                _ = tick.tick() => {
                    let own = self.ctx.time_budget.read().ok().and_then(|g| *g).is_some();
                    if let Ok(mut s) = self.session.lock() {
                        s.on_phase(own, now_ms());
                    }
                }
            }
        }
    }

    fn on_mjai(&self, ev: &MjaiEvent, ratio: Option<f64>) {
        let changed = {
            let Ok(mut s) = self.session.lock() else {
                return;
            };
            match ev {
                MjaiEvent::StartGame { game_meta, .. } => {
                    let meta = game_meta.as_ref();
                    s.on_start_game(
                        meta.and_then(|m| m.game_id),
                        meta.and_then(|m| m.match_mode),
                    );
                    true
                }
                MjaiEvent::StartKyoku {
                    bakaze,
                    kyoku,
                    honba,
                    ..
                } => {
                    s.on_start_kyoku(bakaze, *kyoku, *honba, now_ms());
                    true
                }
                MjaiEvent::EndGame { reason, .. } => {
                    s.on_end_game(matches!(reason, GameEndReason::Confirmed));
                    if let Err(e) = s.save() {
                        warn!("mouse calibration: could not save state: {e}");
                    }
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.push_live(ratio);
            publish(&self.session, &self.ctx, &self.status_bus);
        }
    }

    fn push_live(&self, ratio: Option<f64>) {
        let counts = self.session.lock().ok().and_then(|s| s.live());
        self.ctx.mouse.live.send_replace(LiveRate {
            in_kyoku: counts.is_some(),
            started_ms: counts.map_or(0.0, |c| c.started_ms),
            page_frames: counts.map_or(0, |c| c.page_frames),
            page_moving: counts.map_or(0, |c| c.page_moving),
            frame_ratio: ratio,
        });
    }

    /// Our cursor is the one playing.
    async fn autoplay_running(&self) -> bool {
        let cfg = self.cfg.read().await;
        cfg.autoplay.enabled
            && cfg.platform.kind == Platform::Majsoul
            && majsoul_gate(&cfg.autoplay.majsoul.mouse, self.ctx.mouse.calibrating()).is_ok()
    }
}
