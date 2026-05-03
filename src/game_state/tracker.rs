//! `GameTracker` — observer-mode wrapper around `riichienv_core::state::GameState`.
//!
//! Subscribes to [`crate::event_bus::MjaiBus`], translates each
//! `schema::MjaiEvent` into a `riichienv` event, and feeds it through
//! `apply_mjai_event` so the engine maintains a live snapshot of the
//! game (hands, melds, river, scores, dora indicators, phase).
//!
//! # Lifecycle
//!
//! - On the first `StartGame`, a fresh `GameState` is constructed (the
//!   constructor calls `_initialize_round(0, round_wind=0, 0, 0, ...)`,
//!   so we get East 1 set up by default).
//! - On every subsequent `StartGame`, we drop and reconstruct — full
//!   reset, since `apply_mjai_event(StartGame)` only clears legal-action
//!   stale state and not scores/honba.
//! - All other events go through `apply_mjai_event`, which handles
//!   `StartKyoku` (round reset), tile draws/discards, melds, and round
//!   end.
//! - `MjaiEvent::None` (Akagi-only sentinel for bot replies) is skipped
//!   silently in `convert::to_riichienv`.
//!
//! # Concurrent access
//!
//! `spawn` returns an `Arc<Mutex<GameTracker>>` so future IPC commands
//! can pull a snapshot without going through a separate bus. The IPC
//! layer is intentionally not wired in this round — the tracker is
//! ready to be exposed when the frontend needs it.

use crate::game_state::convert;
use crate::game_state::score::{evaluate_hora_3p, evaluate_hora_4p};
use crate::game_state::snapshot::GameStateSnapshot;
use crate::schema::{HoraScoreInfo, MjaiEvent as AkagiEvent};
use anyhow::Result;
use riichienv_core::rule::GameRule;
use riichienv_core::state::GameState;
use riichienv_core::state_3p::GameState3P;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn};

/// Engine state, varying by player count. Both variants accept the same
/// `riichienv_core::replay::MjaiEvent` produced by `convert::to_riichienv`,
/// so the dispatch surface is just `match self.state`.
///
/// Variants differ in size (~1744 vs ~1400 bytes); boxing the larger
/// variant would shave ~300 bytes off the enum but force every match
/// arm + helper API to deref through `Box`. There is at most one
/// `TrackedGame` per `GameTracker` and one `GameTracker` per process,
/// so the saving is not worth the call-site churn.
#[allow(clippy::large_enum_variant)]
pub enum TrackedGame {
    Four(GameState),
    Three(GameState3P),
}

impl TrackedGame {
    pub fn num_players(&self) -> u8 {
        match self {
            TrackedGame::Four(_) => 4,
            TrackedGame::Three(_) => 3,
        }
    }
}

pub struct GameTracker {
    state: Option<TrackedGame>,
    rule: GameRule,
    /// The bot's own seat, captured from `start_game.id`.
    our_seat: Option<u8>,
    /// Total events fed since process start. Useful for "is the bridge
    /// alive?" checks; not reset on game boundaries.
    pub events_seen: u64,
}

impl GameTracker {
    pub fn new() -> Self {
        Self {
            state: None,
            rule: GameRule::default_tenhou(),
            our_seat: None,
            events_seen: 0,
        }
    }

