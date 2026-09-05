//! Tenhou (天鳳) protocol bridge.
//!
//! Wire format: plain JSON over WebSocket — `{"tag": "...", ...}`. Each
//! frame carries one tag identifying the event (`INIT`, `T0`, `D5`, `N`,
//! `REACH`, `AGARI`, `RYUUKYOKU`, …). Single-frame heartbeat is the literal
//! bytes `<Z/>`.
//!
//! Faithful Rust port of the observation half of the original Akagi Python
//! Tenhou bridge. AkagiV3 uses the bridge in
//! observe-only mode: only `Direction::Down` (server → client) is parsed,
//! `Direction::Up` and [`build`](Bridge::build) are no-ops. The rationale is
//! that all game state we need to feed analysis / bots arrives on server
//! frames; client frames are user input and contribute no new information.
//!
//! Tenhou tile encoding lives in [`tile`]; meld bitfield decoding lives in
//! [`meld`]; per-flow state in [`state`].

pub mod encode;
pub mod meld;
pub mod state;
pub mod tile;

use super::{Bridge, Direction, ParseResult};
use crate::{
    autoplay::tenhou_state::{SharedTenhouState, TenhouState},
    config::Platform,
    event_bus::NotifyBus,
    logger::{FlowLogger, Session},
    schema::{mjai::Actor, GameMeta, MatchInfo, MjaiEvent, Notification},
};
use chrono::Local;
use meld::{Meld, MeldKind};
use serde_json::Value as JsonValue;
use state::State;
use std::sync::Arc;
use tile::{tenhou_to_mjai, tenhou_to_mjai_one};
use tracing::{debug, info, warn};

const HEARTBEAT: &[u8] = b"<Z/>";
const BAKAZE: [&str; 4] = ["E", "S", "W", "N"];
/// Sentinel in a `<REINIT/>` `kawa<n>` list marking the riichi declaration.
const REINIT_RIICHI_MARK: u32 = 255;

/// Per-flow Tenhou state. One bridge instance per WebSocket connection.
pub struct TenhouBridge {
    state: State,
    #[allow(dead_code)]
    flow_log: Option<Arc<FlowLogger>>,
    session: Option<Arc<Session>>,
    mjai_log: Option<Arc<FlowLogger>>,
    /// Autoplay's view of the hand and the current decision window. Written
    /// after every parsed frame; `None` unless the chromium capture path
    /// wired it (see [`crate::autoplay::tenhou_state`]).
    shared: Option<SharedTenhouState>,
    /// Frontend toast channel, for states the user has to know about but no
    /// mjai event can carry (a mid-hand reconnect). `None` in tests.
    notify: Option<NotifyBus>,
}

impl TenhouBridge {
    pub fn new(flow_log: Option<Arc<FlowLogger>>, session: Option<Arc<Session>>) -> Self {
        Self {
            state: State::default(),
            flow_log,
            session,
            mjai_log: None,
            shared: None,
            notify: None,
        }
    }

    /// Attach the frontend notification bus.
    pub fn with_notify(mut self, notify: Option<NotifyBus>) -> Self {
        self.notify = notify;
        self
    }

    fn notify(&self, n: Notification) {
        if let Some(tx) = &self.notify {
            // No subscriber (headless / CLI) is not an error.
            let _ = tx.send(n);
        }
    }

    /// Attach the slot autoplay reads the hand and decision window from.
    pub fn with_shared_state(mut self, shared: Option<SharedTenhouState>) -> Self {
        self.shared = shared;
        self
    }

    /// Mirror the tracked hand + window into the shared slot. Called once per
    /// parsed frame, after dispatch has applied its effects.
    fn publish(&self) {
        let Some(shared) = &self.shared else { return };
        let Ok(mut guard) = shared.write() else {
            warn!(target: "akagi::bridge::tenhou", "tenhou state slot poisoned");
            return;
        };
        *guard = Some(TenhouState {
            seat: self.state.seat,
            hand: self.state.hand.clone(),
            melds: self.state.melds.clone(),
            is_tsumo: self.state.is_tsumo,
            window: self.state.window,
        });
    }

    /// Open a fresh `tenhou_<ts>.mjai.jsonl` mirroring the Majsoul rotation
    /// pattern. No-op when no session is wired.
    fn rotate_mjai_log(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let ts = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
        let file_name = format!("tenhou_{ts}.mjai.jsonl");
        let label = format!("tenhou mjai {ts}");
        match session.flow_logger(Platform::Tenhou.subdir(), &file_name, label) {
            Ok(log) => {
                info!(target: "akagi::bridge::tenhou", "opened mjai log {file_name}");
                self.mjai_log = Some(log);
            }
            Err(e) => {
                warn!(target: "akagi::bridge::tenhou", "failed to open mjai log {file_name}: {e:#}");
                self.mjai_log = None;
            }
        }
    }

    /// Open the mjai log if this flow has none yet. `<TAIKYOKU/>` rotates
    /// one per game; a flow that starts mid-game (reconnect, late attach)
    /// never sees it, so the first kyoku frame opens the file instead.
    fn ensure_mjai_log(&mut self) {
        if self.mjai_log.is_none() {
            self.rotate_mjai_log();
        }
    }

    fn write_mjai(&self, events: &[MjaiEvent]) {
        let Some(log) = &self.mjai_log else { return };
        for ev in events {
            match serde_json::to_string(ev) {
                Ok(line) => log.writeln(&line),
                Err(e) => warn!(
                    target: "akagi::bridge::tenhou",
                    "failed to serialize MjaiEvent: {e:#}"
                ),
            }
        }
    }

    fn dispatch(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let Some(tag) = msg.get("tag").and_then(JsonValue::as_str) else {
            return Vec::new();
        };

        // Tags that contribute no mjai events — silently ignored. The Python
        // reference does the same in `_convert_helo`, `_convert_rejoin`, etc.
        // `SAIKAI` (再開) is the rejoin's resume notice — `ba`, `oya`, `sc` —
        // sent between `<UN/>` and `<REINIT/>`; everything it says, REINIT
        // repeats.
        match tag {
            "HELO" | "REJOIN" | "SAIKAI" | "BYE" | "SHUFFLE" => return Vec::new(),
            _ => {}
        }

        let mut events = self.dispatch_tag(tag, msg);

        // A flow that attached mid-kyoku (`<REINIT/>`) has no history for the
        // hand in progress, so its events would land on whatever the tracker
        // last saw — a hand it may be several turns behind on, or none at
        // all. The handler still ran (autoplay needs the hand and window it
        // maintains); only the stream is withheld until `on_init` lifts the
        // suspension at the next kyoku.
        // The one exception is the game's end: the tracker has to close the
        // game, the bot manager stop its runner, History reset — so
        // `end_game` alone passes, and as terminated (see `end_game_event`).
        if self.state.suspended {
            let before = events.len();
            events.retain(|e| matches!(e, MjaiEvent::EndGame { .. }));
            if before != events.len() {
                debug!(
                    target: "akagi::bridge::tenhou",
                    "dropping {} event(s) from <{tag}/>: joined mid-kyoku, waiting for the next INIT",
                    before - events.len()
                );
            }
        }
        events
    }

    /// The game-end event for this flow. A game this flow joined mid-way has
    /// no complete record — the hands before the rejoin were never seen here
    /// — so its end is reported as terminated: the tracker and bots still
    /// close the game, but History drops it instead of filing the remainder
    /// as a full game with its own per-hand stats.
    fn end_game_event(&self) -> MjaiEvent {
        if self.state.rejoined_mid_game {
            MjaiEvent::terminated_game()
        } else {
            MjaiEvent::end_game()
        }
    }

    fn dispatch_tag(&mut self, tag: &str, msg: &JsonValue) -> Vec<MjaiEvent> {
        if tag == "GO" {
            return self.on_go(msg);
        }
        if tag == "UN" {
            return self.on_un(msg);
        }
        if tag == "TAIKYOKU" {
            return self.on_taikyoku(msg);
        }
        if tag == "INIT" {
            return self.on_init(msg);
        }
        if tag == "REINIT" {
            return self.on_reinit(msg);
        }
        if let Some(actor) = tsumo_actor(tag) {
            return self.on_tsumo(actor, tag, msg);
        }
        if let Some((actor, tag_tsumogiri)) = dahai_actor(tag) {
            return self.on_dahai(actor, tag, tag_tsumogiri, msg);
        }
        if tag == "N" && msg.get("m").is_some() {
            return self.on_meld(msg);
        }
        if tag == "REACH" {
            return self.on_reach(msg);
        }
        if tag == "DORA" {
            return self.on_dora(msg);
        }
        if tag == "AGARI" {
            return self.on_agari(msg);
        }
        if tag == "RYUUKYOKU" {
            return self.on_ryukyoku(msg);
        }

        Vec::new()
    }

    /// `<GO type="…" lobby="…"/>` — rules/room announcement, sent before
    /// `<TAIKYOKU/>`. No mjai events; the raw bitfield is stashed for
    /// history's `MatchInfo` (room tier lives in bits 0x20 / 0x80). Bit
    /// 0x10 is the three-player flag — the earliest sanma signal, and the
    /// only one a flow gets before its first kyoku frame when that frame is
    /// not the E1H0 `<INIT/>` (see `on_init`).
    fn on_go(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        self.state.go_type = parse_u32(msg, "type");
        self.state.lobby = parse_u32(msg, "lobby");
        if let Some(three) = self.state.three_players_hint() {
            self.state.set_three_players(three);
        }
        Vec::new()
    }

    /// `<UN n0=… n1=… …/>` — player roster, names percent-encoded UTF-8 in
    /// wire-*relative* order (n0 = us). A reconnect `<UN/>` carries only the
    /// returning player's name, so only the roster form (n0 and n1 both
    /// present) is accepted — a partial update must not clobber good names.
    /// No mjai events; consumed at `start_game` emission. A roster with one
    /// empty slot is the sanma ghost, which doubles as a player-count hint.
    fn on_un(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let name = |key: &str| {
            msg.get(key).and_then(JsonValue::as_str).map(|raw| {
                percent_encoding::percent_decode_str(raw)
                    .decode_utf8_lossy()
                    .into_owned()
            })
        };
        let (Some(n0), Some(n1)) = (name("n0"), name("n1")) else {
            return Vec::new();
        };
        let n2 = name("n2").unwrap_or_default();
        let n3 = name("n3").unwrap_or_default();
        self.state.un_names = Some([n0, n1, n2, n3]);
        if let Some(three) = self.state.three_players_hint() {
            self.state.set_three_players(three);
        }
        Vec::new()
    }

    /// `<TAIKYOKU oya="N" log="…"/>` — start of game. Resolves our
    /// wire-absolute seat but defers `start_game` emission until the first
    /// `<INIT/>`, where the 0-score slot reveals whether the game is sanma.
    /// Without that wait we'd stamp `start_game.num_players = 4` on every
    /// sanma game (Tenhou's TAIKYOKU itself carries no player-count signal —
    /// only the dealer's relative seat). The `log` attribute is the paifu id.
    fn on_taikyoku(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let oya_rel = parse_u8(msg, "oya").unwrap_or(0);
        self.state.log_id = msg
            .get("log")
            .and_then(JsonValue::as_str)
            .map(str::to_string);
        // oya is dealer's *relative* seat in the 4-cycle wire frame. Our
        // wire-abs seat is the inverse: (-oya_rel) mod 4. For sanma our
        // wire-abs is always in {0, 1, 2} because we are a real player —
        // wire-abs 3 is the ghost slot.
        //
        // Player count: a new game re-announces itself with <GO/> (and
        // <UN/>), so their hint is this game's; the E1H0 <INIT/> that
        // follows has the final say either way.
        self.state
            .set_three_players(self.state.three_players_hint().unwrap_or(false));
        self.state.seat = (4 - oya_rel) % 4;
        self.state.seat_resolved = true;
        // A new game on a flow that was waiting out a mid-kyoku join.
        self.state.suspended = false;
        self.state.rejoined_mid_game = false;
        self.state.pending_start_game = true;
        self.rotate_mjai_log();
        Vec::new()
    }

