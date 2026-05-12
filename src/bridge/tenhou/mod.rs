//! Tenhou (天鳳) protocol bridge.
//!
//! Wire format: plain JSON over WebSocket — `{"tag": "...", ...}`. Each
//! frame carries one tag identifying the event (`INIT`, `T0`, `D5`, `N`,
//! `REACH`, `AGARI`, `RYUUKYOKU`, …). Single-frame heartbeat is the literal
//! bytes `<Z/>`.
//!
//! Faithful Rust port of the observation half of
//! `reference/Akagi/mitm/bridge/tenhou/bridge.py`. AkagiV3 uses the bridge in
//! observe-only mode: only `Direction::Down` (server → client) is parsed,
//! `Direction::Up` and [`build`](Bridge::build) are no-ops. The rationale is
//! that all game state we need to feed analysis / bots arrives on server
//! frames; client frames are user input and contribute no new information.
//!
//! Tenhou tile encoding lives in [`tile`]; meld bitfield decoding lives in
//! [`meld`]; per-flow state in [`state`].

pub mod meld;
pub mod state;
pub mod tile;

use super::{Bridge, Direction, ParseResult};
use crate::{
    config::Platform,
    logger::{FlowLogger, Session},
    schema::{mjai::Actor, MjaiEvent},
};
use chrono::Local;
use meld::{Meld, MeldKind};
use serde_json::Value as JsonValue;
use state::State;
use std::sync::Arc;
use tile::{tenhou_to_mjai, tenhou_to_mjai_one};
use tracing::{info, warn};

const HEARTBEAT: &[u8] = b"<Z/>";
const BAKAZE: [&str; 4] = ["E", "S", "W", "N"];

/// Per-flow Tenhou state. One bridge instance per WebSocket connection.
pub struct TenhouBridge {
    state: State,
    #[allow(dead_code)]
    flow_log: Option<Arc<FlowLogger>>,
    session: Option<Arc<Session>>,
    mjai_log: Option<Arc<FlowLogger>>,
}