    /// Drive one event through the engine. `Ok(())` even when the event
    /// is a no-op (e.g. `MjaiEvent::None`) — the only error path is a
    /// JSON conversion failure, which means a malformed event.
    pub fn handle(&mut self, ev: &AkagiEvent) -> Result<()> {
        self.events_seen += 1;

        if let AkagiEvent::StartGame {
            id, num_players, ..
        } = ev
        {
            // Fresh game → fresh state. Constructor seeds round 0 with the
            // mode-appropriate starting score (25k for 4p, 35k for 3p).
            self.state = Some(match *num_players {
                3 => TrackedGame::Three(GameState3P::new(0, true, None, 0, self.rule)),
                4 => TrackedGame::Four(GameState::new(0, true, None, 0, self.rule)),
                other => {
                    warn!(
                        "tracker: unexpected num_players={other} on start_game; defaulting to 4p"
                    );
                    TrackedGame::Four(GameState::new(0, true, None, 0, self.rule))
                }
            });
            // Each new game may put us in a different seat (or none, in
            // observer/replay mode). ALWAYS replace — never inherit stale
            // perspective from the previous game.
            self.our_seat = *id;
        }

        // riichienv-core's `apply_mjai_event(Dahai)` pushes the tile onto
        // `discards` but leaves the parallel `discard_from_hand` /
        // `discard_is_riichi` arrays empty. Capture the bits we need
        // pre-apply so we can patch them on after.
        let dahai_patch = if let AkagiEvent::Dahai {
            actor, tsumogiri, ..
        } = ev
        {
            self.state.as_ref().map(|tg| {
                let actor = *actor as usize;
                let riichi_stage = match tg {
                    TrackedGame::Four(s) => s.players[actor].riichi_stage,
                    TrackedGame::Three(s) => s.players[actor].riichi_stage,
                };
                (actor, !*tsumogiri, riichi_stage)
            })
        } else {
            None
        };

        let Some(ri) = convert::to_riichienv(ev)? else {
            return Ok(()); // Skipped (e.g. MjaiEvent::None).
        };

        if let Some(tg) = self.state.as_mut() {
            match tg {
                TrackedGame::Four(s) => {
                    s.apply_mjai_event(ri);
                    if let Some((actor, tedashi, was_riichi_commit)) = dahai_patch {
                        let p = &mut s.players[actor];
                        let n = p.discards.len();
                        if p.discard_from_hand.len() < n {
                            p.discard_from_hand.push(tedashi);
                        }
                        if p.discard_is_riichi.len() < n {
                            p.discard_is_riichi.push(was_riichi_commit);
                        }
                        if was_riichi_commit && p.riichi_declaration_index.is_none() {
                            p.riichi_declaration_index = Some(n - 1);
                        }
                    }
                }
                TrackedGame::Three(s) => {
                    s.apply_mjai_event(ri);
                    if let Some((actor, tedashi, was_riichi_commit)) = dahai_patch {
                        let p = &mut s.players[actor];
                        let n = p.discards.len();
                        if p.discard_from_hand.len() < n {
                            p.discard_from_hand.push(tedashi);
                        }
                        if p.discard_is_riichi.len() < n {
                            p.discard_is_riichi.push(was_riichi_commit);
                        }
                        if was_riichi_commit && p.riichi_declaration_index.is_none() {
                            p.riichi_declaration_index = Some(n - 1);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Snapshot of the current state. Returns `None` if no game has
    /// started yet.
    pub fn snapshot(&self) -> Option<GameStateSnapshot> {
        self.state.as_ref().map(|tg| match tg {
            TrackedGame::Four(s) => GameStateSnapshot::from_state(s, self.our_seat),
            TrackedGame::Three(s) => GameStateSnapshot::from_state_3p(s, self.our_seat),
        })
    }

    /// The captured observer seat, or `None` if no `start_game.id` arrived.
    pub fn our_seat(&self) -> Option<u8> {
        self.our_seat
    }

    /// `Some(num_players)` if a game is in progress.
    pub fn num_players(&self) -> Option<u8> {
        self.state.as_ref().map(|tg| tg.num_players())
    }

    /// Score a hypothetical hora by `actor` against the live engine state.
    /// Returns `None` when no game is in progress, the hand isn't a winning
    /// shape, or the winning tile can't be inferred (no recent draw / discard).
    /// Routes to the 4p or 3p evaluator based on the active engine.
    pub fn evaluate_hora(&self, actor: u8, is_tsumo: bool) -> Option<HoraScoreInfo> {
        match &self.state {
            Some(TrackedGame::Four(s)) => evaluate_hora_4p(s, actor, is_tsumo),
            Some(TrackedGame::Three(s)) => evaluate_hora_3p(s, actor, is_tsumo),
            None => None,
        }
    }

    /// Borrow the live engine state. Returns `None` for non-4p games or no
    /// game in progress. Callers needing 3p access can use `state_3p()`.
    pub fn state(&self) -> Option<&GameState> {
        match &self.state {
            Some(TrackedGame::Four(s)) => Some(s),
            _ => None,
        }
    }

    /// Borrow the live 3p engine state if the current game is sanma.
    pub fn state_3p(&self) -> Option<&GameState3P> {
        match &self.state {
            Some(TrackedGame::Three(s)) => Some(s),
            _ => None,
        }
    }
}

impl Default for GameTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an empty tracker handle without spawning a task. Caller is
/// responsible for driving [`drive_loop`] on a runtime.
pub fn new_handle() -> Arc<Mutex<GameTracker>> {
    Arc::new(Mutex::new(GameTracker::new()))
}

/// Spawn a tracker task that consumes the given MJAI receiver. Returns
/// a shared handle for snapshot access. Must be called from within a
/// Tokio runtime context.
///
/// The task ends cleanly when the broadcast channel closes (all
/// `MjaiBus` senders dropped).
pub fn spawn(rx: broadcast::Receiver<AkagiEvent>) -> Arc<Mutex<GameTracker>> {
    spawn_with_post(rx, None)
}

/// Like [`spawn`] but also re-emits each consumed `AkagiEvent` on `post`
/// **after** the tracker has applied it. Subscribers to `post` can rely on
/// the tracker snapshot being current when they receive an event.
pub fn spawn_with_post(
    rx: broadcast::Receiver<AkagiEvent>,
    post: Option<broadcast::Sender<AkagiEvent>>,
) -> Arc<Mutex<GameTracker>> {
    let tracker = new_handle();
    let cloned = tracker.clone();
    tokio::spawn(async move { drive_loop(cloned, rx, post).await });
    tracker
}

/// Drive the tracker loop on the current task. Returns when the
/// broadcast channel closes. Use this when you want to spawn the loop
/// on a runtime that isn't accessible at construction time.
pub async fn drive_loop(
    tracker: Arc<Mutex<GameTracker>>,
    rx: broadcast::Receiver<AkagiEvent>,
    post: Option<broadcast::Sender<AkagiEvent>>,
) {
    run(tracker, rx, post).await
}

async fn run(
    tracker: Arc<Mutex<GameTracker>>,
    mut rx: broadcast::Receiver<AkagiEvent>,
    post: Option<broadcast::Sender<AkagiEvent>>,
) {
    info!("game tracker subscribed to MJAI bus");
    loop {
        match rx.recv().await {
            Ok(ev) => {
                {
                    let mut t = tracker.lock().await;
                    if let Err(e) = t.handle(&ev) {
                        warn!("game tracker: handle error: {e:#}");
                    }
                }
                if let Some(p) = &post {
                    // Receiver may have lagged or no-one subscribed yet — ignore.
                    let _ = p.send(ev);
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("game tracker lagged behind MJAI bus by {n} events");
            }
            Err(broadcast::error::RecvError::Closed) => {
                info!("MJAI bus closed; game tracker exiting");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::mjai_bus;

    fn start_game() -> AkagiEvent {
        AkagiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 4,
        }
    }

    fn start_kyoku(oya: u8) -> AkagiEvent {
        // 13 tiles per hand, garbage-but-parseable.
        let one_hand: Vec<String> = (0..13).map(|_| "1m".into()).collect();
        AkagiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya,
            scores: vec![25_000, 25_000, 25_000, 25_000],
            tehais: vec![
                one_hand.clone(),
                one_hand.clone(),
                one_hand.clone(),
                one_hand,
            ],
            num_players: 4,
        }
    }

    #[test]
    fn tracker_starts_empty() {
        let t = GameTracker::new();
        assert!(t.snapshot().is_none());
        assert!(t.state().is_none());
    }

    #[test]
    fn start_game_constructs_state() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        let snap = t.snapshot().expect("snapshot after start_game");
        assert_eq!(snap.players.len(), 4);
        assert_eq!(snap.bakaze, "E");
        // Constructor seeded each player with the rule's starting score.
        for p in &snap.players {
            assert!(p.score > 0);
        }
    }

    #[test]
    fn start_kyoku_propagates_oya_and_scores() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku(2)).unwrap();
        let snap = t.snapshot().unwrap();
        assert_eq!(snap.oya, 2);
        assert_eq!(snap.honba, 0);
        for p in &snap.players {
            assert_eq!(p.score, 25_000);
        }
    }

    #[test]
    fn none_event_is_skipped() {
        let mut t = GameTracker::new();
        // No state yet → handle(None) shouldn't panic or construct anything.
        t.handle(&AkagiEvent::None).unwrap();
        assert!(t.state().is_none());
    }

    #[test]
    fn second_start_game_resets_state() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku(3)).unwrap();
        let first = t.snapshot().unwrap();
        assert_eq!(first.oya, 3);

        // New game with default oya=0 from constructor.
        t.handle(&start_game()).unwrap();
        let second = t.snapshot().unwrap();
        assert_eq!(second.oya, 0, "fresh state defaults to oya=0");
    }

    fn start_game_with_seat(seat: Option<u8>) -> AkagiEvent {
        AkagiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: seat,
            num_players: 4,
        }
    }

    #[test]
    fn start_game_replaces_our_seat_each_time() {
        let mut t = GameTracker::new();
        // First game — seat 0.
        t.handle(&start_game_with_seat(Some(0))).unwrap();
        assert_eq!(t.our_seat(), Some(0));

        // Second game — seat 2 (different table position).
        t.handle(&start_game_with_seat(Some(2))).unwrap();
        assert_eq!(t.our_seat(), Some(2), "must adopt new seat");

        // Third game — observer/replay mode, no perspective tag.
        // Stale Some(2) MUST NOT carry over.
        t.handle(&start_game_with_seat(None)).unwrap();
        assert_eq!(
            t.our_seat(),
            None,
            "untagged start_game must clear stale seat"
        );

        // Fourth game — back to seat 1.
        t.handle(&start_game_with_seat(Some(1))).unwrap();
        assert_eq!(t.our_seat(), Some(1));
    }

    /// Regression: `riichienv-core 0.4.8::apply_mjai_event(Dahai)` does not
    /// populate `discard_from_hand` / `discard_is_riichi`, so the snapshot
    /// fell back to defaults and the mahgen river rendered with no
    /// tedashi/tsumogiri/riichi markers. We patch the parallel arrays
    /// inside `handle()` — verify the snapshot exposes correct flags.
    #[test]
    fn dahai_marker_arrays_stay_in_sync() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku(0)).unwrap();

        // Tsumogiri 1m (drew 1m, immediate cut).
        t.handle(&AkagiEvent::Tsumo {
            actor: 0,
            pai: "1m".into(),
        })
        .unwrap();
        t.handle(&AkagiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: true,
        })
        .unwrap();

