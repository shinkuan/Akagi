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

use crate::event_bus::TrackedEvent;
use crate::game_state::convert;
use crate::game_state::score::{evaluate_hora_3p, evaluate_hora_4p};
use crate::game_state::snapshot::GameStateSnapshot;
use crate::schema::{HoraScoreInfo, MjaiEvent as AkagiEvent};
use anyhow::Result;
use riichienv_core::rule::GameRule;
use riichienv_core::state::legal_actions::GameStateLegalActions;
use riichienv_core::state::GameState;
use riichienv_core::state_3p::legal_actions::GameState3PLegalActions;
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

        // The replay path also opens no chankan window: an opponent's kakan
        // that our seat could rob must be turned into a real `WaitResponse`
        // window after it is applied, or every downstream consumer (`can_act`,
        // the bot's candidate set, autoplay's buttons) sees an empty legal set
        // and the window hangs (`native_bot::chankan` has the story).
        let opponent_kakan = match (ev, self.our_seat) {
            (AkagiEvent::Kakan { actor, pai, .. }, Some(seat)) if *actor != seat => {
                Some((*actor, pai.clone(), seat))
            }
            _ => None,
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
                    apply_ippatsu_patch_4p(s, ev);
                    // riichienv-core drops the naki `target`, storing the new
                    // meld with `from_who = -1`; patch it back so the meld
                    // renders rotated toward the real discarder (see
                    // `mahgen_view::call_side`).
                    if let Some((actor, target)) = meld_target(ev) {
                        if let Some(m) = s.players.get_mut(actor).and_then(|p| p.melds.last_mut()) {
                            m.from_who = target;
                        }
                    }
                    if let AkagiEvent::Dahai { actor, pai, .. } = ev {
                        open_haitei_claims(s, *actor, pai);
                    }
                    // Runs after `apply_ippatsu_patch_4p`, which has already
                    // retired every ippatsu window (a kakan is a call) — the
                    // live path checks chankan with ippatsu still up. Harmless
                    // for the window itself (chankan is a yaku, so han >= 1
                    // regardless); only `evaluate_hora`'s preview of an
                    // ippatsu-plus-chankan ron loses that han.
                    if let Some((actor, pai, seat)) = &opponent_kakan {
                        native_bot::chankan::open_on_kakan(s, *actor, pai, *seat);
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
                    apply_ippatsu_patch_3p(s, ev);
                    if let Some((actor, target)) = meld_target(ev) {
                        if let Some(m) = s.players.get_mut(actor).and_then(|p| p.melds.last_mut()) {
                            m.from_who = target;
                        }
                    }
                    if let AkagiEvent::Dahai { actor, pai, .. } = ev {
                        open_haitei_claims_3p(s, *actor, pai);
                    }
                    // Same ippatsu-ordering caveat as the 4p arm above.
                    if let Some((actor, pai, seat)) = &opponent_kakan {
                        native_bot::chankan::open_on_kakan_3p(s, *actor, pai, *seat);
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

    /// Does the engine owe our seat a decision right now?
    ///
    /// This is the question everything downstream of the bot is really
    /// asking. A bot is fed every event and answers every event, so its
    /// replies alone cannot say which ones it was *asked* — an mjai `none`
    /// means both "I decline this call" and "this was never mine to answer".
    /// The engine can tell them apart, so ask it here, once, at the point
    /// where the state matches the event.
    ///
    /// `None` when there is nothing to ask: no game in progress, or no seat
    /// tagged (observer / replay mode).
    pub fn our_seat_can_act(&self) -> Option<bool> {
        let seat = self.our_seat?;
        let legals = match &self.state {
            Some(TrackedGame::Four(s)) => s._get_legal_actions_internal(seat),
            Some(TrackedGame::Three(s)) => s._get_legal_actions_internal(seat),
            None => return None,
        };
        Some(is_decision(&legals))
    }
}

/// Whether a legal-action set is a choice our seat has to make.
///
/// Two sets are not. An empty one, obviously — the turn belongs to someone
/// else. And exactly `[Pass]`: the engine hands a `Pass` to every seat while
/// it is in its response phase, including the seats with nothing to claim,
/// so a lone pass is the engine saying "not yours" rather than offering a
/// decline.
///
/// Same rule the native bot applies to its own candidate set before it
/// touches the HUD card (`bot::native::is_decision_point`); the two are
/// deliberately the same test asked of the same engine at different points.
fn is_decision(legals: &[riichienv_core::action::Action]) -> bool {
    legals
        .iter()
        .any(|a| a.action_type != riichienv_core::action::ActionType::Pass)
}

impl Default for GameTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// `(actor, target)` for the open melds that claim another seat's discard
/// (pon / chi / daiminkan), as `(seat index, discarder seat)`. Returns `None`
/// for every other event, including ankan / kakan (no external claim).
///
/// riichienv-core 0.4.8's `apply_mjai_event` ignores the mjai `target` field
/// and stores these melds with `from_who = -1`, which `mahgen_view::call_side`
/// treats as a kamicha default — so every open meld would render rotated to the
/// left regardless of who was called. The tracker re-applies `target` after
/// the event so the meld points at the real discarder.
fn meld_target(ev: &AkagiEvent) -> Option<(usize, i8)> {
    match ev {
        AkagiEvent::Pon { actor, target, .. }
        | AkagiEvent::Chi { actor, target, .. }
        | AkagiEvent::Daiminkan { actor, target, .. } => Some((*actor as usize, *target as i8)),
        _ => None,
    }
}

/// riichienv-core 0.4.8 gates chi/pon/kan detection on
/// `wall.drawable_count > 0`, a stricter variant than Riichi City plays:
/// on a live 2026-08-24 capture the server offered a chi on the haitei
/// discard (the last live-wall tile) and the claim window timed out
/// because the engine's empty claim set made `can_act` false — the bot
/// was never asked. Re-run the engine's own claim detection with the
/// gate satisfied — restoring the wall immediately after, since the ron
/// arm already ran during apply with the true haitei conditions — and
/// splice only the chi/pon claims back in (kan at an empty live wall is
/// left to the engine's own judgment).
fn open_haitei_claims(s: &mut GameState, discarder: u8, pai: &str) {
    use riichienv_core::action::{ActionType, Phase};
    if s.wall.drawable_count != 0 || s.phase != Phase::WaitAct {
        return;
    }
    let Some(tile) = riichienv_core::parser::mjai_to_tid(pai) else {
        return;
    };
    s.wall.drawable_count = 1;
    let mut claimants = Vec::new();
    for i in 0..s.players.len() as u8 {
        if i == discarder {
            continue;
        }
        let (legals, _) = s._get_claim_actions_for_player(i, discarder, tile);
        let calls: Vec<_> = legals
            .into_iter()
            .filter(|a| matches!(a.action_type, ActionType::Chi | ActionType::Pon))
            .collect();
        if !calls.is_empty() {
            s.current_claims.insert(i, calls);
            claimants.push(i);
        }
    }
    s.wall.drawable_count = 0;
    if !claimants.is_empty() {
        s.phase = Phase::WaitResponse;
        s.active_players = claimants;
    }
}

/// Sanma twin of [`open_haitei_claims`]: pon (there is no chi in sanma)
/// on the last live-wall discard, same engine gate, same live incident
/// class.
fn open_haitei_claims_3p(s: &mut GameState3P, discarder: u8, pai: &str) {
    use riichienv_core::action::{ActionType, Phase};
    if s.wall.drawable_count != 0 || s.phase != Phase::WaitAct {
        return;
    }
    let Some(tile) = riichienv_core::parser::mjai_to_tid(pai) else {
        return;
    };
    s.wall.drawable_count = 1;
    let mut claimants = Vec::new();
    for i in 0..s.players.len() as u8 {
        if i == discarder {
            continue;
        }
        let (legals, _) = s._get_claim_actions_for_player(i, discarder, tile);
        let calls: Vec<_> = legals
            .into_iter()
            .filter(|a| matches!(a.action_type, ActionType::Chi | ActionType::Pon))
            .collect();
        if !calls.is_empty() {
            s.current_claims.insert(i, calls);
            claimants.push(i);
        }
    }
    s.wall.drawable_count = 0;
    if !claimants.is_empty() {
        s.phase = Phase::WaitResponse;
        s.active_players = claimants;
    }
}

/// Maintain `players[].ippatsu_cycle` on the replay path.
///
/// `apply_mjai_event(ReachAccepted)` in riichienv-core 0.4.8 leaves
/// `ippatsu_cycle` untouched (only the live-engine `_accept_riichi` sets it
/// to `true`). Without this patch, ippatsu is never detected when scoring
/// against the tracker state — e.g. `evaluate_hora_4p` would miss the +1 han
/// and the frontend's "+N points" display under-reports a riichi-ippatsu
/// hand by exactly one fan band.
///
/// We mirror the live-engine ippatsu lifecycle:
/// - `ReachAccepted`: open the window for the actor.
/// - Any call (`Chi`/`Pon`/`Daiminkan`/`Ankan`/`Kakan`): close every player's
///   window (calls break ippatsu globally).
/// - `Dahai`: close the discarder's window. The actor's reach-tile dahai
///   fires *before* `ReachAccepted`, so this clear is a no-op on that one
///   event; on every subsequent own dahai (the player didn't tsumo on their
///   next own draw) it correctly retires the window.
fn apply_ippatsu_patch_4p(s: &mut GameState, ev: &AkagiEvent) {
    match ev {
        AkagiEvent::ReachAccepted { actor } => {
            if let Some(p) = s.players.get_mut(*actor as usize) {
                p.ippatsu_cycle = true;
            }
        }
        AkagiEvent::Dahai { actor, .. } => {
            if let Some(p) = s.players.get_mut(*actor as usize) {
                p.ippatsu_cycle = false;
            }
        }
        AkagiEvent::Chi { .. }
        | AkagiEvent::Pon { .. }
        | AkagiEvent::Daiminkan { .. }
        | AkagiEvent::Ankan { .. }
        | AkagiEvent::Kakan { .. } => {
            for p in &mut s.players {
                p.ippatsu_cycle = false;
            }
        }
        _ => {}
    }
}

/// 3-player variant. Same lifecycle as 4p; `Kita` (北抜き) is treated like a
/// call because it interrupts the natural turn order with an off-cycle draw.
fn apply_ippatsu_patch_3p(s: &mut GameState3P, ev: &AkagiEvent) {
    match ev {
        AkagiEvent::ReachAccepted { actor } => {
            if let Some(p) = s.players.get_mut(*actor as usize) {
                p.ippatsu_cycle = true;
            }
        }
        AkagiEvent::Dahai { actor, .. } => {
            if let Some(p) = s.players.get_mut(*actor as usize) {
                p.ippatsu_cycle = false;
            }
        }
        AkagiEvent::Chi { .. }
        | AkagiEvent::Pon { .. }
        | AkagiEvent::Daiminkan { .. }
        | AkagiEvent::Ankan { .. }
        | AkagiEvent::Kakan { .. }
        | AkagiEvent::Kita { .. } => {
            for p in &mut s.players {
                p.ippatsu_cycle = false;
            }
        }
        _ => {}
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

/// Like [`spawn`] but also re-emits each consumed event on `post` **after**
/// the tracker has applied it, as a [`TrackedEvent`]. Subscribers to `post`
/// can rely on the tracker snapshot being current when they receive an
/// event, and on `can_act` describing *that* event rather than whatever the
/// tracker has reached by the time they get round to it.
pub fn spawn_with_post(
    rx: broadcast::Receiver<AkagiEvent>,
    post: Option<broadcast::Sender<TrackedEvent>>,
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
    post: Option<broadcast::Sender<TrackedEvent>>,
) {
    run(tracker, rx, post).await
}

async fn run(
    tracker: Arc<Mutex<GameTracker>>,
    mut rx: broadcast::Receiver<AkagiEvent>,
    post: Option<broadcast::Sender<TrackedEvent>>,
) {
    info!("game tracker subscribed to MJAI bus");
    loop {
        match rx.recv().await {
            Ok(ev) => {
                // Read `can_act` under the same lock that applied the event,
                // so it describes the state this event produced and not a
                // later one. A burst of events from one frame is applied in
                // microseconds; anything that asks afterwards has already
                // missed the state it meant to ask about.
                let can_act = {
                    let mut t = tracker.lock().await;
                    if let Err(e) = t.handle(&ev) {
                        warn!("game tracker: handle error: {e:#}");
                    }
                    t.our_seat_can_act()
                };
                if let Some(p) = &post {
                    // Receiver may have lagged or no-one subscribed yet — ignore.
                    let _ = p.send(TrackedEvent { event: ev, can_act });
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
            game_meta: None,
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

    /// Seat 0 holds a real hand — two 1m to pon with, and a 3m4m run — while
    /// the other three are the `?` the bridge feeds for hands we cannot see.
    fn start_kyoku_with_our_hand() -> AkagiEvent {
        let ours: Vec<String> = [
            "1m", "1m", "3m", "4m", "7m", "8m", "9m", "1p", "2p", "3p", "E", "E", "S",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let hidden: Vec<String> = (0..13).map(|_| "?".into()).collect();
        AkagiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25_000, 25_000, 25_000, 25_000],
            tehais: vec![ours, hidden.clone(), hidden.clone(), hidden],
            num_players: 4,
        }
    }

    fn dahai(actor: u8, pai: &str) -> AkagiEvent {
        AkagiEvent::Dahai {
            actor,
            pai: pai.into(),
            tsumogiri: false,
        }
    }

    /// riichienv-core gates chi/pon on a non-empty live wall; Riichi City
    /// offers them on the haitei discard too (live 2026-08-24 capture:
    /// the server offered a chi on the very last live-wall tile and the
    /// window timed out because the engine's empty claim set made
    /// `can_act` false). The post-apply patch must splice those claims
    /// back in — with the wall restored, so the false non-empty wall
    /// doesn't leak into anything else.
    #[test]
    fn haitei_discard_still_offers_claims() {
        use riichienv_core::action::Phase;
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        // Our seat (0) holds 3m4m among others; seat 3 is our claim
        // source ((3+1)%4 = 0).
        t.handle(&start_kyoku_with_our_hand()).unwrap();
        if let Some(TrackedGame::Four(s)) = t.state.as_mut() {
            s.wall.drawable_count = 0;
        }
        t.handle(&dahai(3, "2m")).unwrap();
        assert_eq!(
            t.our_seat_can_act(),
            Some(true),
            "the chi on the haitei discard must be offered"
        );
        if let Some(TrackedGame::Four(s)) = t.state.as_ref() {
            assert_eq!(s.wall.drawable_count, 0, "wall restored after the splice");
            assert_eq!(s.phase, Phase::WaitResponse);
        }
        // And a tile we cannot use still yields nothing at haitei.
        let mut t2 = GameTracker::new();
        t2.handle(&start_game()).unwrap();
        t2.handle(&start_kyoku_with_our_hand()).unwrap();
        if let Some(TrackedGame::Four(s)) = t2.state.as_mut() {
            s.wall.drawable_count = 0;
        }
        t2.handle(&dahai(3, "S")).unwrap();
        assert_eq!(
            t2.our_seat_can_act(),
            Some(false),
            "no pair, no run — nothing to claim"
        );
    }

    #[test]
    fn tracker_starts_empty() {
        let t = GameTracker::new();
        assert!(t.snapshot().is_none());
        assert!(t.state().is_none());
    }

    /// With no game and no seat the engine has nothing to say, and saying so
    /// matters: consumers fall back to their own policy on `None` rather than
    /// treating it as "cannot act" and going quiet for the whole session.
    #[test]
    fn can_act_has_no_opinion_before_a_game() {
        assert_eq!(GameTracker::new().our_seat_can_act(), None);

        let mut t = GameTracker::new();
        t.handle(&AkagiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: None, // observer / replay: no seat of ours
            num_players: 4,
            game_meta: None,
        })
        .unwrap();
        assert_eq!(t.our_seat_can_act(), None, "no seat, no opinion");
    }

    /// Our own draw is a decision — there is at minimum a discard to choose.
    #[test]
    fn can_act_on_our_own_draw() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku_with_our_hand()).unwrap();
        t.handle(&AkagiEvent::Tsumo {
            actor: 0,
            pai: "5s".into(),
        })
        .unwrap();
        assert_eq!(t.our_seat_can_act(), Some(true));
    }

    /// Regression (Hora answered with a pass press): a discard we hold no
    /// claim on is not ours to answer. The engine may still be in its
    /// response phase — another seat's claim puts it there, and it hands
    /// every seat a `Pass` while it is — so "the legal set is non-empty" is
    /// not the test; having something other than that pass is.
    #[test]
    fn cannot_act_on_an_opponent_discard_we_have_no_claim_on() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku_with_our_hand()).unwrap();
        t.handle(&AkagiEvent::Tsumo {
            actor: 1,
            pai: "9p".into(),
        })
        .unwrap();
        t.handle(&dahai(1, "9p")).unwrap();
        assert_eq!(
            t.our_seat_can_act(),
            Some(false),
            "no chi from toimen, no pon, no ron"
        );
    }

    /// The other half: a discard we *can* pon is a decision, so the bot is
    /// asked about that one and only that one.
    #[test]
    fn can_act_on_a_discard_we_can_claim() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku_with_our_hand()).unwrap();
        t.handle(&AkagiEvent::Tsumo {
            actor: 1,
            pai: "1m".into(),
        })
        .unwrap();
        t.handle(&dahai(1, "1m")).unwrap();
        assert_eq!(t.our_seat_can_act(), Some(true), "two 1m in hand — pon");
    }

    /// Our own discard hands the window to everyone else.
    #[test]
    fn cannot_act_on_our_own_discard() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        t.handle(&start_kyoku_with_our_hand()).unwrap();
        t.handle(&AkagiEvent::Tsumo {
            actor: 0,
            pai: "5s".into(),
        })
        .unwrap();
        t.handle(&dahai(0, "5s")).unwrap();
        assert_eq!(t.our_seat_can_act(), Some(false));
    }

    /// Drive a kyoku to just after seat 1 kakans the 5s it pon'd earlier.
    /// `our_hand` seeds our seat (0); opponents' hands stay hidden exactly
    /// as the bridge feeds them.
    fn drive_to_kakan(our_hand: &[&str]) -> GameTracker {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap();
        let ours: Vec<String> = our_hand.iter().map(|s| s.to_string()).collect();
        let hidden: Vec<String> = (0..13).map(|_| "?".into()).collect();
        t.handle(&AkagiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25_000, 25_000, 25_000, 25_000],
            tehais: vec![ours, hidden.clone(), hidden.clone(), hidden.clone()],
            num_players: 4,
        })
        .unwrap();
        // Seat 2 throws a 5s; seat 1 pons it.
        t.handle(&AkagiEvent::Tsumo {
            actor: 2,
            pai: "?".into(),
        })
        .unwrap();
        t.handle(&dahai(2, "5s")).unwrap();
        t.handle(&AkagiEvent::Pon {
            actor: 1,
            target: 2,
            pai: "5s".into(),
            consumed: ["5s".into(), "5s".into()],
        })
        .unwrap();
        // Turn order reaches seat 1 again; it draws the last 5s and kakans.
        // Hidden seats draw `?`; our own draw and every discard are real —
        // that is exactly what the bridge feeds.
        for (actor, draw, discard) in [(1u8, "?", "1z"), (2, "?", "3z"), (3, "?", "4z")] {
            t.handle(&AkagiEvent::Tsumo {
                actor,
                pai: draw.into(),
            })
            .unwrap();
            t.handle(&dahai(actor, discard)).unwrap();
        }
        t.handle(&AkagiEvent::Tsumo {
            actor: 0,
            pai: "2z".into(),
        })
        .unwrap();
        t.handle(&dahai(0, "2z")).unwrap();
        t.handle(&AkagiEvent::Tsumo {
            actor: 1,
            pai: "5s".into(),
        })
        .unwrap();
        t.handle(&AkagiEvent::Kakan {
            actor: 1,
            pai: "5s".into(),
            consumed: ["5s".into(), "5s".into(), "5s".into()],
        })
        .unwrap();
        t
    }

    /// Regression (2026-08-22, West 1): an opponent's kakan that completes
    /// our hand must surface as a real decision window — `can_act` true and
    /// a legal Ron — not as engine silence that leaves the bot unasked and
    /// autoplay with nothing to click until a human takes the ron.
    #[test]
    fn can_act_on_a_kakan_we_can_rob() {
        // Menzen tanyao waiting on 5s (`234m 567m 234p 567p 5s`).
        let t = drive_to_kakan(&[
            "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5p", "6p", "7p", "5s",
        ]);
        assert_eq!(t.our_seat_can_act(), Some(true));

        let legal = t.state().unwrap()._get_legal_actions_internal(0);
        assert!(
            legal
                .iter()
                .any(|a| a.action_type == riichienv_core::action::ActionType::Ron),
            "the robbed 5s must be a legal ron (legal: {legal:?})"
        );
        assert_eq!(
            t.snapshot().unwrap().phase,
            crate::game_state::snapshot::Phase::WaitResponse
        );
    }

    /// A kakan our hand has no claim on is the engine saying "not yours" —
    /// same as an unclaimable discard.
    #[test]
    fn cannot_act_on_a_kakan_we_cannot_rob() {
        // Same shape but waiting on 8s.
        let t = drive_to_kakan(&[
            "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5p", "6p", "7p", "8s",
        ]);
        assert_eq!(t.our_seat_can_act(), Some(false));
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
            game_meta: None,
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

    /// Regression (issue #149): `riichienv-core 0.4.8` grows a hidden seat's
    /// `hand` every turn — `Tsumo` pushes the unknown drawn tile but the
    /// following `Dahai` removes by matching the *known* discard, which never
    /// matches, so nothing is removed. The snapshot must reconstruct the
    /// concealed count (`seat_tehai`) so an opponent never shows > 13 backs.
    #[test]
    fn opponent_tehai_count_does_not_grow() {
        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap(); // observer = seat 0 (start_game id=0)
        t.handle(&start_kyoku(0)).unwrap();

        // Seat 1 (a hidden opponent) draws an unknown tile and discards a known
        // one, several times — the exact path that inflates the engine's hand.
        for _ in 0..6 {
            t.handle(&AkagiEvent::Tsumo {
                actor: 1,
                pai: "?".into(),
            })
            .unwrap();
            t.handle(&AkagiEvent::Dahai {
                actor: 1,
                pai: "3p".into(),
                tsumogiri: true,
            })
            .unwrap();
        }

        // Precondition: the underlying engine hand really did inflate.
        assert!(
            t.state().unwrap().players[1].hand.len() > 13,
            "engine hand should be inflated by unmatched discards"
        );

        let snap = t.snapshot().unwrap();
        // …but the snapshot stays at the true concealed size (no melds → 13).
        assert_eq!(snap.players[1].tehai.len(), 13);
        // The observer's own hand is untouched (engine tracks it correctly).
        assert_eq!(snap.players[0].tehai.len(), 13);
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
        t.handle(&AkagiEvent::Reach {
            actor: 0,
            pai: None,
        })
        .unwrap();
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

    /// Regression (issue #153): `riichienv-core 0.4.8` drops the naki `target`
    /// (stores the meld with `from_who = -1`) and never removes a claimed tile
    /// from the discarder's `discards`. The tracker patches `from_who` back to
    /// the real discarder, and the snapshot flags the claimed discard so the
    /// rendered river hides it while the analysis-facing entry is retained.
    #[test]
    fn pon_records_discarder_and_hides_called_tile() {
        use crate::game_state::mahgen_view::MahgenView;

        let mut t = GameTracker::new();
        t.handle(&start_game()).unwrap(); // observer = seat 0
        t.handle(&start_kyoku(0)).unwrap();

        // Seat 2 (toimen of seat 0) discards 1m; seat 0 pons it. start_kyoku
        // deals everyone 13×1m, so seat 0 holds the two 1m it consumes.
        t.handle(&AkagiEvent::Tsumo {
            actor: 2,
            pai: "1m".into(),
        })
        .unwrap();
        t.handle(&AkagiEvent::Dahai {
            actor: 2,
            pai: "1m".into(),
            tsumogiri: true,
        })
        .unwrap();
        t.handle(&AkagiEvent::Pon {
            actor: 0,
            target: 2,
            pai: "1m".into(),
            consumed: ["1m".into(), "1m".into()],
        })
        .unwrap();

        let snap = t.snapshot().unwrap();

        // Fix 1: the meld records the real discarder (toimen = seat 2), not the
        // riichienv-core `-1` default that `call_side` renders as kamicha.
        assert_eq!(
            snap.players[0].melds[0].from_who, 2,
            "meld must record the discarder seat"
        );

        // Fix 2: seat 2's lone discard is flagged claimed but kept in the list
        // so the analysis engine still sees it as genbutsu.
        assert_eq!(snap.players[2].river.len(), 1);
        assert!(
            snap.players[2].river[0].called,
            "claimed discard is retained but flagged"
        );

        // The rendered strings reflect both fixes: pon rotated toward toimen
        // (middle slot), discarder's river empty.
        let view = MahgenView::from_snapshot(&snap);
        assert_eq!(
            view.players[0].melds[0], "1_11m",
            "pon rotated toward toimen"
        );
        assert_eq!(view.players[2].river, "", "claimed tile hidden from river");
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
            game_meta: None,
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
