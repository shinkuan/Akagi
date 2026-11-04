//! Per-game `MjaiEvent` buffer + finalisation on `EndGame`.
//!
//! `drive_loop` subscribes to the shared `MjaiBus`, accumulates events
//! into an internal buffer, and on `EndGame` runs the aggregator and
//! writes the result via `HistoryStore`. A `StartGame` always resets
//! the buffer first — defensive in case the previous game ended without
//! `EndGame` (mid-session disconnect, app restart mid-game, etc.). Any
//! buffer that never sees `EndGame` is silently dropped on the next
//! `StartGame` or on shutdown — that is the contract for "complete game
//! only" recording.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use tokio::sync::broadcast::{error::RecvError, Receiver};
use tracing::{info, warn};
use ulid::Ulid;

use crate::event_bus::HistoryBus;
use crate::history::aggregator::{aggregate, AggregateInput};
use crate::history::store::HistoryStore;
use crate::schema::{GameEndReason, HistoryEvent, MjaiEvent, Platform};

/// Shared cell holding the platform tag every newly-finalised `GameRecord`
/// is stamped with. `update_config` writes here when the user switches
/// bridges so subsequent games persist with the correct platform without
/// requiring an app relaunch. Reads are sync (cheap, non-blocking) because
/// the recorder finalises inside a Tokio task.
pub type SharedPlatform = Arc<RwLock<Platform>>;

/// Convenience constructor for [`SharedPlatform`].
pub fn shared_platform(initial: Platform) -> SharedPlatform {
    Arc::new(RwLock::new(initial))
}

/// Hard cap on per-game buffer size. Defends against runaway streams
/// (a normal hanchan is ~1500 events; tonpuu ~600). On overflow the
/// buffer is cleared and the game is forfeited from history.
const MAX_EVENTS_PER_GAME: usize = 5_000;

/// Subscribe to `mjai_rx` and finalise complete games into `store`.
/// Drops out cleanly when the broadcast channel is closed.
pub async fn drive_loop(
    store: Arc<HistoryStore>,
    history_bus: HistoryBus,
    platform: SharedPlatform,
    mut mjai_rx: Receiver<MjaiEvent>,
) {
    let mut state = RecorderState::new(platform);
    loop {
        match mjai_rx.recv().await {
            Ok(ev) => state.handle(ev, &store, &history_bus),
            Err(RecvError::Lagged(n)) => {
                // Lagged consumers must reset — we've lost events that
                // belonged to the in-flight game.
                warn!(
                    target: "akagi::history",
                    "history recorder lagged by {n}; dropping in-flight buffer"
                );
                state.reset();
            }
            Err(RecvError::Closed) => {
                info!(target: "akagi::history", "history recorder shutting down");
                return;
            }
        }
    }
}

struct RecorderState {
    /// Read at finalise time so a runtime platform change picks up on the
    /// next completed game. Cloned out under a sync read lock to avoid
    /// holding the lock across the aggregator call.
    platform: SharedPlatform,
    /// In-flight buffer; `None` until the first `start_game` arrives.
    buf: Option<Vec<MjaiEvent>>,
    /// Wall-clock time of the active buffer's first event.
    started_at: Option<DateTime<Utc>>,
    /// True once a `start_game` has been seen for the current buffer.
    /// We refuse to finalise a buffer that never had a `start_game`.
    has_start: bool,
    /// True once the buffer has overflown; subsequent events are
    /// ignored until the next `start_game`.
    overflown: bool,
    /// Stable Mahjong Soul table identity. `None` for other platforms and old
    /// payloads, which retain the existing reset-on-StartGame behavior.
    game_id: Option<u64>,
    /// A duplicate StartGame from reconnect was consumed; the next restored
    /// StartKyoku replaces the previous copy of that round.
    awaiting_restore_start: bool,
}

impl RecorderState {
    fn new(platform: SharedPlatform) -> Self {
        Self {
            platform,
            buf: None,
            started_at: None,
            has_start: false,
            overflown: false,
            game_id: None,
            awaiting_restore_start: false,
        }
    }