        // Tedashi 1m (drew 2m, cut a 1m from hand).
        t.handle(&AkagiEvent::Tsumo {
            actor: 0,
            pai: "2m".into(),
        })
        .unwrap();
        t.handle(&AkagiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: false,
        })
        .unwrap();

        // Riichi declaration — Reach event then Dahai commits riichi.
        t.handle(&AkagiEvent::Tsumo {
            actor: 0,
            pai: "3m".into(),
        })
        .unwrap();
        t.handle(&AkagiEvent::Reach { actor: 0, pai: None }).unwrap();
        t.handle(&AkagiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: false,
        })
        .unwrap();

        let snap = t.snapshot().unwrap();
        let p0 = &snap.players[0];
        assert_eq!(p0.river.len(), 3, "three discards recorded");

        // 1: tsumogiri, no riichi
        assert!(!p0.river[0].tedashi);
        assert!(!p0.river[0].is_riichi);
        // 2: tedashi, no riichi
        assert!(p0.river[1].tedashi);
        assert!(!p0.river[1].is_riichi);
        // 3: tedashi + riichi commit
        assert!(p0.river[2].tedashi);
        assert!(p0.river[2].is_riichi);

        assert_eq!(p0.riichi_declaration_index, Some(2));
    }

    /// 3p `start_game` constructs a `GameState3P` and the snapshot reflects
    /// length-3 players + `num_players: 3`. Switching back to 4p replaces
    /// the engine cleanly.
    #[test]
    fn three_player_start_game_constructs_3p_state_and_snapshot_is_length_three() {
        let mut t = GameTracker::new();
        let ev = AkagiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(1),
            num_players: 3,
        };
        t.handle(&ev).unwrap();
        assert!(t.state().is_none(), "state() returns None for 3p");
        assert!(t.state_3p().is_some(), "state_3p() exposes the 3p engine");
        assert_eq!(t.num_players(), Some(3));
        assert_eq!(t.our_seat(), Some(1));
        let snap = t.snapshot().expect("3p snapshot");
        assert_eq!(snap.num_players, 3);
        assert_eq!(snap.players.len(), 3);
        // 3p starting score is 35000, not 25000.
        for p in &snap.players {
            assert_eq!(p.score, 35000);
            assert!(p.kita_tiles.is_empty(), "no kita declared yet");
        }

        // Switch back to 4p: state replaced cleanly.
        t.handle(&start_game_with_seat(Some(2))).unwrap();
        assert!(t.state().is_some());
        assert!(t.state_3p().is_none());
        assert_eq!(t.num_players(), Some(4));
    }

    #[test]
    fn events_seen_counter_advances() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku(0)).unwrap();
        t.handle(&AkagiEvent::None).unwrap();
        assert_eq!(t.events_seen, 3);
    }

    #[tokio::test]
    async fn spawn_consumes_bus_until_closed() {
        let bus = mjai_bus();
        let rx = bus.subscribe();
        let tracker = spawn(rx);

        bus.send(start_game()).unwrap();
        bus.send(start_kyoku(1)).unwrap();

        // Give the task a moment to drain.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let snap = tracker.lock().await.snapshot().expect("snapshot");
        assert_eq!(snap.oya, 1);

        // Drop the last sender → channel closes → task exits cleanly.
        drop(bus);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