    /// Adopt the seat a kyoku frame implies when `<TAIKYOKU/>` never told
    /// us — a flow that attached mid-game (reconnect, late start). When it
    /// did, TAIKYOKU stays authoritative and a disagreement is only logged:
    /// it would mean a malformed frame, not a seat change. The first deal
    /// (`first_deal`: E1 honba 0) is the exception — it is the same
    /// statement TAIKYOKU makes, so it always wins, which also lets a new
    /// game re-seat a flow whose TAIKYOKU went missing.
    fn resolve_seat_from_kyoku(
        &mut self,
        tag: &str,
        kyoku_index: i32,
        oya_rel: u8,
        first_deal: bool,
    ) {
        let derived = state::seat_from_kyoku(kyoku_index, oya_rel);
        if self.state.seat_resolved && first_deal && derived == self.state.seat {
            // TAIKYOKU and the first deal agree, as they always should.
            return;
        }
        if self.state.seat_resolved && !first_deal {
            if derived != self.state.seat {
                warn!(
                    target: "akagi::bridge::tenhou",
                    "<{tag}/> seed={kyoku_index} oya={oya_rel} implies our seat is {derived}, \
                     TAIKYOKU said {}; keeping TAIKYOKU's",
                    self.state.seat
                );
            }
            return;
        }
        if self.state.is_ghost_abs(derived) {
            warn!(
                target: "akagi::bridge::tenhou",
                "<{tag}/> seed={kyoku_index} oya={oya_rel} resolves our seat onto the sanma ghost slot; \
                 keeping seat {}",
                self.state.seat
            );
            return;
        }
        info!(
            target: "akagi::bridge::tenhou",
            "<{tag}/> on a flow with no TAIKYOKU: resolved our seat as {derived} from seed={kyoku_index} oya={oya_rel}"
        );
        self.state.seat = derived;
        self.state.seat_resolved = true;
    }

    /// Seat-ordered display names for `start_game`. `<UN/>` roster names are
    /// wire-relative (index 0 = us) and arrive before `<TAIKYOKU/>` resolves
    /// our seat, so the remap to wire-absolute happens here (sanma ghost slot
    /// skipped). Falls back to the historical seat-number placeholders when
    /// no roster `<UN/>` was seen.
    fn build_start_names(&mut self) -> Vec<String> {
        let n = self.state.num_players as usize;
        let mut names: Vec<String> = (0..n).map(|i| i.to_string()).collect();
        if let Some(un) = self.state.un_names.take() {
            for (rel, name) in un.into_iter().enumerate() {
                if name.is_empty() {
                    continue;
                }
                let abs = self.state.rel_to_abs(rel as u8);
                if self.state.is_ghost_abs(abs) {
                    continue;
                }
                if let Some(slot) = names.get_mut(abs as usize) {
                    *slot = name;
                }
            }
        }
        names
    }

    /// `<INIT seed="..." ten="..." oya="..." hai0="..."/>` — start of kyoku.
    fn on_init(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let seed = parse_csv_i32(msg, "seed");
        let ten = parse_csv_i32(msg, "ten");
        let oya_rel = parse_u8(msg, "oya").unwrap_or(0);
        if seed.len() < 6 {
            warn!(target: "akagi::bridge::tenhou", "INIT missing seed fields: {msg}");
            return Vec::new();
        }
        if ten.is_empty() {
            warn!(target: "akagi::bridge::tenhou", "INIT missing ten field: {msg}");
            return Vec::new();
        }

        // The seat, in case this flow never saw <TAIKYOKU/>. Must precede
        // every rel→abs translation below.
        let first_deal = seed[0] == 0 && seed[1] == 0;
        self.resolve_seat_from_kyoku("INIT", seed[0], oya_rel, first_deal);
        // A new kyoku is a clean slate for the tracker: whatever this flow
        // missed of the previous one no longer matters.
        self.state.suspended = false;
        if first_deal {
            // E1H0 happens once per game, so it is a game start whether or
            // not a <TAIKYOKU/> announced it; a flow that rejoined the
            // previous game is whole again from here.
            self.state.pending_start_game = true;
            self.state.rejoined_mid_game = false;
        }
        self.ensure_mjai_log();

        let bakaze = BAKAZE[(seed[0] as usize) / 4];
        let kyoku = (seed[0] as u8) % 4 + 1;
        let honba = seed[1].max(0) as u8;
        let kyotaku = seed[2].max(0) as u8;
        let dora_marker = tenhou_to_mjai_one(seed[5] as u32);
        let raw_ten: Vec<i32> = ten.iter().map(|s| s * 100).collect();

        // Sanma detection per Python reference: at the very first kyoku of a
        // game, a 0-score slot in `ten` signals the absent 4th player. No
        // legitimate game starts with anyone at 0 points, so the value-based
        // check is safe at E1H0 (it would not be safe mid-game) and overrides
        // the <GO/> / <UN/> hints. We do NOT wrap `seat` after detection —
        // wire-abs is still 4-cycle, and our wire-abs (already in {0, 1, 2}
        // for any real sanma player) doubles as mjai-abs.
        if first_deal {
            self.state.set_three_players(raw_ten.contains(&0));
        } else if self.state.three_players_hint().is_none() {
            // A flow that joined mid-game with neither <GO/> nor <UN/> —
            // the table total still says how many are playing.
            if let Some(three) = state::three_players_from_scores(&raw_ten, kyotaku) {
                self.state.set_three_players(three);
            }
        }

        // Our hand. Tenhou messages are already in *our* viewpoint (rel seat 0
        // is us), so the hand is always under the bare key `hai` regardless of
        // our absolute seat — not `hai{seat}` as the legacy XML docstring
        // suggests. Other seats appear as `'?'` placeholders.
        let our_hand_indices = parse_csv_u32(msg, "hai");
        self.state.reset_for_kyoku();
        self.state.hand = our_hand_indices.clone();

        let oya_abs = self.state.rel_to_abs(oya_rel);

        // Build per-seat starting hands. Length = num_players. Our wire-abs
        // is in [0, num_players) for both yonma and sanma (we are always
        // real), so it indexes directly into the mjai tehais vector.
        let n = self.state.num_players as usize;
        let mut tehais: Vec<Vec<String>> = vec![vec!["?".to_string(); 13]; n];
        tehais[self.state.seat as usize] = tenhou_to_mjai(&our_hand_indices);

        // Tenhou `ten` is in *relative* seat order in the 4-cycle wire frame
        // (rel 0 is us, rel 3 is kamicha in yonma or whoever lands on the
        // 4th wire position in sanma — possibly the ghost, possibly a real
        // player). For sanma the ghost is always at wire-abs 3, but its
        // relative position depends on our seat — so the ghost's relative
        // index is NOT a constant. Iterate all 4 entries, map each to its
        // wire-abs via `rel_to_abs`, and skip whichever lands on the ghost.
        let mut scores = vec![0i32; n];
        for (i, &s) in raw_ten.iter().enumerate().take(4) {
            let abs = self.state.rel_to_abs(i as u8);
            if self.state.is_ghost_abs(abs) {
                continue;
            }
            scores[abs as usize] = s;
        }

        let mut events = Vec::with_capacity(2);
        if self.state.pending_start_game {
            self.state.pending_start_game = false;
            let names = self.build_start_names();
            // Read non-destructively: a reconnect can replay TAIKYOKU+INIT
            // without a fresh <GO/>, and the re-emitted start_game must
            // keep the room bitfield. A *new* game always sends its own
            // <GO/> (and every TAIKYOKU reassigns log_id), so staleness
            // across games isn't a concern. Names ARE consumed (in
            // `build_start_names`): a stale roster is worse than a missing
            // one, and reconnects resend the full <UN/>.
            let match_info = MatchInfo::Tenhou {
                log_id: self.state.log_id.clone(),
                go_type: self.state.go_type,
                lobby: self.state.lobby,
            };
            events.push(MjaiEvent::StartGame {
                names,
                kyoku_first: None,
                aka_flag: None,
                id: Some(self.state.seat as Actor),
                num_players: self.state.num_players,
                game_meta: Some(GameMeta {
                    game_id: None,
                    match_mode: None,
                    match_info: Some(match_info),
                }),
            });
        }
        events.push(MjaiEvent::StartKyoku {
            bakaze: bakaze.to_string(),
            dora_marker,
            kyoku,
            honba,
            kyotaku,
            oya: oya_abs as Actor,
            scores,
            tehais,
            num_players: self.state.num_players,
        });
        events
    }

    /// `<REINIT seed="…" ten="…" oya="…" hai="…" m0..m3="…" kawa0..kawa3="…"/>`
    /// — the server's picture of a kyoku already in progress, sent in place
    /// of `<INIT/>` when the client rejoins a game.
    ///
    /// It is a snapshot, not a log: each seat's river and calls are listed,
    /// but nothing says *when* a call happened relative to the discards, so
    /// the mjai stream for the hand cannot be rebuilt (Majsoul's
    /// `GameRestore` can — it is an ordered action log). We therefore emit
    /// nothing, rebuild only what this flow itself needs — our seat, the
    /// player count, and the hand / calls / riichi state autoplay and
    /// `build` read — and suspend the stream until the next `<INIT/>`,
    /// which re-opens the game with a fresh `start_game`. Feeding the live
    /// events into the tracker instead would apply them to a hand it last
    /// saw several turns ago (or, with a fresh seat-0 bridge, to the wrong
    /// seats altogether), and every `?` draw that lands on our seat renders
    /// as a 1m.
    ///
    /// Field layout mirrors `<INIT/>` (`seed` / `ten` / `oya` / `hai`, all
    /// wire-relative) plus per-seat `m<rel>` (comma-separated `<N m=…/>`
    /// bitfields) and `kawa<rel>` (tile indices; `255` marks the riichi
    /// declaration).
    fn on_reinit(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let seed = parse_csv_i32(msg, "seed");
        let ten = parse_csv_i32(msg, "ten");
        let oya_rel = parse_u8(msg, "oya").unwrap_or(0);
        if seed.is_empty() {
            warn!(target: "akagi::bridge::tenhou", "REINIT missing seed field: {msg}");
            return Vec::new();
        }
        let first_deal = seed[0] == 0 && seed.get(1) == Some(&0);
        self.resolve_seat_from_kyoku("REINIT", seed[0], oya_rel, first_deal);

        let hai = parse_csv_u32(msg, "hai");
        let kawa = |rel: u8| -> Vec<u32> {
            parse_csv_u32(msg, &format!("kawa{rel}"))
                .into_iter()
                .filter(|&t| t != REINIT_RIICHI_MARK)
                .collect()
        };

        // Player count. <GO/> / <UN/> normally precede a rejoin and decide
        // it. A flow that saw neither reads the table total (see
        // `three_players_from_scores`); should that be unreadable, the
        // frame's shape is the last resort: the sanma ghost has no score to
        // move (stays 0) and no river, and neither our hand nor any river can
        // hold a 2m–8m (sanma removes them). Each alone happens in yonma;
        // all of them together do not.
        if self.state.three_players_hint().is_none() {
            let raw_ten: Vec<i32> = ten.iter().map(|s| s * 100).collect();
            let kyotaku = seed.get(2).copied().unwrap_or(0).max(0) as u8;
            match state::three_players_from_scores(&raw_ten, kyotaku) {
                Some(three) => self.state.set_three_players(three),
                None if !self.state.is_3p => {
                    let ghost_rel = self.state.abs_to_rel(3);
                    let ghost_score_zero = ten.get(ghost_rel as usize) == Some(&0);
                    let ghost_river_empty = kawa(ghost_rel).is_empty();
                    let middle_manzu = |t: &u32| (4..=31).contains(t);
                    let no_middle_manzu = !hai.iter().any(middle_manzu)
                        && !(0..4).any(|rel| kawa(rel).iter().any(middle_manzu));
                    if ghost_score_zero && ghost_river_empty && no_middle_manzu {
                        info!(target: "akagi::bridge::tenhou", "REINIT looks like sanma (ghost slot idle, no 2m–8m in play)");
                        self.state.set_three_players(true);
                    }
                }
                None => {}
            }
        }

        // Rebuild the per-kyoku state autoplay reads. Only our own calls are
        // tracked (as in `on_meld`); nukidora is not a meld, but each one
        // took a tile out of the hand.
        self.state.reset_for_kyoku();
        self.state.hand = hai;
        let m0 = parse_csv_u32(msg, "m0");
        let nukidora = m0.iter().filter(|m| (*m & 0x3F) == 0x20).count();
        self.state.melds = m0
            .into_iter()
            .filter(|m| (m & 0x3F) != 0x20)
            .map(Meld::parse)
            .collect();
        self.state.in_riichi = parse_csv_u32(msg, "kawa0").contains(&REINIT_RIICHI_MARK);
        // 13 tiles (+ a 14th while a draw is in hand), less 3 per call and
        // 1 per nukidora.
        self.state.is_tsumo =
            (self.state.hand.len() + 3 * self.state.melds.len() + nukidora) % 3 == 2;
        // Every discard followed a draw or a call; close enough for a
        // counter nothing downstream reads precisely.
        let discarded: u32 = (0..4).map(|rel| kawa(rel).len() as u32).sum();
        self.state.live_wall = 70u32.saturating_sub(discarded + u32::from(self.state.is_tsumo));

        self.state.suspended = true;
        self.state.rejoined_mid_game = true;
        self.state.pending_start_game = true;
        self.ensure_mjai_log();
        warn!(
            target: "akagi::bridge::tenhou",
            "REINIT: rejoined seed={} as seat {} with {} tiles, {} call(s){}; analysis paused until the next INIT",
            seed[0],
            self.state.seat,
            self.state.hand.len(),
            self.state.melds.len(),
            if self.state.in_riichi { ", in riichi" } else { "" }
        );
        self.notify(
            Notification::warn("Tenhou: rejoined mid-hand")
                .body("The hand in progress can't be reconstructed, so analysis is paused until the next hand starts.")
                .id("tenhou-reinit"),
        );
        Vec::new()
    }

