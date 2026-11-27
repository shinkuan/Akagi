//! Long-lived task feeding the session. Runs whatever `autoplay.enabled`
//! says: calibrating is the user playing with autoplay off.

use crate::autoplay::AutoplayContext;
use crate::event_bus::MjaiBus;
use crate::schema::{GameEndReason, MjaiEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tracing::warn;

/// How often our decision window is sampled for the phase split.
const PHASE_POLL: Duration = Duration::from_millis(50);

pub fn now_ms() -> f64 {
    chrono::Local::now().timestamp_millis() as f64
}

pub async fn run(ctx: Arc<AutoplayContext>, mjai_bus: MjaiBus) {
    let feed = &ctx.majsoul_mouse;
    let mut mjai_rx = mjai_bus.subscribe();
    let mut input_rx = feed.input.subscribe();
    let mut report_rx = feed.reports.subscribe();
    let mut tick = tokio::time::interval(PHASE_POLL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    feed.publish();

    loop {
        tokio::select! {
            ev = mjai_rx.recv() => match ev {
                Ok(ev) => {
                    let changed = feed.with(|s| match &ev {
                        MjaiEvent::StartGame { game_meta, .. } => {
                            let meta = game_meta.as_ref();
                            s.on_start_game(meta.and_then(|m| m.game_id), meta.and_then(|m| m.match_mode));
                            true
                        }
                        MjaiEvent::StartKyoku { bakaze, kyoku, honba, .. } => {
                            s.on_start_kyoku(bakaze, *kyoku, *honba, now_ms());
                            true
                        }
                        MjaiEvent::EndGame { reason, .. } => {
                            s.on_end_game(matches!(reason, GameEndReason::Confirmed));
                            save(s);
                            true
                        }
                        _ => false,
                    });
                    if changed {
                        feed.publish();
                    }
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            batch = input_rx.recv() => match batch {
                Ok(b) => feed.with(|s| s.on_input(&b)),
                Err(RecvError::Lagged(n)) => warn!("mouse: input lagged {n}"),
                Err(RecvError::Closed) => return,
            },
            report = report_rx.recv() => match report {
                Ok(r) => {
                    feed.with(|s| {
                        s.on_report(&r);
                        save(s);
                    });
                    feed.publish();
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            },
            _ = tick.tick() => {
                let own = ctx.time_budget.read().ok().and_then(|g| *g).is_some();
                feed.with(|s| s.on_phase(own, now_ms()));
            }
        }
    }
}

fn save(s: &super::Session) {
    if let Err(e) = s.save() {
        warn!("mouse calibration: could not save state: {e}");
    }
}