    fn reset(&mut self) {
        self.buf = None;
        self.started_at = None;
        self.has_start = false;
        self.overflown = false;
        self.game_id = None;
        self.awaiting_restore_start = false;
    }

    fn handle(&mut self, ev: MjaiEvent, store: &HistoryStore, bus: &HistoryBus) {
        match &ev {
            MjaiEvent::StartGame { game_meta, .. } => {
                let incoming_game_id = game_meta.as_ref().and_then(|meta| meta.game_id);
                let reconnecting_same_game = incoming_game_id.is_some()
                    && incoming_game_id == self.game_id
                    && self.buf.is_some()
                    && !self.overflown;
                if reconnecting_same_game {
                    self.awaiting_restore_start = true;
                    info!(
                        target: "akagi::history",
                        "same Mahjong Soul table reconnected; retaining completed rounds"
                    );
                    return;
                }
                self.buf = Some(Vec::with_capacity(1024));
                self.started_at = Some(Utc::now());
                self.has_start = true;
                self.overflown = false;
                self.game_id = incoming_game_id;
                self.awaiting_restore_start = false;
                self.push(ev);
            }
            MjaiEvent::EndGame { reason, .. } => match reason {
                GameEndReason::Confirmed => {
                    self.push(ev);
                    self.finalise(store, bus);
                    self.reset();
                }
                GameEndReason::Terminated => self.reset(),
            },
            MjaiEvent::StartKyoku { .. } if self.awaiting_restore_start => {
                self.reconcile_restored_round(&ev);
                self.awaiting_restore_start = false;
                self.push(ev);
            }
            _ => self.push(ev),
        }
    }

    fn reconcile_restored_round(&mut self, incoming: &MjaiEvent) {
        let Some(buf) = &mut self.buf else { return };
        let incoming_key = round_key(incoming);
        let last_start = buf
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, event)| round_key(event).map(|key| (index, key)));
        let truncate_at = match (last_start, incoming_key) {
            (Some((index, old_key)), Some(new_key)) if old_key == new_key => index,
            _ => buf
                .iter()
                .rposition(|event| matches!(event, MjaiEvent::EndKyoku))
                .map_or(1, |index| index + 1),
        };
        buf.truncate(truncate_at.min(buf.len()));
    }

    fn push(&mut self, ev: MjaiEvent) {
        if self.overflown {
            return;
        }
        let Some(buf) = &mut self.buf else { return };
        if buf.len() >= MAX_EVENTS_PER_GAME {
            warn!(
                target: "akagi::history",
                "buffer exceeded {MAX_EVENTS_PER_GAME} events; abandoning game"
            );
            self.overflown = true;
            buf.clear();
            return;
        }
        buf.push(ev);
    }

    fn finalise(&mut self, store: &HistoryStore, bus: &HistoryBus) {
        if !self.has_start || self.overflown {
            return;
        }
        let Some(events) = self.buf.take() else {
            return;
        };
        let started_at = self.started_at.unwrap_or_else(Utc::now);
        let ended_at = Utc::now();
        let id = Ulid::new().to_string();

        let platform = *self
            .platform
            .read()
            .expect("history platform lock poisoned");
        let Some(record) = aggregate(AggregateInput {
            events: &events,
            platform,
            started_at,
            ended_at,
            id: id.clone(),
        }) else {
            warn!(
                target: "akagi::history",
                "aggregator rejected buffer (missing start_game?); dropping"
            );
            return;
        };

        if let Err(e) = store.append(&record, &events) {
            warn!(
                target: "akagi::history",
                "failed to persist GameRecord {id}: {e:#}"
            );
            return;
        }

        info!(
            target: "akagi::history",
            "recorded game {id} (rank {:?}, Δ{:?})",
            record.our_rank,
            record.our_delta
        );

        // Best-effort emit; if no subscribers, broadcast::send returns
        // Err but we don't care.
        let _ = bus.send(HistoryEvent::Recorded {
            record: Box::new(record),
        });
    }
}