    /// `<T0/>`, `<U7/>`, `<V12/>`, `<W3/>` — tsumo.
    fn on_tsumo(&mut self, actor_rel: u8, tag: &str, msg: &JsonValue) -> Vec<MjaiEvent> {
        if actor_rel >= 4 {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
        if self.state.is_ghost_abs(actor) {
            // Tenhou shouldn't emit tsumo from a ghost seat; drop defensively.
            warn!(target: "akagi::bridge::tenhou", "tsumo from sanma ghost slot rel={actor_rel}, dropped");
            return Vec::new();
        }
        self.state.live_wall = self.state.live_wall.saturating_sub(1);
        let mut pai = "?".to_string();
        if actor == self.state.seat {
            if let Some(idx) = parse_tail_u32(tag, 1) {
                pai = tenhou_to_mjai_one(idx);
                self.state.hand.push(idx);
                self.state.is_tsumo = true;
            }
            // Our draw always owes a discard, so the window opens whether or
            // not the server offered anything extra (`t` absent → 0).
            self.state.open_window(parse_u32(msg, "t").unwrap_or(0));
        } else {
            self.state.window = None;
        }
        let events = vec![MjaiEvent::Tsumo { actor, pai }];
        events
    }

    /// `<D7/>`, `<E/>`, `<f12/>` — dahai.
    /// `tag_tsumogiri` is true when the tag's leading letter is lowercase
    /// (Tenhou's signal that the discard is the just-drawn tile); uppercase
    /// means tedashi. See `dahai_actor`.
    fn on_dahai(
        &mut self,
        actor_rel: u8,
        tag: &str,
        tag_tsumogiri: bool,
        msg: &JsonValue,
    ) -> Vec<MjaiEvent> {
        if actor_rel >= 4 {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
        if self.state.is_ghost_abs(actor) {
            warn!(target: "akagi::bridge::tenhou", "dahai from sanma ghost slot rel={actor_rel}, dropped");
            return Vec::new();
        }

        // Determine the actual tile index. If the tag has no digits, it must
        // be our own tsumogiri — use the most recently drawn tile.
        let idx = match parse_tail_u32(tag, 1) {
            Some(i) => i,
            None => {
                if actor != self.state.seat {
                    return Vec::new();
                }
                match self.state.hand.last().copied() {
                    Some(i) => i,
                    None => return Vec::new(),
                }
            }
        };
        let pai = tenhou_to_mjai_one(idx);

        // Tsumogiri logic: for our own discards the tag's case is not a
        // reliable signal, so compare against the just-drawn tile instead —
        // `on_tsumo` pushes it onto the tail of `hand`.
        //
        // The `is_tsumo` gate is load-bearing. A call removes tiles from the
        // *middle* of `hand`, so after our own chi/pon/kan the tail still
        // holds the tile we drew last turn; without the gate, discarding that
        // tile reports a tedashi as tsumogiri.
        let tsumogiri = if actor == self.state.seat {
            self.state.is_tsumo && self.state.hand.last().copied() == Some(idx)
        } else {
            tag_tsumogiri
        };

        self.state.last_kawa_tile = pai.clone();
        self.state.last_revealed_tile_actor = Some(actor);
        self.state.is_tsumo = false;
        if actor == self.state.seat {
            if let Some(pos) = self.state.hand.iter().rposition(|&i| i == idx) {
                self.state.hand.remove(pos);
            }
            // Our own discard closes the window it satisfied.
            self.state.window = None;
        } else {
            // Someone else's discard opens a claim window only if the server
            // says we may claim it (`t` bits: pon/kan/chi/ron).
            match parse_u32(msg, "t").unwrap_or(0) {
                0 => self.state.window = None,
                ops => self.state.open_window(ops),
            }
        }

        let events = vec![MjaiEvent::Dahai {
            actor,
            pai,
            tsumogiri,
        }];
        events
    }

    /// `<N who="..." m="..."/>` — call (chi/pon/kan/kakan/nukidora).
    fn on_meld(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let actor_rel = parse_u8(msg, "who").unwrap_or(0);
        if actor_rel >= 4 {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
        if self.state.is_ghost_abs(actor) {
            warn!(target: "akagi::bridge::tenhou", "meld from sanma ghost slot rel={actor_rel}, dropped");
            return Vec::new();
        }
        let m = parse_u32(msg, "m").unwrap_or(0);

        // A call ends whatever draw was in flight: our next discard is a
        // tedashi unless a fresh `T<n>` arrives first (it does for the
        // rinshan draw after ankan / kakan / nukidora). `on_dahai` reads this
        // flag, so leaving it set would mis-report the post-call discard.
        self.state.is_tsumo = false;

        // Whatever claim window this call resolved is now spent. Chi and pon
        // re-open it below because they owe a discard; the kans and nukidora
        // draw a rinshan replacement first, and that `T<n>` opens its own.
        self.state.window = None;

        // ...unless the meld itself is claimable: an opponent's kakan can be
        // robbed (chankan), and the server says so the same way it does for a
        // discard — a `t` bitmask on the frame. The client draws its ron menu
        // straight from that attribute, so a frame without one opens nothing.
        if actor != self.state.seat {
            match parse_u32(msg, "t").unwrap_or(0) {
                0 => {}
                ops => self.state.open_window(ops),
            }
        }

        // Nukidora has its own bit pattern; handle before structured parse.
        if (m & 0x3F) == 0x20 {
            if actor == self.state.seat {
                // Remove one north tile from hand. North is type 30 (108..=111).
                if let Some(pos) = self.state.hand.iter().position(|&i| i / 4 == 30) {
                    self.state.hand.remove(pos);
                }
            }
            let events = vec![MjaiEvent::Kita {
                actor,
                pai: Some("N".to_string()),
            }];
            return events;
        }

        let meld = Meld::parse(m);
        // Target is in the 4-cycle wire frame; meld.target_rel from the
        // bitfield is also 4-cycle. Sanma never calls chi (only pon/kan
        // permitted), so the "kamicha" shortcut only fires for yonma.
        let target = match meld.kind {
            MeldKind::Chi => (actor + 4 - 1) % 4,
            _ => (actor + meld.target_rel) % 4,
        };

        let pai = meld.pai();
        let consumed = meld.consumed();

        let event = match meld.kind {
            MeldKind::Chi => MjaiEvent::Chi {
                actor,
                target,
                pai,
                consumed: [consumed[0].clone(), consumed[1].clone()],
            },
            MeldKind::Pon => MjaiEvent::Pon {
                actor,
                target,
                pai,
                consumed: [consumed[0].clone(), consumed[1].clone()],
            },
            MeldKind::Daiminkan => MjaiEvent::Daiminkan {
                actor,
                target,
                pai,
                consumed: [
                    consumed[0].clone(),
                    consumed[1].clone(),
                    consumed[2].clone(),
                ],
            },
            MeldKind::Kakan => {
                self.state.last_revealed_tile_actor = Some(actor); // chankan target
                MjaiEvent::Kakan {
                    actor,
                    pai,
                    consumed: [
                        consumed[0].clone(),
                        consumed[1].clone(),
                        consumed[2].clone(),
                    ],
                }
            }
            MeldKind::Ankan => MjaiEvent::Ankan {
                actor,
                consumed: [
                    consumed[0].clone(),
                    consumed[1].clone(),
                    consumed[2].clone(),
                    consumed[3].clone(),
                ],
            },
        };

        if actor == self.state.seat {
            for &i in meld.exposed() {
                if let Some(pos) = self.state.hand.iter().position(|&h| h == i) {
                    self.state.hand.remove(pos);
                }
            }
            // Chi and pon hand the turn straight back to us with a discard
            // owed and nothing optional on offer.
            if matches!(meld.kind, MeldKind::Chi | MeldKind::Pon) {
                self.state.open_window(0);
            }
            self.state.melds.push(meld);
        } else {
            // Track other players' melds is not required — observation only.
        }

        let events = vec![event];
        events
    }

    /// `<REACH who="..." step="..."/>` — riichi declaration / acceptance.
    fn on_reach(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let actor_rel = parse_u8(msg, "who").unwrap_or(0);
        if actor_rel >= 4 {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
        if self.state.is_ghost_abs(actor) {
            warn!(target: "akagi::bridge::tenhou", "reach from sanma ghost slot rel={actor_rel}, dropped");
            return Vec::new();
        }
        // step arrives as a string in the Python reference (`message['step'] == '1'`).
        let step = msg.get("step").and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<u8>().ok())
                .or(v.as_u64().map(|n| n as u8))
        });
        match step {
            Some(1) => {
                // Step 1 acknowledges the declaration; the riichi tile is
                // still owed as a separate discard frame, so our window
                // re-opens with nothing optional on offer.
                if actor == self.state.seat {
                    self.state.open_window(0);
                }
                vec![MjaiEvent::Reach { actor, pai: None }]
            }
            Some(2) => {
                if actor == self.state.seat {
                    self.state.in_riichi = true;
                }
                vec![MjaiEvent::ReachAccepted { actor }]
            }
            _ => Vec::new(),
        }
    }

    /// `<DORA hai="..."/>` — new dora indicator (kan dora).
    fn on_dora(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let Some(idx) = parse_u32(msg, "hai") else {
            return Vec::new();
        };
        let dora_marker = tenhou_to_mjai_one(idx);
        let events = vec![MjaiEvent::Dora { dora_marker }];
        events
    }

    /// `<AGARI .../>` — win.
    fn on_agari(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let actor_rel = parse_u8(msg, "who").unwrap_or(0);
        let from_rel = parse_u8(msg, "fromWho").unwrap_or(actor_rel);
        if actor_rel >= 4 || from_rel >= 4 {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
        let target = self.state.rel_to_abs(from_rel);
        if self.state.is_ghost_abs(actor) || self.state.is_ghost_abs(target) {
            warn!(target: "akagi::bridge::tenhou", "agari involves sanma ghost slot, dropped");
            return Vec::new();
        }
        // The kyoku is over; nothing is owed and nothing may be claimed.
        self.state.window = None;

        // sc field is "before0,delta0,before1,delta1,..." in wire-rel order
        // (rel 0 = us). Always carries 4 pairs even in sanma; skip the pair
        // whose wire-abs is the ghost slot before placing into mjai-abs.
        let sc = parse_csv_i32(msg, "sc");
        let mut deltas_abs = vec![0i32; self.state.num_players as usize];
        for (rel, chunk) in sc.chunks(2).take(4).enumerate() {
            let abs = self.state.rel_to_abs(rel as u8);
            if self.state.is_ghost_abs(abs) {
                continue;
            }
            let d = if chunk.len() == 2 { chunk[1] * 100 } else { 0 };
            deltas_abs[abs as usize] = d;
        }

        // Ura dora markers are space-separated tile indices in `dorahaiUra`.
        let ura_markers = msg.get("doraHaiUra").and_then(parse_tile_csv);

        let mut events = vec![MjaiEvent::Hora {
            actor,
            target,
            deltas: Some(deltas_abs),
            ura_markers,
        }];
        events.push(MjaiEvent::EndKyoku);
        if msg.get("owari").is_some() {
            events.push(self.end_game_event());
        }
        events
    }

    /// `<RYUUKYOKU .../>` — exhaustive draw.
    fn on_ryukyoku(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        self.state.window = None;
        let sc = parse_csv_i32(msg, "sc");
        let mut deltas_abs = vec![0i32; self.state.num_players as usize];
        for (rel, chunk) in sc.chunks(2).take(4).enumerate() {
            let abs = self.state.rel_to_abs(rel as u8);
            if self.state.is_ghost_abs(abs) {
                continue;
            }
            let d = if chunk.len() == 2 { chunk[1] * 100 } else { 0 };
            deltas_abs[abs as usize] = d;
        }

        let mut events = vec![
            MjaiEvent::Ryukyoku {
                deltas: Some(deltas_abs),
            },
            MjaiEvent::EndKyoku,
        ];
        if msg.get("owari").is_some() {
            events.push(self.end_game_event());
        }
        events
    }
}

impl Bridge for TenhouBridge {
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult {
        use crate::schema::ParsedFrame;
        // Per design decision: Tenhou observation only consumes server frames.
        // Client frames carry no information our analysis pipeline needs.
        if direction == Direction::Up {
            return ParseResult::empty();
        }
        if content == HEARTBEAT {
            // Surface heartbeats in the inspector with a stable synthetic
            // method name — the user can filter them out, but seeing them
            // confirms the connection is alive.
            return ParseResult {
                events: Vec::new(),
                parsed: Some(ParsedFrame {
                    method: "<heartbeat>".into(),
                    args: serde_json::Value::Null,
                }),
            };
        }
        let msg: JsonValue = match serde_json::from_slice(content) {
            Ok(v) => v,
            Err(e) => {
                // Not all WS frames on tenhou.net are JSON game messages
                // (lobby chat, pings, etc.). Drop silently at warn level so
                // the log isn't deafening.
                warn!(target: "akagi::bridge::tenhou", "non-JSON frame ignored: {e}");
                return ParseResult::empty();
            }
        };
        if !msg.is_object() {
            return ParseResult::empty();
        }
        let tag = msg
            .get("tag")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        let parsed = Some(ParsedFrame {
            method: tag,
            args: msg.clone(),
        });
        let events = self.dispatch(&msg);
        self.write_mjai(&events);
        // Mirror the freshly-applied hand + window to autoplay. Done for every
        // dispatched frame, not just the ones that produced mjai events: a
        // frame that only closes the decision window still has to be seen.
        self.publish();
        ParseResult { events, parsed }
    }

    /// Encode a bot action as the Tenhou client frame that performs it.
    ///
    /// Resolves tile *indices* against the hand this bridge tracks, so it is
    /// only meaningful for our own seat. Returns `None` for events with no
    /// client frame and for any action naming a tile we do not hold — see
    /// [`encode::encode`].
    fn build(&mut self, command: &MjaiEvent) -> Option<Vec<u8>> {
        encode::encode(command, self.state.hand_view()).map(String::into_bytes)
    }
}

// ============================================================================
// Tag parsing helpers
// ============================================================================

/// Tsumo tags are `T<n>` / `U<n>` / `V<n>` / `W<n>` for relative seats 0..=3.
fn tsumo_actor(tag: &str) -> Option<u8> {
    let mut bytes = tag.bytes();
    let first = bytes.next()?;
    let rel = match first {
        b'T' => 0,
        b'U' => 1,
        b'V' => 2,
        b'W' => 3,
        _ => return None,
    };
    if !bytes.all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(rel)
}

/// Dahai tags are `D/E/F/G<n>` (uppercase = tedashi, a tile from the hand)
/// or `d/e/f/g<n>` (lowercase = tsumogiri of the just-drawn tile). Returns
/// `(rel_actor, tsumogiri)`.
///
/// This is the case convention of the Tenhou wire protocol (tenhou-python-bot
/// documents `<e21/>` as tsumogiri and `<E21/>` as a discard from the hand).
/// The original Python Akagi bridge read it the other way round and this port
/// inherited that, so every other seat's river reached the model with its
/// tedashi/tsumogiri channel inverted (#281). Our own discards never depend on
/// the case: `on_dahai` compares the tile against our last draw instead.
fn dahai_actor(tag: &str) -> Option<(u8, bool)> {
    let mut bytes = tag.bytes();
    let first = bytes.next()?;
    let (rel, tsumogiri) = match first {
        b'D' => (0, false),
        b'E' => (1, false),
        b'F' => (2, false),
        b'G' => (3, false),
        b'd' => (0, true),
        b'e' => (1, true),
        b'f' => (2, true),
        b'g' => (3, true),
        _ => return None,
    };
    if !bytes.all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((rel, tsumogiri))
}

/// Parse `tag[skip..]` as a `u32`. Returns `None` if there is no digit suffix.
fn parse_tail_u32(tag: &str, skip: usize) -> Option<u32> {
    if tag.len() <= skip {
        return None;
    }
    tag[skip..].parse::<u32>().ok()
}

fn parse_u8(msg: &JsonValue, key: &str) -> Option<u8> {
    msg.get(key).and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse::<u8>().ok())
            .or_else(|| v.as_u64().map(|n| n as u8))
    })
}

