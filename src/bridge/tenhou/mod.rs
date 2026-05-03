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

    /// `<TAIKYOKU oya="N" .../>` — start of game. Resolves our absolute seat.
    fn on_taikyoku(&mut self, msg: &JsonValue) -> Vec<MjaiEvent> {
        let oya_rel = parse_u8(msg, "oya").unwrap_or(0);
        // oya is dealer's *relative* seat. Inverting gives our absolute seat:
        // if dealer is at relative seat r, our absolute seat is (-r) mod N.
        // Defaults to yonma here; sanma is detected at INIT.
        self.state.num_players = 4;
        self.state.seat = (4 - oya_rel) % 4;
        self.state.is_3p = false;
        self.rotate_mjai_log();
        let events = vec![MjaiEvent::StartGame {
            names: vec![
                "0".to_string(),
                "1".to_string(),
                "2".to_string(),
                "3".to_string(),
            ],
            kyoku_first: None,
            aka_flag: None,
            id: Some(self.state.seat as Actor),
            num_players: 4,
        }];
        self.write_mjai(&events);
        events
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
        let mut scores: Vec<i32> = ten.iter().map(|s| s * 100).collect();

        // Sanma detection per Python reference: at the very first kyoku, a
        // 0-score slot signals the missing 4th player.
        if bakaze == "E" && kyoku == 1 && honba == 0 && scores.contains(&0) {
            self.state.is_3p = true;
            self.state.num_players = 3;
            // Reapply seat with sanma modulus.
            self.state.seat %= 3;
        }

        // Our hand. Tenhou messages are already in *our* viewpoint (rel seat 0
        // is us), so the hand is always under the bare key `hai` regardless of
        // our absolute seat — not `hai{seat}` as the legacy XML docstring
        // suggests. Other seats appear as `'?'` placeholders.
        let our_hand_indices = parse_csv_u32(msg, "hai");
        self.state.reset_for_kyoku();
        self.state.hand = our_hand_indices.clone();

        let oya_abs = self.state.rel_to_abs(oya_rel);

        // Build per-seat starting hands. Length = num_players.
        let n = self.state.num_players as usize;
        let mut tehais: Vec<Vec<String>> = vec![vec!["?".to_string(); 13]; n];
        if (self.state.seat as usize) < n {
            tehais[self.state.seat as usize] = tenhou_to_mjai(&our_hand_indices);
        }

        // Sanma score remap: scores arrive in relative-seat order with a 0
        // for the missing slot. Place each into its absolute seat. The 0-slot
        // simply ends up unused (it never aligns to any of our 3 seats since
        // it is not relative seat 0..2 of a real player).
        if self.state.is_3p {
            let mut new_scores = vec![0i32; n];
            for (i, &s) in scores.iter().enumerate().take(4) {
                if s == 0 {
                    continue;
                }
                let abs = (i as u8 + self.state.seat) % 3;
                new_scores[abs as usize] = s;
            }
            scores = new_scores;
        }

        let events = vec![MjaiEvent::StartKyoku {
            bakaze: bakaze.to_string(),
            dora_marker,
            kyoku,
            honba,
            kyotaku,
            oya: oya_abs as Actor,
            scores,
            tehais,
            num_players: self.state.num_players,
        }];
        self.write_mjai(&events);
        events
    }

    /// `<T0/>`, `<U7/>`, `<V12/>`, `<W3/>` — tsumo.
    fn on_tsumo(&mut self, actor_rel: u8, tag: &str) -> Vec<MjaiEvent> {
        if actor_rel >= self.state.num_players {
            return Vec::new();
        }
        self.state.live_wall = self.state.live_wall.saturating_sub(1);
        let actor = self.state.rel_to_abs(actor_rel);
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
        if actor_rel >= self.state.num_players {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);

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
        if actor_rel >= self.state.num_players {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
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
        let target = match meld.kind {
            MeldKind::Chi => (actor + self.state.num_players - 1) % self.state.num_players,
            _ => (actor + meld.target_rel) % self.state.num_players,
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
        if actor_rel >= self.state.num_players {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
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
        let from_rel = parse_u8(msg, "fromwho").unwrap_or(actor_rel);
        if actor_rel >= self.state.num_players || from_rel >= self.state.num_players {
            return Vec::new();
        }
        let actor = self.state.rel_to_abs(actor_rel);
        let target = self.state.rel_to_abs(from_rel);

        // sc field is "before0,delta0,before1,delta1,..." in 100-yen units.
        let sc = parse_csv_i32(msg, "sc");
        let mut deltas = Vec::with_capacity(self.state.num_players as usize);
        for chunk in sc.chunks(2).take(self.state.num_players as usize) {
            if chunk.len() == 2 {
                deltas.push(chunk[1] * 100);
            } else {
                deltas.push(0);
            }
        }
        // sc arrives in *relative* seat order; re-key to absolute.
        let mut deltas_abs = vec![0i32; self.state.num_players as usize];
        for (rel, &d) in deltas.iter().enumerate() {
            let abs = self.state.rel_to_abs(rel as u8) as usize;
            if abs < deltas_abs.len() {
                deltas_abs[abs] = d;
            }
        }

        // Ura dora markers are space-separated tile indices in `dorahaiUra`.
        let ura_markers = msg.get("dorahaiUra").and_then(parse_tile_csv);

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
        let mut deltas = Vec::with_capacity(self.state.num_players as usize);
        for chunk in sc.chunks(2).take(self.state.num_players as usize) {
            if chunk.len() == 2 {
                deltas.push(chunk[1] * 100);
            } else {
                deltas.push(0);
            }
        }
        let mut deltas_abs = vec![0i32; self.state.num_players as usize];
        for (rel, &d) in deltas.iter().enumerate() {
            let abs = self.state.rel_to_abs(rel as u8) as usize;
            if abs < deltas_abs.len() {
                deltas_abs[abs] = d;
            }
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
    fn taikyoku_resolves_seat_and_emits_start_game() {
        let mut b = TenhouBridge::new(None, None);
        // Dealer is at relative seat 1 → our absolute seat is (4-1)%4 = 3.
        let events = parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"1"}"#);
        assert_eq!(events.len(), 1);
        match &events[0] {
            MjaiEvent::StartGame {
                id, num_players, ..
            } => {
                assert_eq!(*id, Some(3));
                assert_eq!(*num_players, 4);
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
    }

    #[test]
    fn init_emits_start_kyoku_yonma() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        // East-1, dora indicator at index 4 (5m), all 13 tiles for our seat.
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        assert_eq!(events.len(), 1);
        match &events[0] {
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
                assert_eq!(*oya, 0);
                assert_eq!(dora_marker, "2m"); // index 4 → 2m
                assert_eq!(scores, &vec![25000; 4]);
                assert_eq!(tehais.len(), 4);
                assert_eq!(tehais[0].len(), 13);
                assert_eq!(*num_players, 4);
                // Other seats are placeholders.
                for hand in tehais.iter().skip(1) {
                    assert_eq!(hand, &vec!["?".to_string(); 13]);
                }
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }

    #[test]
    fn init_detects_sanma_via_zero_score() {
        let mut b = TenhouBridge::new(None, None);
        parse_one(&mut b, r#"{"tag":"TAIKYOKU","oya":"0"}"#);
        // 4-element ten with one slot 0 indicates sanma.
        let init = r#"{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"350,350,350,0","oya":"0","hai":"0,4,8,36,40,44,72,76,80,108,112,116,120"}"#;
        let events = parse_one(&mut b, init);
        match &events[0] {
            MjaiEvent::StartKyoku {
                num_players,
                scores,
                tehais,
                ..
            } => {
                assert_eq!(*num_players, 3);
                assert_eq!(scores.len(), 3);
                assert_eq!(tehais.len(), 3);
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
        // Tsumo win: who=0, fromwho=0; sc deltas distribute points.
        let events = parse_one(
            &mut b,
            r#"{"tag":"AGARI","who":"0","fromwho":"0","sc":"250,40,250,-10,250,-10,250,-20"}"#,
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
            r#"{"tag":"AGARI","who":"0","fromwho":"0","sc":"250,40,250,-10,250,-10,250,-20","owari":"290,30,240,-10,240,-10,230,-20"}"#,
        );
        assert_eq!(events.len(), 3);
        assert!(matches!(events[2], MjaiEvent::EndGame));
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
        match &events[0] {
            MjaiEvent::StartKyoku { tehais, oya, .. } => {
                // E1 dealer at rel seat 3 → abs seat 0.
                assert_eq!(*oya, 0);
                // Our hand should be at index 1 (our absolute seat) and
                // contain 13 real tiles, not be empty.
                assert_eq!(tehais.len(), 4);
                assert_eq!(tehais[1].len(), 13);
                assert_ne!(tehais[1], vec!["?".to_string(); 13]);
                // First tile is index 65 → 65/4 = 16 → 8p.
                assert_eq!(tehais[1][0], "8p");
                // Other seats stay as 13 placeholder tiles.
                for (i, hand) in tehais.iter().enumerate() {
                    if i == 1 {
                        continue;
                    }
                    assert_eq!(hand, &vec!["?".to_string(); 13]);
                }
            }
            other => panic!("expected StartKyoku, got {other:?}"),
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
}