impl TenhouBridge {
    pub fn new(flow_log: Option<Arc<FlowLogger>>, session: Option<Arc<Session>>) -> Self {
        Self {
            state: State::default(),
            flow_log,
            session,
            mjai_log: None,
        }
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
        match tag {
            "HELO" | "REJOIN" | "GO" | "UN" | "BYE" | "SHUFFLE" => return Vec::new(),
            _ => {}
        }

        if tag == "TAIKYOKU" {
            return self.on_taikyoku(msg);
        }
        if tag == "INIT" {
            return self.on_init(msg);
        }
        if let Some(actor) = tsumo_actor(tag) {
            return self.on_tsumo(actor, tag);
        }
        if let Some((actor, tsumogiri_uppercase)) = dahai_actor(tag) {
            return self.on_dahai(actor, tag, tsumogiri_uppercase);
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

    /// `<TAIKYOKU oya="N" .../>` — start of game. Resolves our wire-absolute
    /// seat but defers `start_game` emission until the first `<INIT/>`, where
    /// the 0-score slot reveals whether the game is sanma. Without that wait
    /// we'd stamp `start_game.num_players = 4` on every sanma game (Tenhou's
    /// TAIKYOKU itself carries no player-count signal — only the dealer's
    /// relative seat).
    fn on_taikyoku(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let oya_rel = parse_u8(msg, "oya").unwrap_or(0);
        // oya is dealer's *relative* seat in the 4-cycle wire frame. Our
        // wire-abs seat is the inverse: (-oya_rel) mod 4. For sanma our
        // wire-abs is always in {0, 1, 2} because we are a real player —
        // wire-abs 3 is the ghost slot.
        self.state.num_players = 4;
        self.state.seat = (4 - oya_rel) % 4;
        self.state.is_3p = false;
        self.state.pending_start_game = true;
        self.rotate_mjai_log();
        Vec::new()
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

        let bakaze = BAKAZE[(seed[0] as usize) / 4];
        let kyoku = (seed[0] as u8) % 4 + 1;
        let honba = seed[1].max(0) as u8;
        let kyotaku = seed[2].max(0) as u8;
        let dora_marker = tenhou_to_mjai_one(seed[5] as u32);
        let raw_ten: Vec<i32> = ten.iter().map(|s| s * 100).collect();

        // Sanma detection per Python reference: at the very first kyoku of a
        // game, a 0-score slot in `ten` signals the absent 4th player. No
        // legitimate game starts with anyone at 0 points, so the value-based
        // check is safe at E1H0 (it would not be safe mid-game). We do NOT
        // wrap `seat` after detection — wire-abs is still 4-cycle, and our
        // wire-abs (already in {0, 1, 2} for any real sanma player) doubles
        // as mjai-abs.
        if bakaze == "E" && kyoku == 1 && honba == 0 && raw_ten.contains(&0) {
            self.state.is_3p = true;
            self.state.num_players = 3;
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
            let names: Vec<String> = (0..self.state.num_players)
                .map(|i| i.to_string())
                .collect();
            events.push(MjaiEvent::StartGame {
                names,
                kyoku_first: None,
                aka_flag: None,
                id: Some(self.state.seat as Actor),
                num_players: self.state.num_players,
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
        self.write_mjai(&events);
        events
    }

    /// `<T0/>`, `<U7/>`, `<V12/>`, `<W3/>` — tsumo.
    fn on_tsumo(&mut self, actor_rel: u8, tag: &str) -> Vec<MjaiEvent> {
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
        }
        let events = vec![MjaiEvent::Tsumo { actor, pai }];
        self.write_mjai(&events);
        events
    }

    /// `<D7/>`, `<E/>`, `<f12/>` — dahai.
    /// `tsumogiri_uppercase` is true when the tag's leading letter is uppercase
    /// (Tenhou's signal that the discard is just-drawn).
    fn on_dahai(
        &mut self,
        actor_rel: u8,
        tag: &str,
        tsumogiri_uppercase: bool,
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

        // Tsumogiri logic: for our own discards, compare against the just-drawn
        // tile (handles edge case where tag is uppercase but tile is a tedashi
        // that happens to match the drawn tile's index).
        let tsumogiri = if actor == self.state.seat {
            self.state.hand.last().copied() == Some(idx)
        } else {
            tsumogiri_uppercase
        };

        self.state.last_kawa_tile = pai.clone();
        self.state.last_revealed_tile_actor = Some(actor);
        self.state.is_tsumo = false;
        if actor == self.state.seat {
            if let Some(pos) = self.state.hand.iter().rposition(|&i| i == idx) {
                self.state.hand.remove(pos);
            }
        }

        let events = vec![MjaiEvent::Dahai {
            actor,
            pai,
            tsumogiri,
        }];
        self.write_mjai(&events);
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
            self.write_mjai(&events);
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
                consumed: [consumed[0].clone(), consumed[1].clone(), consumed[2].clone()],
            },
            MeldKind::Kakan => {
                self.state.last_revealed_tile_actor = Some(actor); // chankan target
                MjaiEvent::Kakan {
                    actor,
                    pai,
                    consumed: [consumed[0].clone(), consumed[1].clone(), consumed[2].clone()],
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
            self.state.melds.push(meld);
        } else {
            // Track other players' melds is not required — observation only.
        }

        let events = vec![event];
        self.write_mjai(&events);
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
        let step = msg
            .get("step")
            .and_then(|v| v.as_str().and_then(|s| s.parse::<u8>().ok()).or(v.as_u64().map(|n| n as u8)));
        let events = match step {
            Some(1) => vec![MjaiEvent::Reach { actor, pai: None }],
            Some(2) => {
                if actor == self.state.seat {
                    self.state.in_riichi = true;
                }
                vec![MjaiEvent::ReachAccepted { actor }]
            }
            _ => return Vec::new(),
        };
        self.write_mjai(&events);
        events
    }

    /// `<DORA hai="..."/>` — new dora indicator (kan dora).
    fn on_dora(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let Some(idx) = parse_u32(msg, "hai") else {
            return Vec::new();
        };
        let dora_marker = tenhou_to_mjai_one(idx);
        let events = vec![MjaiEvent::Dora { dora_marker }];
        self.write_mjai(&events);
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
            events.push(MjaiEvent::EndGame);
        }
        self.write_mjai(&events);
        events
    }

    /// `<RYUUKYOKU .../>` — exhaustive draw.
    fn on_ryukyoku(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
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
            events.push(MjaiEvent::EndGame);
        }
        self.write_mjai(&events);
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
        ParseResult { events, parsed }
    }

    fn build(&mut self, _command: &MjaiEvent) -> Option<Vec<u8>> {
        // Observe-only mode. Autoplay is intentionally out-of-scope.
        None
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

/// Dahai tags are `D/E/F/G<n>` (uppercase = tsumogiri of just-drawn tile)
/// or `d/e/f/g<n>` (lowercase = tedashi). Returns `(rel_actor, uppercase)`.
fn dahai_actor(tag: &str) -> Option<(u8, bool)> {
    let mut bytes = tag.bytes();
    let first = bytes.next()?;
    let (rel, upper) = match first {
        b'D' => (0, true),
        b'E' => (1, true),
        b'F' => (2, true),
        b'G' => (3, true),
        b'd' => (0, false),
        b'e' => (1, false),
        b'f' => (2, false),
        b'g' => (3, false),
        _ => return None,
    };
    if !bytes.all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((rel, upper))
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

    #[test]
    fn init_emits_start_game_then_start_kyoku_yonma() {
        let mut b = TenhouBridge::new(None, None);
        // Dealer at rel 1 → our wire-abs = (4-1)%4 = 3.
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#);
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"1","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        assert_eq!(events.len(), 2, "yonma first INIT emits start_game + start_kyoku");
        match &events[0] {
            MjaiEvent::StartGame { id, num_players, names, .. } => {
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
        assert_eq!(sg_num_players, 3, "start_game.num_players must reflect sanma");
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
            MjaiEvent::StartKyoku { scores, num_players, .. } => {
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

    #[test]
    fn dahai_other_player_uppercase_is_tsumogiri() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        parse_one(
            &mut b,
            r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,1,2,3,4,5,6,7,8,9,10,11,12"}"#,
        );
        let events = parse_one(&mut b, r#"{"tag":"E40"}"#); // uppercase E -> rel=1, tsumogiri
        match &events[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 1);
                // index 40 / 4 = 10 → 2p (pin block starts at type 9 = 1p).
                assert_eq!(pai, "2p");
                assert!(*tsumogiri);
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
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
        assert!(matches!(events[2], MjaiEvent::EndGame));
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
                MjaiEvent::Hora { actor, target, deltas, ura_markers } => {
                    Some((*actor, *target, deltas.clone(), ura_markers.clone()))
                }
                _ => None,
            })
            .expect("hora event emitted");
        assert_eq!(hora.0, 0, "winner is seat 0");
        assert_eq!(hora.1, 2, "ron target is seat 2 — fromWho must parse case-sensitively");
        // Ura dora indicator field is also camelCase; verify it survives the
        // same casing fix. Tile id 45 / 4 = 11 → 3p in 34-space; mjai's
        // string for `id % 4` variants of 3p is "3p".
        assert_eq!(
            hora.3.as_deref().map(|v| v.iter().map(String::as_str).collect::<Vec<_>>()),
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
        let e2 = parse_one(&mut b, r#"{"tag":"REACH","who":"0","step":"2","ten":"240,250,250,260"}"#);
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
            MjaiEvent::StartGame { id, num_players, names, .. } => {
                (*id, *num_players, names.len())
            }
            other => panic!("expected StartGame first, got {other:?}"),
        };
        assert_eq!(sg, (Some(1), 3, 3), "sanma start_game with id=1 (wire-abs of South seat) and num_players=3");

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
        assert_eq!(sk.2, 0, "E1 dealer at wire-abs 0 (East), not collapsed onto our seat");
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

        // Dealer's dahai of tile 56 (6p), tsumogiri.
        let g56 = parse_one(&mut b, r#"{"tag":"g56"}"#);
        match &g56[0] {
            MjaiEvent::Dahai { actor, pai, tsumogiri } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "6p");
                assert!(!*tsumogiri, "lowercase g → tedashi");
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
            MjaiEvent::Dahai { actor, pai, tsumogiri } => {
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
}