fn parse_u32(msg: &JsonValue, key: &str) -> Option<u32> {
    msg.get(key).and_then(|v| {
        v.as_str()
            .and_then(|s| s.parse::<u32>().ok())
            .or_else(|| v.as_u64().map(|n| n as u32))
    })
}

fn parse_csv_i32(msg: &JsonValue, key: &str) -> Vec<i32> {
    msg.get(key)
        .and_then(JsonValue::as_str)
        .map(|s| {
            s.split(',')
                .filter_map(|t| t.trim().parse::<i32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn parse_csv_u32(msg: &JsonValue, key: &str) -> Vec<u32> {
    msg.get(key)
        .and_then(JsonValue::as_str)
        .map(|s| {
            s.split(',')
                .filter_map(|t| t.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// `dorahaiUra` is a CSV of tile indices. Returns the corresponding mjai tiles.
fn parse_tile_csv(v: &JsonValue) -> Option<Vec<String>> {
    let s = v.as_str()?;
    let tiles: Vec<String> = s
        .split(',')
        .filter_map(|t| t.trim().parse::<u32>().ok())
        .map(tenhou_to_mjai_one)
        .collect();
    if tiles.is_empty() {
        None
    } else {
        Some(tiles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::GameEndReason;

    fn parse_one(b: &mut TenhouBridge, json: &str) -> Vec<MjaiEvent> {
        b.parse(Direction::Down, json.as_bytes()).events
    }

    #[test]
    fn ignores_up_direction() {
        let mut b = TenhouBridge::new(None, None);
        let out = b.parse(Direction::Up, br#"{"tag":"INIT"}"#);
        assert!(out.events.is_empty());
        assert!(out.parsed.is_none());
    }

    #[test]
    fn heartbeat_yields_no_events_but_visible_in_inspector() {
        let mut b = TenhouBridge::new(None, None);
        let out = b.parse(Direction::Down, b"<Z/>");
        assert!(out.events.is_empty());
        // Heartbeat is surfaced as parsed for inspector visibility.
        assert!(out.parsed.is_some());
        assert_eq!(out.parsed.unwrap().method, "<heartbeat>");
    }

    #[test]
    fn parsed_view_carries_tag_and_args() {
        let mut b = TenhouBridge::new(None, None);
        let frame = br#"{"tag":"INIT","seed":"1,0,0,2,5,134","ten":"250,250,250,250"}"#;
        let out = b.parse(Direction::Down, frame);
        let parsed = out.parsed.expect("INIT should produce parsed view");
        assert_eq!(parsed.method, "INIT");
        assert_eq!(parsed.args["seed"], "1,0,0,2,5,134");
    }

    #[test]
    fn malformed_json_does_not_panic() {
        let mut b = TenhouBridge::new(None, None);
        let out = b.parse(Direction::Down, b"not json");
        assert!(out.events.is_empty());
        assert!(out.parsed.is_none());
    }

    #[test]
    fn taikyoku_resolves_seat_but_defers_start_game() {
        let mut b = TenhouBridge::new(None, None);
        // TAIKYOKU now only resolves seat and primes the pending flag.
        // start_game is emitted at the first INIT (when sanma is known).
        let events = parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#);
        assert!(events.is_empty(), "start_game must be deferred to INIT");
    }

    /// GO / UN / TAIKYOKU metadata surfaces on `start_game`: real roster
    /// names (percent-decoded, remapped from wire-relative to wire-absolute
    /// seats) plus `MatchInfo::Tenhou` with the room bitfield, lobby and
    /// paifu id.
    #[test]
    fn go_un_taikyoku_metadata_reaches_start_game() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"GO","type":"169","lobby":"0"}"#);
        // n0 is *us* (wire-relative); "%E3%81%82" decodes to "あ".
        parse_one(
            &mut b,
            r#"{"tag":"UN","n0":"%E3%81%82","n1":"bob","n2":"carol","n3":"dave","dan":"16,15,14,13","rate":"2100.00,2000.00,1900.00,1800.00"}"#,
        );
        // Dealer at rel 1 → our wire-abs seat = (4-1)%4 = 3.
        parse_one(
            &mut b,
            r#"{"tag":"TAIKYOKU","oya":"1","log":"2026082300gm-00a9-0000-deadbeef"}"#,
        );
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"1","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                names,
                game_meta,
                ..
            } => {
                assert_eq!(*id, Some(3));
                // rel [us, shimocha, toimen, kamicha] → abs [3, 0, 1, 2].
                assert_eq!(
                    names,
                    &vec![
                        "bob".to_string(),
                        "carol".to_string(),
                        "dave".to_string(),
                        "あ".to_string(),
                    ]
                );
                let meta = game_meta.as_ref().expect("tenhou game meta");
                match &meta.match_info {
                    Some(MatchInfo::Tenhou {
                        log_id,
                        go_type,
                        lobby,
                    }) => {
                        assert_eq!(log_id.as_deref(), Some("2026082300gm-00a9-0000-deadbeef"));
                        assert_eq!(*go_type, Some(169));
                        assert_eq!(*lobby, Some(0));
                    }
                    other => panic!("expected Tenhou match_info, got {other:?}"),
                }
            }
            other => panic!("expected StartGame first, got {other:?}"),
        }
    }

    /// A reconnect `<UN/>` carries a single player's name — it must not
    /// clobber a full roster (or install a mostly-empty one).
    #[test]
    fn partial_un_does_not_replace_roster() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(
            &mut b,
            r#"{"tag":"UN","n0":"alice","n1":"bob","n2":"carol","n3":"dave"}"#,
        );
        // A different name in the partial update proves the guard rejected
        // the whole message rather than merging the one field.
        parse_one(&mut b, r#"{"tag":"UN","n2":"INTRUDER"}"#);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        match &events[0] {
            MjaiEvent::StartGame { names, .. } => {
                // oya rel 0 → our seat 0, so rel order == abs order here.
                assert_eq!(
                    names,
                    &vec![
                        "alice".to_string(),
                        "bob".to_string(),
                        "carol".to_string(),
                        "dave".to_string(),
                    ]
                );
            }
            other => panic!("expected StartGame first, got {other:?}"),
        }
    }

    /// Sanma roster: the wire stays 4-positional and relative, so the ghost
    /// slot's empty name sits at the ghost's *relative* index (here rel 1:
    /// our seat is 2, ghost wire-abs is 3). Real names land on their real
    /// seats and the emitted vector stays length 3.
    #[test]
    fn sanma_un_names_skip_the_ghost_slot() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(
            &mut b,
            r#"{"tag":"UN","n0":"alice","n1":"","n2":"bob","n3":"carol"}"#,
        );
        // Dealer at rel 2 → our wire-abs = (4-2)%4 = 2.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"2"}"#);
        // 0-score slot at E1H0 marks sanma; like the UN names, `ten` is
        // relative, so the ghost's 0 sits at rel 1 (wire-abs 3) here.
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"350,0,350,350","oya":"2","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        match &events[0] {
            MjaiEvent::StartGame {
                names, num_players, ..
            } => {
                assert_eq!(*num_players, 3);
                // rel→abs with seat 2: rel0→2 (alice), rel1→3 ghost (empty,
                // skipped), rel2→0 (bob), rel3→1 (carol).
                assert_eq!(
                    names,
                    &vec!["bob".to_string(), "carol".to_string(), "alice".to_string()]
                );
            }
            other => panic!("expected StartGame first, got {other:?}"),
        }
        // The ghost's rel-1 zero is skipped; all three real seats carry
        // their 35000.
        match &events[1] {
            MjaiEvent::StartKyoku { scores, .. } => {
                assert_eq!(scores, &vec![35000, 35000, 35000]);
            }
            other => panic!("expected StartKyoku second, got {other:?}"),
        }
    }

    #[test]
    fn init_emits_start_game_then_start_kyoku_yonma() {
        let mut b = TenhouBridge::new(None, None);
        // Dealer at rel 1 → our wire-abs = (4-1)%4 = 3.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#);
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"1","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        assert_eq!(
            events.len(),
            2,
            "yonma first INIT emits start_game + start_kyoku"
        );
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                num_players,
                names,
                ..
            } => {
                assert_eq!(*id, Some(3));
                assert_eq!(*num_players, 4);
                assert_eq!(names.len(), 4);
            }
            other => panic!("expected StartGame first, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::StartKyoku {
                bakaze,
                kyoku,
                honba,
                kyotaku,
                oya,
                dora_marker,
                scores,
                tehais,
                num_players,
            } => {
                assert_eq!(bakaze, "E");
                assert_eq!(*kyoku, 1);
                assert_eq!(*honba, 0);
                assert_eq!(*kyotaku, 0);
                assert_eq!(*oya, 0, "dealer at rel 1 from our seat 3 → wire-abs 0");
                assert_eq!(dora_marker, "2m");
                assert_eq!(scores, &vec![25000; 4]);
                assert_eq!(tehais.len(), 4);
                assert_eq!(tehais[3].len(), 13);
                assert_eq!(*num_players, 4);
                // Our hand lands at our wire-abs seat (3).
                assert_ne!(tehais[3], vec!["?".to_string(); 13]);
                for (i, hand) in tehais.iter().enumerate() {
                    if i == 3 {
                        continue;
                    }
                    assert_eq!(hand, &vec!["?".to_string(); 13]);
                }
            }
            other => panic!("expected StartKyoku second, got {other:?}"),
        }
    }

    /// Second INIT in the same game must NOT re-emit start_game.
    #[test]
    fn second_init_does_not_repeat_start_game() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        let init_e1 = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let e1 = parse_one(&mut b, init_e1);
        assert_eq!(e1.len(), 2); // start_game + start_kyoku
        let init_e2 = r#"{"tag":"INIT","seed":"1,0,0,1,2,4","ten":"250,250,250,250","oya":"1","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let e2 = parse_one(&mut b, init_e2);
        assert_eq!(e2.len(), 1, "second INIT emits only start_kyoku");
        assert!(matches!(e2[0], MjaiEvent::StartKyoku { .. }));
    }

    /// Regression for issue #107: Tenhou's `ten` field is in relative-seat
    /// order (rel 0 is us), but `start_kyoku.scores` is keyed by absolute seat.
    /// Before the fix, the yonma INIT path skipped the rel→abs remap, so when
    /// our absolute seat ≠ 0 the scores were cyclically rotated. The user
    /// observed it as "self always shown as player 1, shimocha/toimen/kamicha
    /// as players 2/3/4 regardless of actual table position."
    #[test]
    fn init_yonma_remaps_scores_rel_to_abs() {
        let mut b = TenhouBridge::new(None, None);
        // TAIKYOKU oya=1 → our absolute seat = (4-1)%4 = 3.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#);
        // ten in relative order: us=100, shimocha=200, toimen=300, kamicha=400.
        // Absolute mapping: rel 0 (us, abs 3), rel 1 (abs 0), rel 2 (abs 1),
        // rel 3 (abs 2). Expected scores[abs] in 100-yen units × 100.
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"100,200,300,400","oya":"1","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        // First INIT emits start_game + start_kyoku; the kyoku payload is at [1].
        let start_kyoku = events
            .iter()
            .find_map(|e| match e {
                MjaiEvent::StartKyoku { scores, oya, .. } => Some((scores.clone(), *oya)),
                _ => None,
            })
            .expect("start_kyoku emitted");
        assert_eq!(start_kyoku.1, 0, "dealer at rel 1 from our seat 3 → abs 0");
        assert_eq!(
            start_kyoku.0,
            vec![20_000, 30_000, 40_000, 10_000],
            "scores must be in absolute-seat order, not relative",
        );
    }

    #[test]
    fn init_detects_sanma_via_zero_score() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        // 4-element ten with one slot 0 indicates sanma.
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"350,350,350,0","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        // start_game must also be downgraded to 3 players.
        let sg_num_players = events
            .iter()
            .find_map(|e| match e {
                MjaiEvent::StartGame { num_players, .. } => Some(*num_players),
                _ => None,
            })
            .expect("start_game emitted alongside first INIT");
        assert_eq!(
            sg_num_players, 3,
            "start_game.num_players must reflect sanma"
        );
        let sk = events
            .iter()
            .find_map(|e| match e {
                MjaiEvent::StartKyoku {
                    num_players,
                    scores,
                    tehais,
                    ..
                } => Some((*num_players, scores.len(), tehais.len())),
                _ => None,
            })
            .expect("start_kyoku emitted");
        assert_eq!(sk, (3, 3, 3));
    }

    /// Sanma mid-game: a real player can legitimately have 0 points
    /// (bust-but-continuing rule, or transient 0 between deltas). The
    /// rel→abs remap must not conflate that with the ghost slot at
    /// relative index 3 — otherwise the real player gets silently
    /// dropped. We exercise the post-detection sanma INIT directly by
    /// seeding `is_3p` via an initial E1H0 frame, then issuing an E2
    /// INIT where rel 1 (a real player) holds 0 points alongside the
    /// rel-3 ghost slot which also holds 0.
    #[test]
    fn init_sanma_preserves_real_zero_point_player() {
        let mut b = TenhouBridge::new(None, None);
        // Our seat = 0 (oya=0). E1H0 with a 0 slot triggers sanma detection.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"350,350,350,0","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#,
        );
        // E2 INIT mid-game: rel 1 player (abs 1) is at 0 points, ghost at rel 3.
        // ten in 100-yen units: us=400, rel1=0 (real), rel2=300, rel3=0 (ghost).
        let init = r#"{"tag":"INIT","seed":"1,0,0,1,2,4","ten":"400,0,300,0","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        match &events[0] {
            MjaiEvent::StartKyoku {
                scores,
                num_players,
                ..
            } => {
                assert_eq!(*num_players, 3);
                assert_eq!(
                    scores,
                    &vec![40_000, 0, 30_000],
                    "real-player-at-0 must survive the ghost-slot remap",
                );
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }

    #[test]
    fn tsumo_for_self_reveals_tile() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        let events = parse_one(&mut b, r#"{"tag":"T16"}"#);
        match &events[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "5mr"); // index 16 = red 5m
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    #[test]
    fn tsumo_for_other_player_is_unknown() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        let events = parse_one(&mut b, r#"{"tag":"U99"}"#);
        match &events[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "?");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Bridge at our seat 0 (dealer), one kyoku started, ready for discards.
    fn bridge_in_kyoku() -> TenhouBridge {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        b
    }

    fn other_dahai(b: &mut TenhouBridge, tag: &str) -> (u8, String, bool) {
        let events = parse_one(b, &format!(r#"{{"tag":"{tag}"}}"#));
        match &events[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => (*actor, pai.clone(), *tsumogiri),
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    #[test]
    fn dahai_other_player_uppercase_is_tedashi() {
        let mut b = bridge_in_kyoku();
        // Uppercase E -> rel=1, discard from the hand.
        let (actor, pai, tsumogiri) = other_dahai(&mut b, "E40");
        assert_eq!(actor, 1);
        // index 40 / 4 = 10 → 2p (pin block starts at type 9 = 1p).
        assert_eq!(pai, "2p");
        assert!(!tsumogiri, "uppercase tag is a tedashi");
    }

    /// Regression for #281: Tenhou marks a tsumogiri with a *lowercase* tag
    /// (`<e21/>`) and a tedashi with uppercase (`<E21/>`). The bridge had the
    /// two swapped for other seats, so every opponent river reached the model
    /// with its tedashi/tsumogiri channel inverted and live suggestions
    /// drifted from a review of the same game.
    #[test]
    fn dahai_other_player_lowercase_is_tsumogiri() {
        let mut b = bridge_in_kyoku();
        let (actor, pai, tsumogiri) = other_dahai(&mut b, "e40");
        assert_eq!(actor, 1);
        assert_eq!(pai, "2p");
        assert!(tsumogiri, "lowercase tag is a tsumogiri");
    }

    /// Regression for #281, in the shape that exposed it: after a riichi every
    /// discard is a forced tsumogiri, and Tenhou sends those lowercase. Before
    /// the fix the whole post-riichi river came out as `tsumogiri: false`.
    #[test]
    fn other_seat_discards_after_riichi_are_tsumogiri() {
        let mut b = bridge_in_kyoku();
        // Seat 1 draws, declares riichi and discards from the hand (uppercase).
        parse_one(&mut b, r#"{"tag":"U"}"#);
        parse_one(&mut b, r#"{"tag":"REACH","who":"1","step":"1"}"#);
        let (_, _, declared) = other_dahai(&mut b, "E40");
        assert!(!declared, "riichi declaration tile was a tedashi");
        parse_one(
            &mut b,
            r#"{"tag":"REACH","who":"1","step":"2","ten":"250,240,250,250"}"#,
        );
        // Every later discard of the riichi player is a tsumogiri (lowercase).
        for tag in ["e44", "e48", "e52"] {
            parse_one(&mut b, r#"{"tag":"U"}"#);
            let (actor, _, tsumogiri) = other_dahai(&mut b, tag);
            assert_eq!(actor, 1);
            assert!(tsumogiri, "{tag}: post-riichi discard must be a tsumogiri");
        }
    }

    /// Regression: the first discard after our own call is a tedashi, never a
    /// tsumogiri. A meld removes tiles from the *middle* of `state.hand`, so
    /// the tail still holds the tile we drew on the previous turn — comparing
    /// the discard against it (without checking `is_tsumo`) reported the
    /// tedashi as tsumogiri.
    #[test]
    fn dahai_after_our_pon_is_tedashi_not_tsumogiri() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        // Hand holds two 1m (indices 0, 1) so we can pon a third.
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,4,5,8,9,12,16,20,24,28,32,36"}"#,
        );

        // Our turn: draw 2p (index 40), then tedashi the 1p (index 36). The
        // drawn 2p stays at the tail of `hand`.
        parse_one(&mut b, r#"{"tag":"T40"}"#);
        let tedashi = parse_one(&mut b, r#"{"tag":"D36"}"#);
        match &tedashi[0] {
            MjaiEvent::Dahai { pai, tsumogiri, .. } => {
                assert_eq!(pai, "1p");
                assert!(!*tsumogiri, "drew 2p but discarded 1p");
            }
            other => panic!("expected Dahai, got {other:?}"),
        }

        // Kamicha (rel 3 → abs 3) discards a 1m (index 2).
        parse_one(&mut b, r#"{"tag":"g2"}"#);

        // We pon it. m = 1131: pon marker (bit 3), target_rel 3 (kamicha),
        // tile type 1m, unused = the 4th copy (index 3), called tile at r = 2.
        let pon = parse_one(&mut b, r#"{"tag":"N","who":"0","m":"1131"}"#);
        match &pon[0] {
            MjaiEvent::Pon {
                actor,
                target,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(*target, 3, "called off kamicha");
                assert_eq!(pai, "1m");
                assert_eq!(consumed, &["1m".to_string(), "1m".to_string()]);
            }
            other => panic!("expected Pon, got {other:?}"),
        }

        // Now discard the 2p we drew *last* turn. It is still the tail of
        // `hand` (the pon removed the two 1m from the middle), but we did not
        // draw this turn, so this is a tedashi.
        let after_pon = parse_one(&mut b, r#"{"tag":"D40"}"#);
        match &after_pon[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "2p");
                assert!(!*tsumogiri, "no draw since the pon — must be tedashi");
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    /// The `is_tsumo` gate must not break the ordinary case: after ankan the
    /// rinshan draw re-arms it, so tsumogiri of the replacement tile still
    /// reports true.
    #[test]
    fn dahai_of_rinshan_draw_after_ankan_is_tsumogiri() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        // Four 5m (16..19, 16 is the red) so we can ankan them.
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"16,17,18,19,0,4,8,12,20,24,28,32,36"}"#,
        );
        parse_one(&mut b, r#"{"tag":"T40"}"#); // draw 2p

        // Ankan of 5m: hai0 = 16, target 0 → m = 16 << 8 = 4096.
        let kan = parse_one(&mut b, r#"{"tag":"N","who":"0","m":"4096"}"#);
        assert!(matches!(kan[0], MjaiEvent::Ankan { actor: 0, .. }));

        // Rinshan draw re-arms is_tsumo, so discarding it is a tsumogiri.
        parse_one(&mut b, r#"{"tag":"T44"}"#); // draw 3p
        let discard = parse_one(&mut b, r#"{"tag":"D44"}"#);
        match &discard[0] {
            MjaiEvent::Dahai { pai, tsumogiri, .. } => {
                assert_eq!(pai, "3p");
                assert!(*tsumogiri, "discarded the rinshan tile we just drew");
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    /// Regression (chankan): an opponent's kakan can be robbed, and the
    /// server marks it exactly as it marks a claimable discard — a `t`
    /// bitmask on the frame. Dropping it left autoplay with no window, so
    /// the ron (or the pass that keeps the clock from running out) was
    /// never pressed.
    #[test]
    fn an_opponents_kakan_with_t_opens_a_ron_window() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,4,8,12,16,20,24,28,32,36,40,44,48"}"#,
        );
        // Shimocha adds to a pon of 1m (crafted kakan m = 16) and we may rob
        // it: the frame carries t = 8 (ron).
        let kakan = parse_one(&mut b, r#"{"tag":"N","who":"1","m":"16","t":"8"}"#);
        assert!(matches!(kakan[0], MjaiEvent::Kakan { actor: 1, .. }));
        let w = b.state.window.expect("chankan must open a window");
        assert!(w.allows(crate::autoplay::tenhou_state::OP_RON));
        assert!(w.has_declinable_claim(), "a chankan pass is a real decline");
    }

    /// A kakan we cannot rob carries no `t`, and must leave no window: a
    /// stale one would let a later bot reply act into the caller's turn.
    #[test]
    fn an_opponents_kakan_without_t_opens_nothing() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,4,8,12,16,20,24,28,32,36,40,44,48"}"#,
        );
        // Open a claim window first so the kakan has something to clear.
        parse_one(&mut b, r#"{"tag":"e50","t":"1"}"#);
        assert!(b.state.window.is_some());
        parse_one(&mut b, r#"{"tag":"N","who":"1","m":"16"}"#);
        assert!(b.state.window.is_none(), "no t, no claim");
    }

    /// Our own kakan never opens a window off its own frame — the rinshan
    /// draw that follows opens the real one.
    #[test]
    fn our_own_kakan_opens_no_window_even_with_t() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,4,8,12,16,20,24,28,32,36,40,44,48"}"#,
        );
        parse_one(&mut b, r#"{"tag":"N","who":"0","m":"16","t":"8"}"#);
        assert!(b.state.window.is_none());
    }

    #[test]
    fn agari_emits_full_hora_then_endkyoku() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        // Tsumo win: who=0, fromWho=0; sc deltas distribute points.
        let events = parse_one(
            &mut b,
            r#"{"tag":"AGARI","who":"0","fromWho":"0","sc":"250,40,250,-10,250,-10,250,-20"}"#,
        );
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ..
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(*target, 0);
                let d = deltas.as_ref().unwrap();
                assert_eq!(d.len(), 4);
                assert_eq!(d[0], 4000);
            }
            other => panic!("expected Hora, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    #[test]
    fn ryukyoku_emits_deltas_then_endkyoku() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        let events = parse_one(
            &mut b,
            r#"{"tag":"RYUUKYOKU","type":"yao9","sc":"250,15,250,-5,250,-5,250,-5"}"#,
        );
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                let d = deltas.as_ref().unwrap();
                assert_eq!(d.len(), 4);
                assert_eq!(d[0], 1500);
            }
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    #[test]
    fn agari_with_owari_appends_endgame() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        let events = parse_one(
            &mut b,
            r#"{"tag":"AGARI","who":"0","fromWho":"0","sc":"250,40,250,-10,250,-10,250,-20","owari":"290,30,240,-10,240,-10,230,-20"}"#,
        );
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], MjaiEvent::EndGame { .. }));
    }

    /// Regression: Tenhou's WebSocket JSON spells the discarder seat `fromWho`
    /// (camelCase). The bridge previously read `"fromwho"` (all-lowercase),
    /// so every ron silently fell back to `from_rel = actor_rel`, mis-emitting
    /// `hora.target = winner` (i.e. presenting a ron as a tsumo). Frame
    /// captured from log `20260512-135922/inspector.jsonl:1106`.
    #[test]
    fn agari_ron_from_capital_w_fromwho_resolves_to_discarder() {
        let mut b = TenhouBridge::new(None, None);
        // TAIKYOKU oya=0 → our absolute seat = (4-0)%4 = 0. Winner=0 (us),
        // discarder=2 (kamicha across). Both arrive as *relative* seats in
        // the AGARI frame; rel_to_abs is identity here because our seat is 0.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,2,0,3,4,71","ten":"360,280,280,280","oya":"0","hai":"133,56,14,44,103,10,42,0,122,124,70,117,61"}"#,
        );
        let frame = r#"{"tag":"AGARI","ba":"2,1","doraHai":"71","doraHaiUra":"45","fromWho":"2","hai":"10,14,16,39,42,44,53,56,61,100,101,103,132,133","machi":"16","sc":"350,136,280,0,280,-126,280,0","ten":"40,12000,1","who":"0","yaku":"1,1,2,1,52,1,54,1,53,0"}"#;
        let events = parse_one(&mut b, frame);
        let hora = events
            .iter()
            .find_map(|e| match e {
                MjaiEvent::Hora {
                    actor,
                    target,
                    deltas,
                    ura_markers,
                } => Some((*actor, *target, deltas.clone(), ura_markers.clone())),
                _ => None,
            })
            .expect("hora event emitted");
        assert_eq!(hora.0, 0, "winner is seat 0");
        assert_eq!(
            hora.1, 2,
            "ron target is seat 2 — fromWho must parse case-sensitively"
        );
        // Ura dora indicator field is also camelCase; verify it survives the
        // same casing fix. Tile id 45 / 4 = 11 → 3p in 34-space; mjai's
        // string for `id % 4` variants of 3p is "3p".
        assert_eq!(
            hora.3
                .as_deref()
                .map(|v| v.iter().map(String::as_str).collect::<Vec<_>>()),
            Some(vec!["3p"]),
            "doraHaiUra must parse a single-marker CSV",
        );
        // Deltas are sc[1], sc[3], sc[5], sc[7] × 100, re-keyed to absolute
        // seats. Seat 0 wins 13600, seat 2 pays 12600 (the actual user log).
        let d = hora.2.expect("deltas present");
        assert_eq!(d, vec![13_600, 0, -12_600, 0]);
    }

    #[test]
    fn reach_step_one_then_two() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        let e1 = parse_one(&mut b, r#"{"tag":"REACH","who":"0","step":"1"}"#);
        assert!(matches!(e1[0], MjaiEvent::Reach { actor: 0, .. }));
        let e2 = parse_one(
            &mut b,
            r#"{"tag":"REACH","who":"0","step":"2","ten":"240,250,250,260"}"#,
        );
        assert!(matches!(e2[0], MjaiEvent::ReachAccepted { actor: 0 }));
    }

    /// Regression: real frames captured 2026-05-01 from tenhou.net. The user
    /// is at relative seat 0 (always); their absolute seat is `(4 - oya) % 4`.
    /// The hand arrives under the bare key `hai`, never `hai{seat}` — the
    /// legacy XML docstring was misleading.
    #[test]
    fn captured_init_hand_uses_bare_hai_key() {
        let mut b = TenhouBridge::new(None, None);
        // TAIKYOKU oya=3 → our absolute seat = (4-3)%4 = 1.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"3"}"#);
        let init = r#"{"tag":"INIT","seed":"0,0,0,0,3,101","ten":"250,250,250,250","oya":"3","hai":"65,108,40,123,61,67,32,134,120,132,78,52,91"}"#;
        let events = parse_one(&mut b, init);
        let sk = events
            .iter()
            .find_map(|e| match e {
                MjaiEvent::StartKyoku { tehais, oya, .. } => Some((tehais.clone(), *oya)),
                _ => None,
            })
            .expect("start_kyoku emitted");
        // E1 dealer at rel seat 3 → abs seat 0.
        assert_eq!(sk.1, 0);
        let tehais = sk.0;
        assert_eq!(tehais.len(), 4);
        assert_eq!(tehais[1].len(), 13);
        assert_ne!(tehais[1], vec!["?".to_string(); 13]);
        // First tile is index 65 → 65/4 = 16 → 8p.
        assert_eq!(tehais[1][0], "8p");
        for (i, hand) in tehais.iter().enumerate() {
            if i == 1 {
                continue;
            }
            assert_eq!(hand, &vec!["?".to_string(); 13]);
        }
    }

    #[test]
    fn dora_emits_marker() {
        let mut b = TenhouBridge::new(None, None);
        let events = parse_one(&mut b, r#"{"tag":"DORA","hai":"108"}"#);
        match &events[0] {
            MjaiEvent::Dora { dora_marker } => assert_eq!(dora_marker, "E"),
            other => panic!("expected Dora, got {other:?}"),
        }
    }

    /// Regression for issue #113: captured 2026-05-12 sanma game where the
    /// player sits at wire-abs 1 (South). Before the fix, `rel_to_abs` used
    /// `% 3` and collapsed wire-rel 0 (us) with wire-rel 3 (dealer/kamicha)
    /// onto the same mjai actor; the ghost slot was conflated with a real
    /// player; and `start_game.num_players` stayed at 4 because sanma
    /// detection happened only at INIT (after emission).
    ///
    /// Raw frames preserved verbatim:
    /// ```
    /// {"tag":"TAIKYOKU","oya":"3"}
    /// {"tag":"INIT","seed":"0,0,0,0,0,85","ten":"350,350,0,350","oya":"3","hai":"67,86,75,116,104,73,48,51,58,127,54,44,125"}
    /// {"tag":"W"}     <- wire-rel 3 (East dealer) tsumo, tile hidden from us
    /// {"tag":"g56"}   <- wire-rel 3 dahai of tile 56 (6p)
    /// {"tag":"T41"}   <- wire-rel 0 (us) tsumo of tile 41 (2p)
    /// {"tag":"D116"}  <- wire-rel 0 dahai of tile 116 (W)
    /// {"tag":"U"}     <- wire-rel 1 (West) tsumo, hidden
    /// ```
    #[test]
    fn captured_sanma_game_seat_one_assigns_correct_actors() {
        let mut b = TenhouBridge::new(None, None);
        let taikyoku = parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"3"}"#);
        assert!(taikyoku.is_empty(), "start_game deferred until INIT");

        let init = parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,0,0,85","ten":"350,350,0,350","oya":"3","hai":"67,86,75,116,104,73,48,51,58,127,54,44,125"}"#,
        );
        // start_game (deferred from TAIKYOKU) + start_kyoku.
        assert_eq!(init.len(), 2);
        let sg = match &init[0] {
            MjaiEvent::StartGame {
                id,
                num_players,
                names,
                ..
            } => (*id, *num_players, names.len()),
            other => panic!("expected StartGame first, got {other:?}"),
        };
        assert_eq!(
            sg,
            (Some(1), 3, 3),
            "sanma start_game with id=1 (wire-abs of South seat) and num_players=3"
        );

        let sk = match &init[1] {
            MjaiEvent::StartKyoku {
                bakaze,
                kyoku,
                oya,
                scores,
                tehais,
                num_players,
                ..
            } => (
                bakaze.clone(),
                *kyoku,
                *oya,
                scores.clone(),
                tehais.len(),
                tehais.iter().map(|h| h.len()).collect::<Vec<_>>(),
                *num_players,
            ),
            other => panic!("expected StartKyoku second, got {other:?}"),
        };
        assert_eq!(sk.0, "E");
        assert_eq!(sk.1, 1);
        assert_eq!(
            sk.2, 0,
            "E1 dealer at wire-abs 0 (East), not collapsed onto our seat"
        );
        assert_eq!(
            sk.3,
            vec![35_000, 35_000, 35_000],
            "all three real players start at 35000; ghost slot dropped",
        );
        assert_eq!(sk.4, 3);
        // Our hand sits at mjai-abs 1, others are placeholders.
        for hand_len in &sk.5 {
            assert_eq!(*hand_len, 13);
        }
        assert_eq!(sk.6, 3);

        // Dealer (wire-rel 3 from us) tsumo — must map to mjai actor 0.
        let w = parse_one(&mut b, r#"{"tag":"W"}"#);
        match &w[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 0, "wire-rel 3 with seat 1 → wire-abs 0 = dealer");
                assert_eq!(pai, "?", "we don't see other players' draws");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }

        // Dealer's dahai of tile 56 (6p), tsumogiri (lowercase tag).
        let g56 = parse_one(&mut b, r#"{"tag":"g56"}"#);
        match &g56[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "6p");
                assert!(*tsumogiri, "lowercase g → tsumogiri");
            }
            other => panic!("expected Dahai, got {other:?}"),
        }

        // Our own tsumo (T41 → 2p).
        let t41 = parse_one(&mut b, r#"{"tag":"T41"}"#);
        match &t41[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 1, "wire-rel 0 = us = wire-abs 1");
                assert_eq!(pai, "2p");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }

        // Our dahai D116 (W). Uppercase D, but we drew 2p — different tile,
        // so tsumogiri must resolve to false.
        let d116 = parse_one(&mut b, r#"{"tag":"D116"}"#);
        match &d116[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "W");
                assert!(!*tsumogiri, "drew 2p but discarded W → not tsumogiri");
            }
            other => panic!("expected Dahai, got {other:?}"),
        }

        // West player (wire-rel 1 = wire-abs 2) tsumo, hidden.
        let u = parse_one(&mut b, r#"{"tag":"U"}"#);
        match &u[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 2, "wire-rel 1 with seat 1 → wire-abs 2");
                assert_eq!(pai, "?");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Yonma INIT.oya from a non-East perspective: dealer at relative seat 3
    /// from us must resolve to wire-abs 0 (the East-1 dealer), exercising
    /// `rel_to_abs(3) = (3 + seat) % 4` where seat=1.
    #[test]
    fn yonma_init_oya_three_with_seat_one_resolves_to_zero() {
        let mut b = TenhouBridge::new(None, None);
        // TAIKYOKU oya=3 → seat=(4-3)%4=1.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"3"}"#);
        let init = parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,0,3,101","ten":"250,250,250,250","oya":"3","hai":"65,108,40,123,61,67,32,134,120,132,78,52,91"}"#,
        );
        let oya = init
            .iter()
            .find_map(|e| match e {
                MjaiEvent::StartKyoku { oya, .. } => Some(*oya),
                _ => None,
            })
            .expect("start_kyoku emitted");
        assert_eq!(oya, 0, "yonma E1 dealer should be at wire-abs 0");
    }

    // ------------------------------------------------------------------
    // Reconnect / mid-game attach (issue #280)
    // ------------------------------------------------------------------

    /// A hand with no 1m (indices 0..=3): a `?` draw that lands on our seat
    /// renders as 1m downstream, so its absence is the tell.
    const HAND_NO_1M: &str = "4,8,12,36,40,44,72,76,80,108,112,116,120";

    fn tehais_len(events: &[MjaiEvent]) -> usize {
        events
            .iter()
            .find_map(|e| match e {
                MjaiEvent::StartKyoku { tehais, .. } => Some(tehais.len()),
                _ => None,
            })
            .expect("start_kyoku emitted")
    }

    /// The reconnect sequence as the bridge sees it on a fresh WebSocket:
    /// no `<TAIKYOKU/>`, a `<REINIT/>` snapshot of the hand in progress, then
    /// live play. The seat comes from the kyoku frame, the rest of the hand
    /// is withheld, and the next `<INIT/>` reopens the game on the right
    /// seat with a fresh `start_game`.
    #[test]
    fn reinit_on_a_fresh_flow_resolves_seat_and_suspends_until_next_init() {
        let mut b = TenhouBridge::new(None, None);
        // E3 (seed 2): the dealer is wire-abs 2 and we see them at rel 1,
        // so we sit at wire-abs 1.
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"2,1,0,3,4,60","ten":"230,260,250,260","oya":"1","hai":"{HAND_NO_1M}","kawa0":"20,255,24","kawa1":"33","kawa2":"5","kawa3":"9"}}"#
        );
        assert!(
            parse_one(&mut b, &reinit).is_empty(),
            "REINIT emits nothing"
        );
        assert_eq!(b.state.seat, 1);
        assert!(b.state.seat_resolved);
        assert!(b.state.suspended);
        assert!(b.state.in_riichi, "255 in kawa0 marks our riichi");
        assert_eq!(b.state.hand.len(), 13);
        assert!(!b.state.is_tsumo);
        assert_eq!(
            b.state.live_wall,
            70 - 5,
            "five discards, riichi mark excluded"
        );

        // Live play for the rest of this hand is withheld, but the hand keeps
        // tracking for autoplay.
        assert!(parse_one(&mut b, r#"{"tag":"U"}"#).is_empty());
        assert!(parse_one(&mut b, r#"{"tag":"E33"}"#).is_empty());
        assert!(parse_one(&mut b, r#"{"tag":"T60","t":"16"}"#).is_empty());
        assert_eq!(b.state.hand.len(), 14);
        assert!(b.state.is_tsumo);
        assert!(b.state.window.is_some(), "autoplay still sees the window");
        assert!(parse_one(&mut b, r#"{"tag":"D60"}"#).is_empty());
        assert_eq!(b.state.hand.len(), 13);
        let agari = r#"{"tag":"AGARI","who":"2","fromWho":"1","sc":"230,0,260,-20,250,20,260,0","ba":"1,0"}"#;
        assert!(
            parse_one(&mut b, agari).is_empty(),
            "the hand's end is withheld too"
        );
        assert!(b.state.suspended);

        // E4 (seed 3): dealer wire-abs 3, at rel 2 from seat 1 — consistent.
        let init = format!(
            r#"{{"tag":"INIT","seed":"3,0,0,1,2,8","ten":"230,260,270,240","oya":"2","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert!(!b.state.suspended);
        assert_eq!(events.len(), 2, "start_game + start_kyoku, got {events:?}");
        match &events[0] {
            MjaiEvent::StartGame {
                id, num_players, ..
            } => {
                assert_eq!(*id, Some(1));
                assert_eq!(*num_players, 4);
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::StartKyoku {
                oya,
                tehais,
                scores,
                ..
            } => {
                assert_eq!(*oya, 3);
                let hand: Vec<u32> = HAND_NO_1M.split(',').map(|t| t.parse().unwrap()).collect();
                assert_eq!(tehais[1], tenhou_to_mjai(&hand));
                assert_eq!(tehais[0], vec!["?"; 13]);
                // ten is relative: rel0=us(1) 230, rel1=abs2 260, rel2=abs3 270, rel3=abs0 240.
                assert_eq!(scores, &vec![24_000, 23_000, 26_000, 27_000]);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }

        // Resumed: draws land on the right seats.
        let events = parse_one(&mut b, r#"{"tag":"W"}"#);
        assert!(matches!(&events[0], MjaiEvent::Tsumo { actor: 0, pai } if pai == "?"));
        let events = parse_one(&mut b, r#"{"tag":"T9"}"#);
        assert!(matches!(&events[0], MjaiEvent::Tsumo { actor: 1, pai } if pai == "3m"));
    }

    /// Akagi attached between hands (or the rejoin landed exactly on a kyoku
    /// boundary): the first `<INIT/>` alone must place us and open the game.
    #[test]
    fn init_on_a_fresh_flow_derives_seat_and_opens_the_game() {
        let mut b = TenhouBridge::new(None, None);
        // S2 (seed 5): dealer wire-abs 1, seen at rel 3 → we sit at 2.
        let init = format!(
            r#"{{"tag":"INIT","seed":"5,0,1,1,2,8","ten":"200,300,250,240","oya":"3","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert_eq!(b.state.seat, 2);
        assert_eq!(events.len(), 2, "{events:?}");
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame {
                id: Some(2),
                num_players: 4,
                ..
            }
        ));
        match &events[1] {
            MjaiEvent::StartKyoku {
                bakaze,
                kyoku,
                oya,
                tehais,
                scores,
                ..
            } => {
                assert_eq!(bakaze, "S");
                assert_eq!(*kyoku, 2);
                assert_eq!(*oya, 1);
                assert_eq!(tehais[2].len(), 13);
                assert_ne!(tehais[2][0], "?");
                assert_eq!(scores, &vec![25_000, 24_000, 20_000, 30_000]);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        // S3 (seed 6): we deal. Only start_kyoku now, still on seat 2.
        let init = format!(
            r#"{{"tag":"INIT","seed":"6,0,0,1,2,8","ten":"200,300,250,240","oya":"0","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], MjaiEvent::StartKyoku { oya: 2, .. }));
        assert_eq!(b.state.seat, 2);
    }

    /// TAIKYOKU remains the authority mid-game; a later kyoku frame that
    /// disagrees is a malformed frame, not a seat change. The first deal is
    /// the one frame that says the same thing TAIKYOKU does, so it wins.
    #[test]
    fn taikyoku_seat_wins_over_a_disagreeing_kyoku_frame() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#); // seat 3
                                                              // E2 (seed 1): dealer wire-abs 1 sits at rel 2 from seat 3; oya "0"
                                                              // would put us at 1.
        let init = format!(
            r#"{{"tag":"INIT","seed":"1,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert_eq!(b.state.seat, 3);
        match &events[1] {
            MjaiEvent::StartKyoku { tehais, .. } => {
                assert_ne!(tehais[3][0], "?");
                assert_eq!(tehais[0], vec!["?"; 13]);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }

        // E1H0 disagreeing with TAIKYOKU: the deal wins.
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#); // seat 3
        let init = format!(
            r#"{{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert_eq!(b.state.seat, 0);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { id: Some(0), .. }
        ));
    }

    /// REINIT on a flow that did see TAIKYOKU (same-socket re-auth, or a
    /// server that replays it): the seat is already right, and the rest of
    /// the reconnect handling is identical.
    #[test]
    fn reinit_after_taikyoku_keeps_the_taikyoku_seat() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"2"}"#); // seat 2
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"2","hai":"{HAND_NO_1M}","kawa0":"20","kawa2":"5"}}"#
        );
        assert!(parse_one(&mut b, &reinit).is_empty());
        assert_eq!(b.state.seat, 2);
        assert!(b.state.suspended);
        let init = format!(
            r#"{{"tag":"INIT","seed":"1,0,0,1,2,8","ten":"250,250,250,250","oya":"3","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { id: Some(2), .. }
        ));
        assert!(matches!(&events[1], MjaiEvent::StartKyoku { oya: 1, .. }));
    }

    /// The hand, our calls and the draw-in-hand flag come back from REINIT
    /// so autoplay can keep encoding discards for the rest of the hand.
    #[test]
    fn reinit_restores_our_calls_and_draw_in_hand() {
        use crate::autoplay::tenhou_state::new_shared;
        let shared = new_shared();
        let mut b = TenhouBridge::new(None, None).with_shared_state(Some(shared.clone()));
        // Pon of 1p (t=27 → tile type 9) from rel 1; see meld.rs bit layout.
        let pon = (27u32 << 9) | 8 | 1;
        // 10 concealed tiles + one pon = no draw in hand.
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"4,8,12,40,44,48,72,76,80,108","m0":"{pon}","kawa0":"20","kawa1":"36"}}"#
        );
        parse_one(&mut b, &reinit);
        assert_eq!(b.state.melds.len(), 1);
        assert_eq!(b.state.melds[0].kind, MeldKind::Pon);
        assert_eq!(b.state.melds[0].pai(), "1p");
        assert!(!b.state.is_tsumo);
        assert!(!b.state.in_riichi);
        let published = shared.read().unwrap().clone().expect("published");
        assert_eq!(published.seat, 0);
        assert_eq!(published.hand.len(), 10);
        assert_eq!(published.melds.len(), 1);

        // 11 concealed tiles + one pon = it is our turn, draw in hand.
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"4,8,12,40,44,48,72,76,80,108,112","m0":"{pon}","kawa0":"20","kawa1":"36"}}"#
        );
        parse_one(&mut b, &reinit);
        assert!(b.state.is_tsumo);
        assert!(shared.read().unwrap().as_ref().unwrap().is_tsumo);
    }

    /// `<GO type/>` bit 0x10 is the three-player flag. It is the only sanma
    /// signal a rejoining flow gets when the next kyoku is not E1H0.
    #[test]
    fn go_type_sanma_bit_sets_three_players_before_the_first_kyoku() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"GO","type":"25","lobby":"0"}"#); // 0x19
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        // E2 mid-game: no E1H0 check to fall back on. Ghost is rel 3 for seat 0.
        let init = r#"{"tag":"INIT","seed":"1,0,0,1,2,4","ten":"350,300,400,0","oya":"1","hai":"0,32,36,40,44,72,76,80,108,112,116,120,124"}"#;
        let events = parse_one(&mut b, init);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { num_players: 3, .. }
        ));
        match &events[1] {
            MjaiEvent::StartKyoku { scores, oya, .. } => {
                assert_eq!(scores, &vec![35_000, 30_000, 40_000]);
                assert_eq!(*oya, 1);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        assert_eq!(tehais_len(&events), 3);
    }

    /// Without `<GO/>`, a roster with one empty slot says sanma.
    #[test]
    fn un_roster_with_an_empty_slot_marks_sanma() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(
            &mut b,
            r#"{"tag":"UN","n0":"alice","n1":"","n2":"bob","n3":"carol"}"#,
        );
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"2"}"#); // seat 2, ghost at rel 1
                                                              // E2 (seed 1): dealer wire-abs 1 sits at rel 3 from seat 2.
        let init = r#"{"tag":"INIT","seed":"1,0,0,1,2,4","ten":"350,0,300,400","oya":"3","hai":"0,32,36,40,44,72,76,80,108,112,116,120,124"}"#;
        let events = parse_one(&mut b, init);
        match &events[0] {
            MjaiEvent::StartGame {
                num_players, names, ..
            } => {
                assert_eq!(*num_players, 3);
                assert_eq!(names, &vec!["bob", "carol", "alice"]);
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::StartKyoku { scores, oya, .. } => {
                assert_eq!(scores, &vec![30_000, 40_000, 35_000]);
                assert_eq!(*oya, 1);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }

    /// The E1H0 score check stays authoritative over a wrong hint.
    #[test]
    fn e1h0_init_overrides_a_wrong_sanma_hint() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"GO","type":"25","lobby":"0"}"#);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        assert!(b.state.is_3p, "hint applied");
        let init = format!(
            r#"{{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert!(!b.state.is_3p);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { num_players: 4, .. }
        ));
        assert_eq!(tehais_len(&events), 4);
    }

    /// A rejoining flow that saw neither `<GO/>` nor `<UN/>` reads the
    /// player count off the table total; when that is unreadable, off the
    /// frame's shape: an idle ghost slot and no 2m–8m anywhere in play.
    #[test]
    fn reinit_on_a_fresh_flow_infers_sanma_from_the_frame_shape() {
        // E2 (seed 1): dealer wire-abs 1 at rel 1 → seat 0, ghost at rel 3.
        // 104000 on the table is neither yonma nor sanma, so shape decides.
        let sanma_hand = "0,32,36,40,44,72,76,80,108,112,116,120,124";
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"340,300,400,0","oya":"1","hai":"{sanma_hand}","kawa0":"36","kawa1":"40","kawa2":"44"}}"#
        );
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, &reinit);
        assert_eq!(b.state.seat, 0);
        assert!(b.state.is_3p);
        let init = r#"{"tag":"INIT","seed":"2,0,0,1,2,8","ten":"350,300,400,0","oya":"2","hai":"0,32,36,40,44,72,76,80,108,112,116,120,124"}"#;
        let events = parse_one(&mut b, init);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { num_players: 3, .. }
        ));
        assert_eq!(tehais_len(&events), 3);

        // A 5m in hand rules sanma out even with the other signs.
        let reinit = r#"{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"340,300,400,0","oya":"1","hai":"17,32,36,40,44,72,76,80,108,112,116,120,124","kawa0":"36","kawa1":"40","kawa2":"44"}"#;
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, reinit);
        assert!(!b.state.is_3p);
        // So does a 2m–8m in anyone's river...
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"340,300,400,0","oya":"1","hai":"{sanma_hand}","kawa0":"36","kawa1":"20","kawa2":"44"}}"#
        );
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, &reinit);
        assert!(!b.state.is_3p);
        // ...or a river on the 4th wire slot.
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"340,300,400,0","oya":"1","hai":"{sanma_hand}","kawa0":"36","kawa1":"40","kawa2":"44","kawa3":"48"}}"#
        );
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, &reinit);
        assert!(!b.state.is_3p);

        // The table total alone settles it when it is readable: 105000 is
        // sanma even with a 4p-looking hand, 100000 is yonma even with a
        // sanma-looking one.
        let reinit = r#"{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"350,300,400,0","oya":"1","hai":"17,32,36,40,44,72,76,80,108,112,116,120,124","kawa0":"36"}"#;
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, reinit);
        assert!(b.state.is_3p);
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"250,250,500,0","oya":"1","hai":"{sanma_hand}","kawa0":"36"}}"#
        );
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, &reinit);
        assert!(!b.state.is_3p);
    }

    /// Suspension is per hand: a new game's TAIKYOKU lifts it as well, and
    /// a game rejoined mid-way ends terminated while the next, seen whole,
    /// ends confirmed.
    #[test]
    fn new_game_after_a_suspended_hand_resumes_normally() {
        let mut b = TenhouBridge::new(None, None);
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"7,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"{HAND_NO_1M}"}}"#
        );
        parse_one(&mut b, &reinit);
        assert!(b.state.suspended);
        // The game's end still goes out — as terminated, since this flow
        // never saw the whole game — so every consumer closes it.
        let events = parse_one(
            &mut b,
            r#"{"tag":"RYUUKYOKU","sc":"250,0,250,0,250,0,250,0","owari":"1"}"#,
        );
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(matches!(
            &events[0],
            MjaiEvent::EndGame {
                reason: GameEndReason::Terminated,
                ..
            }
        ));
        parse_one(&mut b, r#"{"tag":"GO","type":"9","lobby":"0"}"#);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#);
        assert!(!b.state.suspended);
        assert!(!b.state.rejoined_mid_game);
        let init = format!(
            r#"{{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"1","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { id: Some(3), .. }
        ));
        let events = parse_one(
            &mut b,
            r#"{"tag":"RYUUKYOKU","sc":"250,0,250,0,250,0,250,0","owari":"1"}"#,
        );
        assert!(matches!(
            events.last(),
            Some(MjaiEvent::EndGame {
                reason: GameEndReason::Confirmed,
                ..
            })
        ));
    }

    /// The hands after the rejoin resume normally, but the game as a whole
    /// was never seen by this flow, so its end is still terminated: History
    /// must not file the remainder as a complete game.
    #[test]
    fn rejoined_game_ends_terminated_even_after_resuming() {
        let mut b = TenhouBridge::new(None, None);
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"6,0,0,1,2,4","ten":"250,250,250,250","oya":"2","hai":"{HAND_NO_1M}"}}"#
        );
        parse_one(&mut b, &reinit);
        let init = format!(
            r#"{{"tag":"INIT","seed":"7,0,0,1,2,4","ten":"250,250,250,250","oya":"3","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert_eq!(events.len(), 2);
        assert!(b.state.rejoined_mid_game);
        let events = parse_one(
            &mut b,
            r#"{"tag":"AGARI","who":"0","fromWho":"1","sc":"250,80,250,-80,250,0,250,0","ba":"0,0","owari":"1"}"#,
        );
        assert_eq!(events.len(), 3, "{events:?}");
        assert!(matches!(
            &events[2],
            MjaiEvent::EndGame {
                reason: GameEndReason::Terminated,
                ..
            }
        ));
    }

    /// If a new game's TAIKYOKU went missing too, its first deal is enough:
    /// E1H0 re-seats the flow, reopens the game, and clears the rejoin.
    #[test]
    fn first_deal_without_taikyoku_opens_a_new_game() {
        let mut b = TenhouBridge::new(None, None);
        let reinit = format!(
            r#"{{"tag":"REINIT","seed":"7,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"{HAND_NO_1M}"}}"#
        );
        parse_one(&mut b, &reinit);
        assert_eq!(b.state.seat, 3);
        parse_one(
            &mut b,
            r#"{"tag":"AGARI","who":"0","fromWho":"0","sc":"250,120,250,-40,250,-40,250,-40","ba":"0,0","owari":"1"}"#,
        );
        // Next game, different seat, no TAIKYOKU: E1 dealer at rel 3 → seat 1.
        let init = format!(
            r#"{{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"3","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert_eq!(b.state.seat, 1);
        assert!(!b.state.rejoined_mid_game);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { id: Some(1), .. }
        ));
        assert!(matches!(&events[1], MjaiEvent::StartKyoku { oya: 0, .. }));
        let events = parse_one(
            &mut b,
            r#"{"tag":"RYUUKYOKU","sc":"250,0,250,0,250,0,250,0","owari":"1"}"#,
        );
        assert!(matches!(
            events.last(),
            Some(MjaiEvent::EndGame {
                reason: GameEndReason::Confirmed,
                ..
            })
        ));
    }

    /// Nukidora took tiles out of the hand without adding a meld; the
    /// draw-in-hand judgement has to count them.
    #[test]
    fn reinit_counts_nukidora_when_judging_the_draw_in_hand() {
        let mut b = TenhouBridge::new(None, None);
        // Sanma table (35000 × 3), two kita declared (`m` low bits 0x20).
        let reinit = r#"{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"350,300,400,0","oya":"1","hai":"0,32,36,40,44,72,76,80,112,116,120","m0":"32,32","kawa0":"36"}"#;
        parse_one(&mut b, reinit);
        assert!(b.state.is_3p);
        assert!(b.state.melds.is_empty(), "kita is not a meld");
        assert!(!b.state.is_tsumo, "11 tiles + 2 kita = 13, nothing drawn");
        let reinit = r#"{"tag":"REINIT","seed":"1,0,0,1,2,8","ten":"350,300,400,0","oya":"1","hai":"0,32,36,40,44,72,76,80,112,116,120,124","m0":"32,32","kawa0":"36"}"#;
        parse_one(&mut b, reinit);
        assert!(b.state.is_tsumo, "12 tiles + 2 kita = 14, draw in hand");
    }

    /// A flow that joined mid-game with neither GO nor UN reads the player
    /// count off the table total on its first INIT.
    #[test]
    fn init_on_a_fresh_flow_reads_player_count_off_the_scores() {
        let mut b = TenhouBridge::new(None, None);
        // S1 (seed 4): dealer wire-abs 0 at rel 2 → seat 2. Total 105000.
        let init = r#"{"tag":"INIT","seed":"4,0,0,1,2,8","ten":"400,300,350,0","oya":"2","hai":"0,32,36,40,44,72,76,80,108,112,116,120,124"}"#;
        let events = parse_one(&mut b, init);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { num_players: 3, .. }
        ));
        assert_eq!(tehais_len(&events), 3);

        // Yonma with one riichi stick out: 99000 + 1000.
        let mut b = TenhouBridge::new(None, None);
        let init = format!(
            r#"{{"tag":"INIT","seed":"1,0,1,1,2,8","ten":"240,250,250,250","oya":"1","hai":"{HAND_NO_1M}"}}"#
        );
        let events = parse_one(&mut b, &init);
        assert!(matches!(
            &events[0],
            MjaiEvent::StartGame { num_players: 4, .. }
        ));
        assert_eq!(tehais_len(&events), 4);
    }

    /// The rejoin sequence as captured from the web client on a fresh
    /// socket: `HELO`, `GO`, `UN` (full roster), `SAIKAI`, `REINIT`, then
    /// live play — no `TAIKYOKU`. Our riichi is the `255` in `kawa0`; the
    /// only call on the table is another seat's (`m3`), so we track none.
    #[test]
    fn rejoin_sequence_as_captured_from_the_web_client() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(
            &mut b,
            r#"{"tag":"GO","type":"1","lobby":"0","gpid":"00000000-00000000"}"#,
        );
        parse_one(
            &mut b,
            r#"{"tag":"UN","n0":"us","n1":"shimocha","n2":"toimen","n3":"kamicha","dan":"0,11,0,0","rate":"1500.00,1605.68,1500.00,1500.00","sx":"M,M,M,M"}"#,
        );
        assert!(parse_one(
            &mut b,
            r#"{"tag":"SAIKAI","ba":"0,1","oya":"2","sc":"240,0,250,0,250,0,250,0"}"#
        )
        .is_empty());
        let reinit = r#"{"tag":"REINIT","seed":"0,0,1,3,0,23","ten":"240,250,250,250","oya":"2","hai":"14,18,22,36,37,48,52,57,58,61,89,93,96","m3":"48203","kawa0":"1,106,74,8,255,40","kawa1":"79,122,3,6,41","kawa2":"72,100,9,21,98","kawa3":"107,78,67,43,113,73"}"#;
        assert!(parse_one(&mut b, reinit).is_empty());
        assert_eq!(b.state.seat, 2, "E1 dealer at rel 2 → we sit at wire-abs 2");
        assert!(b.state.seat_resolved);
        assert!(b.state.suspended);
        assert!(b.state.in_riichi);
        assert!(b.state.melds.is_empty());
        assert_eq!(b.state.hand.len(), 13);
        assert!(!b.state.is_tsumo);
        assert!(!b.state.is_3p);
        assert_eq!(b.state.num_players, 4);

        // Our draw right after the rejoin: tracked for autoplay, not emitted.
        assert!(parse_one(&mut b, r#"{"tag":"T120"}"#).is_empty());
        assert!(b.state.is_tsumo);
        assert_eq!(b.state.hand.len(), 14);
        assert!(b.state.window.is_some());
        assert!(parse_one(&mut b, r#"{"tag":"D120"}"#).is_empty());
        assert_eq!(b.state.hand.len(), 13);
        assert!(parse_one(&mut b, r#"{"tag":"U"}"#).is_empty());

        // The hand ends (our ron) and E2 begins: the game reopens on seat 2
        // with the roster from the rejoin's <UN/>.
        assert!(parse_one(
            &mut b,
            r#"{"tag":"AGARI","who":"0","fromWho":"1","sc":"240,77,250,-77,250,0,250,0","ba":"0,1"}"#
        )
        .is_empty());
        let events = parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"1,0,0,2,0,55","ten":"317,173,250,250","oya":"3","hai":"36,4,124,72,84,50,60,128,32,40,62,44,63"}"#,
        );
        assert_eq!(events.len(), 2, "{events:?}");
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                num_players,
                names,
                ..
            } => {
                assert_eq!(*id, Some(2));
                assert_eq!(*num_players, 4);
                assert_eq!(names, &vec!["toimen", "kamicha", "us", "shimocha"]);
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::StartKyoku {
                kyoku,
                oya,
                scores,
                tehais,
                ..
            } => {
                assert_eq!(*kyoku, 2);
                assert_eq!(*oya, 1);
                assert_eq!(scores, &vec![25_000, 25_000, 31_700, 17_300]);
                assert_eq!(tehais[2].len(), 13);
                assert_ne!(tehais[2][0], "?");
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        assert!(!b.state.suspended);
        let events = parse_one(&mut b, r#"{"tag":"W"}"#);
        assert!(matches!(&events[0], MjaiEvent::Tsumo { actor: 1, pai } if pai == "?"));
    }
}