fn round_key(event: &MjaiEvent) -> Option<(&str, u8, u8, u8)> {
    match event {
        MjaiEvent::StartKyoku {
            bakaze,
            kyoku,
            honba,
            oya,
            ..
        } => Some((bakaze.as_str(), *kyoku, *honba, *oya)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::{HistoryBus, DEFAULT_CAPACITY};
    use crate::schema::{HistoryFilter, MjaiEvent};
    use tempfile::TempDir;
    use tokio::sync::broadcast;

    fn start_game() -> MjaiEvent {
        MjaiEvent::StartGame {
            names: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            kyoku_first: Some(0),
            aka_flag: Some(true),
            id: Some(0),
            num_players: 4,
            game_meta: None,
        }
    }

    fn start_game_with_id(game_id: u64) -> MjaiEvent {
        let mut event = start_game();
        if let MjaiEvent::StartGame { game_meta, .. } = &mut event {
            *game_meta = Some(crate::schema::GameMeta {
                game_id: Some(game_id),
                match_mode: Some(2),
                match_info: None,
            });
        }
        event
    }

    fn start_kyoku() -> MjaiEvent {
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "1m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000, 25000, 25000, 25000],
            tehais: vec![vec![]; 4],
            num_players: 4,
            catch_up: 0,
        }
    }

    fn start_kyoku_at(kyoku: u8, scores: Vec<i32>) -> MjaiEvent {
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "1m".into(),
            kyoku,
            honba: 0,
            kyotaku: 0,
            oya: (kyoku - 1) % 4,
            scores,
            tehais: vec![vec![]; 4],
            num_players: 4,
            catch_up: 0,
        }
    }

    #[tokio::test]
    async fn complete_game_writes_record() {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(HistoryStore::new(tmp.path().to_path_buf()).unwrap());
        let (tx, rx) = broadcast::channel::<MjaiEvent>(DEFAULT_CAPACITY);
        let (history_tx, _history_rx): (HistoryBus, _) = broadcast::channel(8);

        let store_clone = store.clone();
        let bus_clone = history_tx.clone();
        let handle = tokio::spawn(async move {
            drive_loop(
                store_clone,
                bus_clone,
                shared_platform(Platform::Majsoul),
                rx,
            )
            .await
        });

        tx.send(start_game()).unwrap();
        tx.send(start_kyoku()).unwrap();
        tx.send(MjaiEvent::Hora {
            actor: 0,
            target: 1,
            deltas: Some(vec![8000, -8000, 0, 0]),
            ura_markers: None,
        })
        .unwrap();
        tx.send(MjaiEvent::EndKyoku).unwrap();
        tx.send(MjaiEvent::end_game()).unwrap();

        // Give the loop a tick to drain.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let records = store.list(&HistoryFilter::default(), 100, 0).unwrap();
        assert_eq!(records.len(), 1, "exactly one record after end_game");
        assert_eq!(records[0].our_rank, Some(1));
        assert_eq!(records[0].our_delta, Some(8000));

        drop(tx);
        let _ = handle.await;
    }

    #[tokio::test]
    async fn disconnect_without_end_game_drops_buffer() {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(HistoryStore::new(tmp.path().to_path_buf()).unwrap());
        let (tx, rx) = broadcast::channel::<MjaiEvent>(DEFAULT_CAPACITY);
        let (history_tx, _history_rx): (HistoryBus, _) = broadcast::channel(8);

        let store_clone = store.clone();
        let bus_clone = history_tx.clone();
        let handle = tokio::spawn(async move {
            drive_loop(
                store_clone,
                bus_clone,
                shared_platform(Platform::Majsoul),
                rx,
            )
            .await
        });

        tx.send(start_game()).unwrap();
        tx.send(start_kyoku()).unwrap();
        tx.send(MjaiEvent::Hora {
            actor: 0,
            target: 1,
            deltas: Some(vec![8000, -8000, 0, 0]),
            ura_markers: None,
        })
        .unwrap();
        // Channel closes without an EndGame.
        drop(tx);
        let _ = handle.await;

        let records = store.list(&HistoryFilter::default(), 100, 0).unwrap();
        assert!(records.is_empty(), "no record without end_game");
    }

    #[tokio::test]
    async fn second_start_game_resets_buffer() {
        let tmp = TempDir::new().unwrap();
        let store = Arc::new(HistoryStore::new(tmp.path().to_path_buf()).unwrap());
        let (tx, rx) = broadcast::channel::<MjaiEvent>(DEFAULT_CAPACITY);
        let (history_tx, _history_rx): (HistoryBus, _) = broadcast::channel(8);

        let store_clone = store.clone();
        let bus_clone = history_tx.clone();
        let handle = tokio::spawn(async move {
            drive_loop(
                store_clone,
                bus_clone,
                shared_platform(Platform::Majsoul),
                rx,
            )
            .await
        });

        // First game starts but never ends.
        tx.send(start_game()).unwrap();
        tx.send(start_kyoku()).unwrap();

        // Second game starts cleanly and ends.
        tx.send(start_game()).unwrap();
        tx.send(start_kyoku()).unwrap();
        tx.send(MjaiEvent::Hora {
            actor: 0,
            target: 1,
            deltas: Some(vec![8000, -8000, 0, 0]),
            ura_markers: None,
        })
        .unwrap();
        tx.send(MjaiEvent::end_game()).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drop(tx);
        let _ = handle.await;

        let records = store.list(&HistoryFilter::default(), 100, 0).unwrap();
        assert_eq!(records.len(), 1, "only the cleanly-ended second game");
    }

    #[test]
    fn same_game_reconnect_replaces_only_the_restored_round() {
        let tmp = TempDir::new().unwrap();
        let store = HistoryStore::new(tmp.path().to_path_buf()).unwrap();
        let (history_tx, _history_rx): (HistoryBus, _) = broadcast::channel(8);
        let mut state = RecorderState::new(shared_platform(Platform::Majsoul));

        state.handle(start_game_with_id(7), &store, &history_tx);
        state.handle(start_kyoku_at(1, vec![25000; 4]), &store, &history_tx);
        state.handle(
            MjaiEvent::Hora {
                actor: 0,
                target: 1,
                deltas: Some(vec![8000, -8000, 0, 0]),
                ura_markers: None,
            },
            &store,
            &history_tx,
        );
        state.handle(MjaiEvent::EndKyoku, &store, &history_tx);
        state.handle(
            start_kyoku_at(2, vec![33000, 17000, 25000, 25000]),
            &store,
            &history_tx,
        );
        state.handle(
            MjaiEvent::Dahai {
                actor: 0,
                pai: "9m".into(),
                tsumogiri: true,
            },
            &store,
            &history_tx,
        );

        // Re-authentication repeats StartGame and GameRestore repeats the
        // current round from StartKyoku.
        state.handle(start_game_with_id(7), &store, &history_tx);
        state.handle(
            start_kyoku_at(2, vec![33000, 17000, 25000, 25000]),
            &store,
            &history_tx,
        );
        state.handle(
            MjaiEvent::Ryukyoku {
                deltas: Some(vec![0, 0, 0, 0]),
            },
            &store,
            &history_tx,
        );
        state.handle(MjaiEvent::EndKyoku, &store, &history_tx);
        state.handle(
            MjaiEvent::confirmed_game(
                Some(vec![12000, 41000, 27000, 20000]),
                Some(vec![4, 1, 2, 3]),
            ),
            &store,
            &history_tx,
        );

        let records = store.list(&HistoryFilter::default(), 100, 0).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].stats.round, 2, "restored round counted once");
        assert_eq!(records[0].final_scores, vec![12000, 41000, 27000, 20000]);
        let events = store.get_events(&records[0].id).unwrap().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, MjaiEvent::StartKyoku { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn explicit_termination_drops_unfinished_game() {
        let tmp = TempDir::new().unwrap();
        let store = HistoryStore::new(tmp.path().to_path_buf()).unwrap();
        let (history_tx, _history_rx): (HistoryBus, _) = broadcast::channel(8);
        let mut state = RecorderState::new(shared_platform(Platform::Majsoul));
        state.handle(start_game_with_id(7), &store, &history_tx);
        state.handle(start_kyoku(), &store, &history_tx);
        state.handle(MjaiEvent::terminated_game(), &store, &history_tx);
        assert!(store
            .list(&HistoryFilter::default(), 100, 0)
            .unwrap()
            .is_empty());
    }
}
