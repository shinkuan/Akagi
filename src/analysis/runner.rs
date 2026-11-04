//! Subscribe to the post-tracker bus, snapshot the game state, run the
//! analysis engine for the active player's seat, and broadcast the result.
//!
//! Triggered after every `MjaiEvent` the tracker has digested. Re-runs
//! the full analysis whether the event affected our hand or not — the
//! per-event cost (~200 µs in release for a 13-tile state, up to ~1 ms
//! for a 14-tile discard search) is comfortably below the IPC latency
//! budget. See `tests/analysis_bench.rs` for the figures.

use std::sync::Arc;

use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{debug, info, warn};

use super::result::AnalysisResult;
use super::snapshot_adapter::to_player_info;
use crate::event_bus::{AnalysisBus, TrackedEvent};
use crate::game_state::tracker::GameTracker;

/// Cache of the latest analysis output. The IPC `get_analysis` command reads
/// this for one-shot queries; new tabs / panes opened mid-game can pull the
/// freshest snapshot without waiting for the next `analysis-result` event.
pub type AnalysisCache = Arc<RwLock<Option<AnalysisResult>>>;

/// Spawn the analysis runner task. Must be called from within a Tokio
/// runtime context.
pub fn spawn(
    mut rx: broadcast::Receiver<TrackedEvent>,
    tracker: Arc<Mutex<GameTracker>>,
    bus: AnalysisBus,
    cache: AnalysisCache,
) {
    tokio::spawn(async move {
        run(&mut rx, tracker, bus, cache).await;
    });
}

/// Drive the analysis loop on the current task. Returns when the
/// post-tracker bus closes. Use this when you want to spawn the loop on
/// a runtime that isn't accessible at construction time.
pub async fn drive_loop(
    mut rx: broadcast::Receiver<TrackedEvent>,
    tracker: Arc<Mutex<GameTracker>>,
    bus: AnalysisBus,
    cache: AnalysisCache,
) {
    run(&mut rx, tracker, bus, cache).await
}

async fn run(
    rx: &mut broadcast::Receiver<TrackedEvent>,
    tracker: Arc<Mutex<GameTracker>>,
    bus: AnalysisBus,
    cache: AnalysisCache,
) {
    info!("analysis runner subscribed to post-tracker bus");
    loop {
        match rx.recv().await {
            Ok(_ev) => {
                // Snapshot the game state. Tracker has already digested the
                // event (post-tracker bus ordering) and captured our seat
                // from any `start_game.id`.
                let snap = {
                    let t = tracker.lock().await;
                    t.snapshot()
                };
                let Some(snap) = snap else { continue };
                let Some(seat) = snap.our_seat else { continue };

                // Build PlayerInfo34. Some snapshot states are intermediate
                // (e.g. a 14-tile state immediately after we drew but before
                // anyone reacted) — those are fine. We only skip if the seat
                // is somehow invalid or the tehai parse fails.
                let info = match to_player_info(&snap, seat) {
                    Ok(i) => i,
                    Err(e) => {
                        debug!("analysis: snapshot adapter rejected event: {e:#}");
                        continue;
                    }
                };

                // Skip if our hand is in a transient state (post-call before
                // discard, etc) — analysis_engine asserts 13 or 14 closed tiles.
                let size = info.hand_size();
                if size != 13
                    && size != 14
                    && size != 10
                    && size != 11
                    && size != 7
                    && size != 8
                    && size != 4
                    && size != 5
                {
                    debug!("analysis: skipping hand_size={size}");
                    continue;
                }

                let result = super::analyze(&info);
                {
                    let mut c = cache.write().await;
                    *c = Some(result.clone());
                }
                if let Err(e) = bus.send(result) {
                    debug!("analysis bus: no subscribers: {e}");
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("analysis runner lagged by {n} events");
            }
            Err(broadcast::error::RecvError::Closed) => {
                info!("post-tracker bus closed; analysis runner exiting");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::{analysis_bus, mjai_bus, post_tracker_bus};
    use crate::game_state::tracker::spawn_with_post;
    use crate::schema::MjaiEvent;

    fn start_game() -> MjaiEvent {
        MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 4,
            game_meta: None,
        }
    }

    fn start_kyoku() -> MjaiEvent {
        let one_hand: Vec<String> = vec![
            "1m".into(),
            "2m".into(),
            "3m".into(),
            "4m".into(),
            "5m".into(),
            "6m".into(),
            "7m".into(),
            "8m".into(),
            "9m".into(),
            "1p".into(),
            "2p".into(),
            "3p".into(),
            "1s".into(),
        ];
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "9p".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25_000, 25_000, 25_000, 25_000],
            tehais: vec![
                one_hand.clone(),
                one_hand.clone(),
                one_hand.clone(),
                one_hand,
            ],
            num_players: 4,
            catch_up: 0,
        }
    }

    #[tokio::test]
    async fn runner_emits_analysis_after_start_kyoku() {
        let mjai = mjai_bus();
        let post = post_tracker_bus();
        let bus = analysis_bus();
        let cache: AnalysisCache = Arc::new(RwLock::new(None));

        let _tracker = spawn_with_post(mjai.subscribe(), Some(post.clone()));
        spawn(
            post.subscribe(),
            _tracker.clone(),
            bus.clone(),
            cache.clone(),
        );

        let mut rx = bus.subscribe();
        mjai.send(start_game()).unwrap();
        mjai.send(start_kyoku()).unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
            .await
            .expect("timed out waiting for analysis-result")
            .expect("recv");

        assert_eq!(result.seat, 0);
        // 13-tile drawing state at start_kyoku.
        assert_eq!(result.shanten, 0);
        // Cache should be populated.
        assert!(cache.read().await.is_some());
    }
}
