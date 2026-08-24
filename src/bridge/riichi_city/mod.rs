//! Riichi City (麻雀一番街) protocol bridge.
//!
//! Wire format: a 15-byte big-endian [`packet`] header + optional JSON body
//! over WebSocket. Gameplay messages carry a string `"cmd"` discriminator
//! inside the JSON (`cmd_enter_room`, `cmd_game_start`, `cmd_game_action_brc`,
//! …); the binary `cmd` field only matters for `CMDAuth`, which carries our
//! `uid`. See [`packet`] for the framing and [`consts`] for the tile table.
//!
//! Rust port of the observation half of the Akagi v2 Python Riichi City
//! bridge, extended with the uplink half autoplay needs: [`build`](Bridge::build)
//! encodes our actions as client frames injected onto the gameplay socket.
//! Both wire directions are parsed (the `CMDAuth` packet is client→server
//! while gameplay broadcasts are server→client; client request frames don't
//! match any server `cmd_*` and fall through).
//!
//! Two intentional improvements over v2:
//! - **Sanma** events use the native length 3 (no ghost-padding to 4); actor
//!   indices stay in `0..=2`, matching the Tenhou bridge and V3 downstream.
//! - **`cmd_game_action_brc`** iterates every entry in `action_info` instead of
//!   returning after the first (v2 dropped batched actions).
//!
//! Win / draw settlement is decoded from `cmd_game_end`: a `hora` per winner
//! (with deltas + ura-dora) or a `ryukyoku`, followed by `end_kyoku`. Deltas are
//! each player's `point_profit` (+ `extra_profit`) — the per-hand change
//! including honba and collected kyotaku but EXCLUDING that player's own riichi
//! stick. The −1000 for declaring riichi is applied by the mjai consumer at
//! `reach_accepted` (same split as the Tenhou bridge); a `user_point` running
//! diff would double-count it. The action codes 7/10/12 in
//! `cmd_game_action_brc` only flag the end — the scores live in `cmd_game_end`.

pub mod build;
pub mod consts;
pub mod packet;
pub mod state;

use super::{Bridge, Direction, ParseResult};
use crate::{
    config::Platform,
    logger::{FlowLogger, Session},
    schema::{GameMeta, MatchInfo, MjaiEvent, ParsedFrame},
};
use chrono::Local;
use consts::card_to_mjai;
use packet::{WPacket, CMD_AUTH};
use serde_json::Value as JsonValue;
use state::GameStatus;
use std::sync::Arc;
use tracing::{info, warn};

const LOG: &str = "akagi::bridge::riichi_city";

/// Per-flow Riichi City state. One bridge instance per WebSocket connection.
pub struct RiichiCityBridge {
    /// Our own player id, captured from the `CMDAuth` handshake. `-1` until seen.
    uid: i64,
    status: GameStatus,
    #[allow(dead_code)]
    flow_log: Option<Arc<FlowLogger>>,
    session: Option<Arc<Session>>,
    mjai_log: Option<Arc<FlowLogger>>,
    /// Frame-injection gate (see `autoplay::inject`): set while this
    /// connection is inside a game, so injected gameplay frames only ride
    /// the WS flow that is actually carrying one.
    inject: Option<crate::autoplay::inject::SharedInjectBus>,
}

impl RiichiCityBridge {
    pub fn new(flow_log: Option<Arc<FlowLogger>>, session: Option<Arc<Session>>) -> Self {
        Self {
            uid: -1,
            status: GameStatus::default(),
            flow_log,
            session,
            mjai_log: None,
            inject: None,
        }
    }

    pub fn with_inject(mut self, inject: Option<crate::autoplay::inject::SharedInjectBus>) -> Self {
        self.inject = inject;
        self
    }

    /// Open a fresh `riichi_city_<ts>.mjai.jsonl` per game (Tenhou/Majsoul
    /// rotation pattern). No-op when no session is wired (tests).
    fn rotate_mjai_log(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let ts = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
        let file_name = format!("riichi_city_{ts}.mjai.jsonl");
        let label = format!("riichi_city mjai {ts}");
        match session.flow_logger(Platform::RiichiCity.subdir(), &file_name, label) {
            Ok(log) => {
                info!(target: LOG, "opened mjai log {file_name}");
                self.mjai_log = Some(log);
            }
            Err(e) => {
                warn!(target: LOG, "failed to open mjai log {file_name}: {e:#}");
                self.mjai_log = None;
            }
        }
    }

    fn write_mjai(&self, events: &[MjaiEvent]) {
        let Some(log) = &self.mjai_log else { return };
        for ev in events {
            match serde_json::to_string(ev) {
                Ok(line) => log.writeln(&line),
                Err(e) => warn!(target: LOG, "failed to serialize MjaiEvent: {e:#}"),
            }
        }
    }

    fn dispatch(&mut self, pkt: &WPacket) -> Vec<MjaiEvent> {
        // The binary auth handshake (client → server) carries our uid.
        if pkt.cmd == CMD_AUTH {
            if let Some(uid) = pkt.body.get("uid").and_then(json_i64) {
                self.uid = uid;
                info!(target: LOG, "captured player uid from auth");
            }
            return Vec::new();
        }
        let Some(cmd) = pkt.body.get("cmd").and_then(JsonValue::as_str) else {
            return Vec::new();
        };
        let data = pkt.body.get("data");
        // Decision-window state: opened by the server's cmd_send_* offers,
        // closed by anything that resolves play.
        if let Some(inject) = &self.inject {
            match cmd {
                "cmd_send_current_action" | "cmd_send_other_action" => inject.note_window(),
                "cmd_game_action_brc" | "cmd_game_end" | "cmd_room_end" | "cmd_game_start" => {
                    inject.note_window_closed()
                }
                _ => {}
            }
        }
        match cmd {
            // Per-request ack, counted for autoplay verification.
            "rsp_game_action" => {
                if let Some(inject) = &self.inject {
                    let code = data
                        .and_then(|d| d.get("code"))
                        .and_then(json_i64)
                        .unwrap_or(0);
                    inject.note_rsp(code);
                }
                Vec::new()
            }
            "cmd_enter_room" => {
                // Wire sends the table token as a string; tolerate a number.
                let room_id = pkt.body.get("room_id").and_then(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| json_i64(v).map(|n| n.to_string()))
                });
                self.on_enter_room(room_id, data)
            }
            "cmd_game_start" => self.on_game_start(data),
            "cmd_in_card_brc" => self.on_in_card_brc(data),
            "cmd_send_current_action" => self.on_send_current_action(data),
            "cmd_game_action_brc" => self.on_game_action_brc(data),
            "cmd_gang_bao_brc" => self.on_gang_bao_brc(data),
            "cmd_game_end" => self.on_game_end(data),
            "cmd_room_end" => self.on_room_end(),
            _ => Vec::new(),
        }
    }

    /// `cmd_enter_room` — collect players + table size; emits nothing. Dedups
    /// a repeated announcement of the *same table* by the wrapper's
    /// `room_id` (the per-table instance token) — NOT by `classify_id`,
    /// which names the matchmaking queue class and so repeats across
    /// consecutive games in the same room tier. The dedup also requires an
    /// already-collected roster, so a fuller re-announcement can still
    /// replace an empty one.
    fn on_enter_room(
        &mut self,
        room_id: Option<String>,
        data: Option<&JsonValue>,
    ) -> Vec<MjaiEvent> {
        let Some(data) = data else { return Vec::new() };
        let classify_id = data.pointer("/options/classify_id").and_then(|v| {
            v.as_str()
                .map(str::to_string)
                .or_else(|| json_i64(v).map(|n| n.to_string()))
        });
        if let (Some(prev), Some(now)) = (self.status.room_id.as_deref(), room_id.as_deref()) {
            if prev == now && !self.status.player_list.is_empty() {
                warn!(target: LOG, "duplicate cmd_enter_room for active room, ignored");
                return Vec::new();
            }
        }
        let player_count = data
            .pointer("/options/player_count")
            .and_then(json_i64)
            .unwrap_or(4);
        let is_3p = player_count == 3;
        let mut status = GameStatus {
            game_start: true,
            is_3p,
            num_players: if is_3p { 3 } else { 4 },
            classify_id,
            room_id,
            stage_type: data.pointer("/options/stage_type").and_then(json_i64),
            game_play: data.pointer("/options/game_play").and_then(json_i64),
            ..GameStatus::default()
        };
        if let Some(players) = data.get("players").and_then(JsonValue::as_array) {
            for p in players {
                if let Some(uid) = p.pointer("/user/user_id").and_then(json_i64) {
                    status.player_list.push(uid);
                    if let Some(name) = p.pointer("/user/nickname").and_then(JsonValue::as_str) {
                        status.nicknames.insert(uid, name.to_string());
                    }
                }
            }
        }
        // Mid-game reconnect: the running hand's `cmd_game_start` never
        // arrives on this connection, so the rotation and our seat must be
        // resolved here or every event — our own decision windows included
        // — is attributed to the wrong actor (observed live: our window
        // tagged `seat 0`, the default, and autoplay went blind). The
        // payload carries everything the first `cmd_game_start` uses: the
        // roster plus `initial_dealer_pos`, the first hand's East dealer —
        // the client does exactly this from this same message
        // (`GameParam.FirstBanker = initial_dealer_pos + 1`, "根据第一局的
        // 东家 确定座位").
        if data.get("is_reconnect").and_then(JsonValue::as_bool) == Some(true)
            && !status.player_list.is_empty()
        {
            let dealer = field_i64(data, "initial_dealer_pos").unwrap_or(0);
            let mid = dealer.rem_euclid(status.player_list.len() as i64) as usize;
            status.player_list.rotate_left(mid);
            let seat = status
                .player_list
                .iter()
                .position(|&id| id == self.uid)
                .unwrap_or_else(|| {
                    warn!(target: LOG, "reconnect: our uid not in player_list; defaulting seat 0");
                    0
                }) as u8;
            info!(target: LOG, "reconnect: resolved seat {seat} from initial_dealer_pos {dealer}");
            status.seat = seat;
            status.shift = dealer;
            status.seat_resolved = true;
            // Mid-hand (`hand_status` present): the game is already running
            // and the tracker holds its state from the original connection,
            // so the next kyoku boundary must NOT emit a second
            // `start_game`. Pre-deal reconnects (`hand_status` null) leave
            // the flag set — that `start_game` is still owed.
            if data.get("hand_status").is_some_and(|h| !h.is_null()) {
                status.game_start = false;
            }
        }
        self.status = status;
        if let Some(inject) = &self.inject {
            inject.set_in_game(true);
        }
        Vec::new()
    }

    /// `cmd_game_start` — per kyoku. On the first kyoku also resolves our seat
    /// and emits `start_game`; then `start_kyoku` + the opening `tsumo`.
    fn on_game_start(&mut self, data: Option<&JsonValue>) -> Vec<MjaiEvent> {
        let Some(data) = data else { return Vec::new() };
        let mut events = Vec::new();
        let bakaze = field_card(data, "quan_feng");
        let dora_marker = field_card(data, "bao_pai_card");
        let dealer_pos = field_i64(data, "dealer_pos").unwrap_or(0);
        let n = self.status.num_players as i64;

        if self.status.game_start {
            // Rotate enter-room order so index 0 is the first dealer —
            // unless a reconnect already did (seat_resolved); re-rotating
            // the rotated list would shift every actor.
            if !self.status.seat_resolved {
                if !self.status.player_list.is_empty() {
                    let mid = dealer_pos.rem_euclid(self.status.player_list.len() as i64) as usize;
                    self.status.player_list.rotate_left(mid);
                }
                let seat = self
                    .status
                    .player_list
                    .iter()
                    .position(|&id| id == self.uid)
                    .unwrap_or_else(|| {
                        warn!(target: LOG, "our uid not found in player_list; defaulting seat 0");
                        0
                    }) as u8;
                self.status.seat = seat;
                self.status.shift = dealer_pos;
                self.status.seat_resolved = true;
            }
            let seat = self.status.seat;
            self.rotate_mjai_log();
            // Display names in mjai-actor order (player_list is already
            // dealer-rotated). A missing nickname becomes "" so the frontend
            // falls back to its localized "Seat N" label.
            let mut names: Vec<String> = self
                .status
                .player_list
                .iter()
                .map(|uid| self.status.nicknames.get(uid).cloned().unwrap_or_default())
                .collect();
            names.resize(self.status.num_players as usize, String::new());
            events.push(MjaiEvent::StartGame {
                names,
                kyoku_first: None,
                aka_flag: None,
                id: Some(seat),
                num_players: self.status.num_players,
                game_meta: Some(GameMeta {
                    game_id: None,
                    match_mode: None,
                    match_info: Some(MatchInfo::RiichiCity {
                        room_id: self.status.room_id.clone(),
                        classify_id: self.status.classify_id.clone(),
                        stage_type: self.status.stage_type,
                        game_play: self.status.game_play,
                    }),
                }),
            });
            self.status.game_start = false;
        }

        let rel = (dealer_pos - self.status.shift).rem_euclid(n) as u8;
        let kyoku = rel + 1;
        let oya = rel;
        let honba = field_i64(data, "ben_chang_num").unwrap_or(0).max(0) as u8;
        let kyotaku = field_i64(data, "li_zhi_bang_num").unwrap_or(0).max(0) as u8;

        // user_info_list stays in fixed wire-seat order, while player_list is
        // rotated into MJAI actor order. Map by user_id so each score follows
        // the same actor assignment as gameplay events and settlement deltas.
        let mut scores = vec![0i32; self.status.num_players as usize];
        if let Some(list) = data.get("user_info_list").and_then(JsonValue::as_array) {
            for p in list.iter().take(self.status.num_players as usize) {
                let Some(uid) = field_i64(p, "user_id") else {
                    continue;
                };
                let Some(actor) = self.status.actor_of(uid) else {
                    continue;
                };
                if let Some(score) = scores.get_mut(actor as usize) {
                    *score = field_i64(p, "hand_points").unwrap_or(0) as i32;
                }
            }
        }

        // Our starting hand; opponents are hidden placeholders.
        let hand_cards: Vec<i64> = data
            .get("hand_cards")
            .and_then(JsonValue::as_array)
            .map(|a| a.iter().filter_map(json_i64).collect())
            .unwrap_or_default();
        let (tehai_codes, tsumo_code): (&[i64], Option<i64>) = if hand_cards.len() == 14 {
            (&hand_cards[..13], Some(hand_cards[13]))
        } else {
            (&hand_cards[..], None)
        };
        let my_tehai: Vec<String> = tehai_codes
            .iter()
            .map(|&c| card_to_mjai(c as u32))
            .collect();
        let mut tehais: Vec<Vec<String>> =
            vec![vec!["?".to_string(); 13]; self.status.num_players as usize];
        if (self.status.seat as usize) < tehais.len() && my_tehai.len() == 13 {
            tehais[self.status.seat as usize] = my_tehai;
        }

        events.push(MjaiEvent::StartKyoku {
            bakaze,
            dora_marker,
            kyoku,
            honba,
            kyotaku,
            oya,
            scores,
            tehais,
            num_players: self.status.num_players,
        });

        self.status.pending_dora.clear();
        self.status.pending_reach = None;
        self.status.meld_counts = [0; 4];

        // Opening draw: our own drawn 14th tile if we are the dealer, otherwise
        // the dealer's hidden first draw.
        match tsumo_code {
            Some(code) => {
                self.status.drawn_by = Some(self.status.seat);
                events.push(MjaiEvent::Tsumo {
                    actor: self.status.seat,
                    pai: card_to_mjai(code as u32),
                });
            }
            None => {
                self.status.drawn_by = Some(oya);
                events.push(MjaiEvent::Tsumo {
                    actor: oya,
                    pai: "?".to_string(),
                });
            }
        }
        events
    }

    /// `cmd_in_card_brc` — another player's draw (tile hidden as `?`).
    fn on_in_card_brc(&mut self, data: Option<&JsonValue>) -> Vec<MjaiEvent> {
        let mut events = self.flush_pending_reach();
        let Some(data) = data else { return events };
        let Some(uid) = field_i64(data, "user_id") else {
            return events;
        };
        let Some(actor) = self.status.actor_of(uid) else {
            warn!(target: LOG, "cmd_in_card_brc from unknown user_id");
            return events;
        };
        self.status.drawn_by = Some(actor);
        events.push(MjaiEvent::Tsumo {
            actor,
            pai: field_card(data, "card"),
        });
        events
    }

    /// `cmd_send_current_action` — our own draw (revealed tile). Also
    /// fires with `in_card: 0` as the discard prompt after our own
    /// chi/pon (the client's handler documents both), which is no draw
    /// at all — the `"?"` guard skips it, and `drawn_by` must stay unset
    /// so the following discard is never read as tsumogiri.
    fn on_send_current_action(&mut self, data: Option<&JsonValue>) -> Vec<MjaiEvent> {
        let mut events = self.flush_pending_reach();
        let Some(data) = data else { return events };
        let pai = field_card(data, "in_card");
        if pai != "?" {
            self.status.drawn_by = Some(self.status.seat);
            events.push(MjaiEvent::Tsumo {
                actor: self.status.seat,
                pai,
            });
        } else {
            warn!(target: LOG, "cmd_send_current_action with unknown in_card");
        }
        events
    }

    /// `cmd_game_action_brc` — chi/pon/kan/dahai/reach/nukidora and end-of-kyoku
    /// triggers. Every entry in `action_info` is processed.
    fn on_game_action_brc(&mut self, data: Option<&JsonValue>) -> Vec<MjaiEvent> {
        let mut events = self.flush_pending_reach();
        let Some(data) = data else { return events };
        let Some(actions) = data.get("action_info").and_then(JsonValue::as_array) else {
            return events;
        };
        let n = self.status.num_players;
        for action in actions {
            let act = field_i64(action, "action").unwrap_or(-1);
            // Win (ron 7 / tsumo 10) and abortive draw (12) are finalized by the
            // cmd_game_end settlement (hora/ryukyoku + end_kyoku), not here.
            if matches!(act, 7 | 10 | 12) {
                continue;
            }
            let Some(uid) = field_i64(action, "user_id") else {
                continue;
            };
            let Some(actor) = self.status.actor_of(uid) else {
                warn!(target: LOG, "game action from unknown user_id");
                continue;
            };
            // Chi/pon/daiminkan/ankan shrink the caller's rack by one
            // 3-tile set (a kakan upgrades a counted pon; kita nets zero).
            if matches!(act, 2..=6 | 8) {
                if let Some(m) = self.status.meld_counts.get_mut(actor as usize) {
                    *m = m.saturating_add(1);
                }
            }
            match act {
                2..=4 => {
                    // Chi always calls from kamicha (previous seat).
                    let target = (actor + n - 1) % n;
                    let consumed = group_cards(action);
                    events.push(MjaiEvent::Chi {
                        actor,
                        target,
                        pai: field_card(action, "card"),
                        consumed: [idx(&consumed, 0), idx(&consumed, 1)],
                    });
                }
                5 => {
                    let consumed = group_cards(action);
                    events.push(MjaiEvent::Pon {
                        actor,
                        target: self.status.last_dahai_actor.unwrap_or(actor),
                        pai: field_card(action, "card"),
                        consumed: [idx(&consumed, 0), idx(&consumed, 1)],
                    });
                }
                6 => {
                    let consumed = group_cards(action);
                    events.push(MjaiEvent::Daiminkan {
                        actor,
                        target: self.status.last_dahai_actor.unwrap_or(actor),
                        pai: field_card(action, "card"),
                        consumed: [idx(&consumed, 0), idx(&consumed, 1), idx(&consumed, 2)],
                    });
                }
                8 => {
                    // Concealed kan: four copies, the first marked red for 5s.
                    let base = field_card(action, "card");
                    let red = red_of(&base);
                    events.push(MjaiEvent::Ankan {
                        actor,
                        consumed: [red, base.clone(), base.clone(), base],
                    });
                }
                9 => {
                    // Added kan: pai is the added tile; the existing pon is three
                    // copies. If the added tile is a plain 5 the pon held the red;
                    // if it is the red 5 the pon held three plain.
                    let pai = field_card(action, "card");
                    let consumed = if is_red(&pai) {
                        let plain = pai[..2].to_string();
                        [plain.clone(), plain.clone(), plain]
                    } else {
                        [red_of(&pai), pai.clone(), pai.clone()]
                    };
                    events.push(MjaiEvent::Kakan {
                        actor,
                        pai,
                        consumed,
                    });
                }
                11 => {
                    let pai = field_card(action, "card");
                    // Tsumogiri inference: `move_cards_pos[0]` is the tile's
                    // 1-based rack slot and the held draw racks LAST, so a
                    // tsumogiri names exactly `13 - 3*melds + 1` — the rack
                    // size while holding a draw. The client clamps a
                    // would-be last-slot tedashi below that
                    // (`CheckOutCardIndex`), making the sentinel
                    // unambiguous — but only on a turn that actually drew:
                    // a post-chi/pon discard reuses the same slot range
                    // with no draw involved.
                    let drawn_slot = self
                        .status
                        .meld_counts
                        .get(actor as usize)
                        .map(|&m| 13 - 3 * m as i64 + 1);
                    let tsumogiri = self.status.drawn_by == Some(actor)
                        && match action.get("move_cards_pos").and_then(JsonValue::as_array) {
                            Some(a) if !a.is_empty() => a.first().and_then(json_i64) == drawn_slot,
                            // No positions: the draw is all we know.
                            _ => true,
                        };
                    self.status.drawn_by = None;
                    let is_li_zhi = field_bool(action, "is_li_zhi");
                    if is_li_zhi {
                        events.push(MjaiEvent::Reach { actor, pai: None });
                    }
                    events.push(MjaiEvent::Dahai {
                        actor,
                        pai,
                        tsumogiri,
                    });
                    self.status.last_dahai_actor = Some(actor);
                    if is_li_zhi {
                        // Defer reach_accepted until after we know whether the
                        // discard was called (chi/pon/kan) — flushed at the next
                        // action / draw.
                        self.status.pending_reach = Some(MjaiEvent::ReachAccepted { actor });
                    }
                    // Kan-dora revealed by an earlier cmd_gang_bao_brc surfaces
                    // after this (the kan-caller's) discard.
                    for marker in self.status.pending_dora.drain(..) {
                        events.push(MjaiEvent::Dora {
                            dora_marker: marker,
                        });
                    }
                }
                13 => {
                    // Nukidora (北抜き, sanma) → mjai kita.
                    events.push(MjaiEvent::Kita {
                        actor,
                        pai: Some(field_card(action, "card")),
                    });
                }
                _ => {}
            }
        }
        events
    }

    /// `cmd_gang_bao_brc` — a new kan-dora indicator; deferred until the next
    /// discard.
    fn on_gang_bao_brc(&mut self, data: Option<&JsonValue>) -> Vec<MjaiEvent> {
        if let Some(cards) = data
            .and_then(|d| d.get("cards"))
            .and_then(JsonValue::as_array)
        {
            if let Some(last) = cards.last().and_then(json_i64) {
                self.status.pending_dora.push(card_to_mjai(last as u32));
            }
        }
        Vec::new()
    }

    /// `cmd_game_end` — per-kyoku settlement. Emits a `hora` per winner (with
    /// deltas + ura-dora) or a `ryukyoku`, then `end_kyoku`.
    fn on_game_end(&mut self, data: Option<&JsonValue>) -> Vec<MjaiEvent> {
        let mut events = self.flush_pending_reach();
        // Report the settlement's size for the round-advance pacing: the
        // client's score screen renders progressively per yaku, and a
        // mangan-plus takes visibly longer than five seconds to finish.
        if let Some(inject) = &self.inject {
            let hand = data
                .and_then(|d| d.get("win_info"))
                .and_then(JsonValue::as_array)
                .and_then(|ws| {
                    ws.iter()
                        .find(|w| field_i64(w, "all_point").unwrap_or(0) > 0)
                })
                .map(|w| {
                    let han = field_i64(w, "all_fang_num").unwrap_or(0).max(0) as u32;
                    let yakus = w
                        .get("fang_info")
                        .and_then(JsonValue::as_array)
                        .map_or(0, |a| a.len() as u32);
                    (han, yakus)
                })
                .unwrap_or((0, 0));
            inject.note_settlement(hand.0, hand.1);
        }
        let Some(data) = data else {
            events.push(MjaiEvent::EndKyoku);
            return events;
        };
        let end_type = field_i64(data, "end_type").unwrap_or(-1);
        let n = self.status.num_players as usize;

        // Per-actor deltas = point_profit (+ extra_profit for pao / special
        // payments, observed 0 in normal hands). This is the per-hand change
        // including honba and collected kyotaku but EXCLUDING the player's own
        // riichi stick: the −1000 for declaring riichi is applied by the mjai
        // consumer at reach_accepted, so folding it in here (e.g. via a
        // user_point running diff) would double-count it.
        let mut deltas = vec![0i32; n];
        if let Some(list) = data.get("user_profit").and_then(JsonValue::as_array) {
            for up in list {
                let Some(uid) = field_i64(up, "user_id") else {
                    continue;
                };
                let Some(actor) = self.status.actor_of(uid) else {
                    continue;
                };
                let a = actor as usize;
                if a >= n {
                    continue;
                }
                deltas[a] = (field_i64(up, "point_profit").unwrap_or(0)
                    + field_i64(up, "extra_profit").unwrap_or(0))
                    as i32;
            }
        }

        // A win_info entry is a real winner only when its hand has value
        // (all_point > 0). On an exhaustive draw (荒牌流局) win_info instead
        // lists the *tenpai* players with all_point == 0 — not winners — so the
        // presence of win_info alone must NOT be read as a win.
        let winners: Vec<&JsonValue> = data
            .get("win_info")
            .and_then(JsonValue::as_array)
            .map(|ws| {
                ws.iter()
                    .filter(|w| field_i64(w, "all_point").unwrap_or(0) > 0)
                    .collect()
            })
            .unwrap_or_default();

        if winners.is_empty() {
            // Any draw: exhaustive (荒牌流局), 九種九牌, or other abortive. The
            // deltas already carry tenpai/noten payments and riichi-stick moves.
            events.push(MjaiEvent::Ryukyoku {
                deltas: Some(deltas),
            });
        } else {
            // One hora per winner (multiple for double/triple ron).
            let is_tsumo = end_type == 1;
            for win in winners {
                let Some(uid) = field_i64(win, "user_id") else {
                    continue;
                };
                let Some(actor) = self.status.actor_of(uid) else {
                    continue;
                };
                let target = if is_tsumo {
                    actor
                } else {
                    self.status.last_dahai_actor.unwrap_or(actor)
                };
                // li_bao_card = ura-dora indicators (revealed on a riichi win).
                let ura = win
                    .get("li_bao_card")
                    .and_then(JsonValue::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(json_i64)
                            .map(|c| card_to_mjai(c as u32))
                            .collect::<Vec<_>>()
                    })
                    .filter(|v| !v.is_empty());
                events.push(MjaiEvent::Hora {
                    actor,
                    target,
                    deltas: Some(deltas.clone()),
                    ura_markers: ura,
                });
            }
        }

        events.push(MjaiEvent::EndKyoku);
        events
    }

    /// `cmd_room_end` — game over.
    fn on_room_end(&mut self) -> Vec<MjaiEvent> {
        self.status = GameStatus::default();
        if let Some(inject) = &self.inject {
            inject.set_in_game(false);
        }
        vec![MjaiEvent::end_game()]
    }

    fn flush_pending_reach(&mut self) -> Vec<MjaiEvent> {
        match self.status.pending_reach.take() {
            Some(ev) => vec![ev],
            None => Vec::new(),
        }
    }
}

impl Bridge for RiichiCityBridge {
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult {
        // Both directions are parsed: the CMDAuth uid is client→server, gameplay
        // is server→client. A frame normally holds one packet.
        let packets = WPacket::parse_frame(content);
        if packets.is_empty() {
            return ParseResult::empty();
        }
        // Track the client's uplink request counter (injected frames are
        // indexed past it).
        if matches!(direction, Direction::Up) {
            if let Some(bus) = &self.inject {
                for pkt in &packets {
                    bus.note_up_index(pkt.message_index);
                }
            }
        }
        let parsed = Some(ParsedFrame {
            method: packets[0].method_label(),
            args: packets[0].body.clone(),
        });
        let mut events = Vec::new();
        for pkt in &packets {
            events.extend(self.dispatch(pkt));
        }
        self.write_mjai(&events);
        ParseResult { events, parsed }
    }

    fn build(&mut self, command: &MjaiEvent) -> Option<Vec<u8>> {
        build::encode_action(command)
    }
}

// ============================================================================
// JSON / tile helpers
// ============================================================================

/// Coerce a JSON value to `i64`, accepting integers and numeric strings.
fn json_i64(v: &JsonValue) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(n) = v.as_u64() {
        return Some(n as i64);
    }
    v.as_str().and_then(|s| s.parse::<i64>().ok())
}

fn field_i64(data: &JsonValue, key: &str) -> Option<i64> {
    data.get(key).and_then(json_i64)
}

/// Read a tile-code field and convert to its mjai string (`?` when absent).
fn field_card(data: &JsonValue, key: &str) -> String {
    card_to_mjai(field_i64(data, key).unwrap_or(0) as u32)
}

/// Read a flag that may arrive as a JSON bool or a 0/1 integer.
fn field_bool(data: &JsonValue, key: &str) -> bool {
    match data.get(key) {
        Some(JsonValue::Bool(b)) => *b,
        Some(other) => json_i64(other).map(|n| n != 0).unwrap_or(false),
        None => false,
    }
}

/// Decode the `group_cards` array (tiles consumed from hand for a call).
fn group_cards(action: &JsonValue) -> Vec<String> {
    action
        .get("group_cards")
        .and_then(JsonValue::as_array)
        .map(|a| {
            a.iter()
                .map(|c| card_to_mjai(json_i64(c).unwrap_or(0) as u32))
                .collect()
        })
        .unwrap_or_default()
}

/// `v[i]` or `?` — keeps consumed arrays well-formed against short input.
fn idx(v: &[String], i: usize) -> String {
    v.get(i).cloned().unwrap_or_else(|| "?".to_string())
}

/// True for an mjai red-five string (`5mr`/`5pr`/`5sr`).
fn is_red(pai: &str) -> bool {
    matches!(pai, "5mr" | "5pr" | "5sr")
}

/// The red-five variant of a plain `5m`/`5p`/`5s`; otherwise the tile itself.
fn red_of(pai: &str) -> String {
    match pai {
        "5m" | "5p" | "5s" => format!("{pai}r"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// Frame a JSON body as a WPacket and run it through the bridge.
    fn feed(bridge: &mut RiichiCityBridge, cmd: u16, body: Value) -> Vec<MjaiEvent> {
        let json = serde_json::to_vec(&body).unwrap();
        let packet_size = (15 + json.len()) as u32;
        let mut buf = Vec::new();
        buf.extend_from_slice(&packet_size.to_be_bytes());
        buf.extend_from_slice(&[0x00, 0x0f, 0x00, 0x01]);
        buf.extend_from_slice(&0u32.to_be_bytes()); // message_index
        buf.extend_from_slice(&cmd.to_be_bytes());
        buf.push(1);
        buf.extend_from_slice(&json);
        bridge.parse(Direction::Down, &buf).events
    }

    // Fake ids per CLAUDE.md rule 8.
    const ME: i64 = 1001;

    fn auth(b: &mut RiichiCityBridge) {
        feed(b, CMD_AUTH, json!({ "uid": ME.to_string() }));
    }

    /// Every rsp_game_action must bump the ack counter autoplay verifies
    /// its injected frames against.
    #[test]
    fn rsp_game_action_bumps_the_ack_counter() {
        let inject = std::sync::Arc::new(crate::autoplay::inject::InjectBus::new());
        let mut bridge = RiichiCityBridge::new(None, None).with_inject(Some(inject.clone()));
        let ticket = inject.rsp_ticket();
        feed(
            &mut bridge,
            6,
            json!({ "cmd": "rsp_game_action", "data": { "code": 0 } }),
        );
        assert!(inject.rsp_since(ticket));
        assert_eq!(inject.last_rsp_code(), 0);

        feed(
            &mut bridge,
            6,
            json!({ "cmd": "rsp_game_action", "data": { "code": 4001 } }),
        );
        assert_eq!(inject.last_rsp_code(), 4001);
    }

    fn enter_room_4p(b: &mut RiichiCityBridge) {
        // Mirrors the real wire shape: string ids, room token on the wrapper.
        feed(
            b,
            18,
            json!({
                "cmd": "cmd_enter_room",
                "room_id": "tabletoken0001",
                "data": {
                    "options": {
                        "classify_id": "classifytoken0001",
                        "player_count": 4,
                        "stage_type": 1,
                        "game_play": 1001
                    },
                    "players": [
                        {"user": {"user_id": 1001, "nickname": "alice"}},
                        {"user": {"user_id": 1002, "nickname": "bob"}},
                        {"user": {"user_id": 1003, "nickname": "carol"}},
                        {"user": {"user_id": 1004, "nickname": "dave"}}
                    ]
                }
            }),
        );
    }

    /// Consecutive games in the same matchmaking queue share a
    /// `classify_id` but get distinct table tokens. The dedup must key on
    /// the table token: the second game's `cmd_enter_room` must be honored
    /// (fresh `start_game`) even without an intervening `cmd_room_end`.
    #[test]
    fn same_queue_new_table_is_not_deduped() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        enter_room_4p(&mut b);
        let start = json!({
            "cmd": "cmd_game_start",
            "data": {
                "quan_feng": 0x31, "bao_pai_card": 0x21, "dealer_pos": 0,
                "ben_chang_num": 0, "li_zhi_bang_num": 0,
                "user_info_list": [
                    {"user_id": 1001, "hand_points": 25000},
                    {"user_id": 1002, "hand_points": 25000},
                    {"user_id": 1003, "hand_points": 25000},
                    {"user_id": 1004, "hand_points": 25000}
                ],
                "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04,0x05]
            }
        });
        let events = feed(&mut b, 18, start.clone());
        assert!(matches!(events[0], MjaiEvent::StartGame { .. }));

        // Next game, same queue class, new table token.
        feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_enter_room",
                "room_id": "tabletoken0002",
                "data": {
                    "options": {
                        "classify_id": "classifytoken0001",
                        "player_count": 4,
                        "stage_type": 1,
                        "game_play": 1001
                    },
                    "players": [
                        {"user": {"user_id": 1001, "nickname": "alice"}},
                        {"user": {"user_id": 1002, "nickname": "bob"}},
                        {"user": {"user_id": 1003, "nickname": "carol"}},
                        {"user": {"user_id": 1004, "nickname": "dave"}}
                    ]
                }
            }),
        );
        let events = feed(&mut b, 18, start);
        match &events[0] {
            MjaiEvent::StartGame { game_meta, .. } => {
                match &game_meta.as_ref().expect("meta").match_info {
                    Some(MatchInfo::RiichiCity { room_id, .. }) => {
                        assert_eq!(room_id.as_deref(), Some("tabletoken0002"));
                    }
                    other => panic!("expected RiichiCity match_info, got {other:?}"),
                }
            }
            other => panic!("expected StartGame for the second game, got {other:?}"),
        }
    }

    /// A re-announcement of the *same* table (reconnect) is ignored, so a
    /// partial payload can't clobber the collected roster.
    #[test]
    fn duplicate_same_table_enter_room_keeps_the_roster() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        enter_room_4p(&mut b);
        feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_enter_room",
                "room_id": "tabletoken0001",
                "data": {
                    "options": { "classify_id": "classifytoken0001", "player_count": 4 },
                    "players": []
                }
            }),
        );
        assert_eq!(b.status.player_list, vec![1001, 1002, 1003, 1004]);
        assert_eq!(
            b.status.nicknames.get(&1001).map(String::as_str),
            Some("alice")
        );
    }

    /// A mid-game reconnect payload (from a live 2026-08-24 capture,
    /// ids/nicks swapped for fakes): `is_reconnect: true`, the same roster
    /// as the original table, `initial_dealer_pos` = the first hand's
    /// East dealer in roster order, and a non-null `hand_status` marking
    /// the running hand.
    fn reconnect_room(b: &mut RiichiCityBridge, hand_status: Value) {
        feed(
            b,
            18,
            json!({
                "cmd": "cmd_enter_room",
                "room_id": "reconnecttoken1",
                "data": {
                    "is_reconnect": true,
                    "initial_dealer_pos": 2,
                    "hand_status": hand_status,
                    "action_info": { "current_user_id": 1004, "current_action": null },
                    "options": {
                        "classify_id": "classifytoken0001",
                        "player_count": 4,
                        "stage_type": 1,
                        "game_play": 1001
                    },
                    "players": [
                        {"user": {"user_id": 1001, "nickname": "alice"}},
                        {"user": {"user_id": 1002, "nickname": "bob"}},
                        {"user": {"user_id": 1003, "nickname": "carol"}},
                        {"user": {"user_id": 1004, "nickname": "dave"}}
                    ]
                }
            }),
        );
    }

    /// The live incident: after a mid-game reconnect, our own decision
    /// window was attributed to the default `seat 0` — the tracker still
    /// believed our seat was the one from the original connection, so the
    /// bot was never asked and autoplay went blind for the rest of the
    /// game. The reconnect enter_room must resolve the rotation and seat
    /// itself, because the running hand's `cmd_game_start` never arrives.
    #[test]
    fn mid_hand_reconnect_resolves_seat_for_our_windows() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        reconnect_room(&mut b, json!({ "ben_chang_num": 0, "chang_ci": 2 }));
        // Roster [1001,1002,1003,1004] rotated left by 2 → actor order
        // [1003,1004,1001,1002]; our uid 1001 lands at seat 2.
        assert_eq!(b.status.seat, 2);
        assert!(b.status.seat_resolved);
        assert_eq!(b.status.shift, 2);
        assert!(!b.status.game_start, "mid-hand: start_game is not owed");
        assert_eq!(b.status.actor_of(1003), Some(0));
        assert_eq!(b.status.actor_of(1001), Some(2));

        // Our own draw prompt lands on the right actor — pre-fix this was
        // `actor: 0` and the bot never saw a decision for its seat.
        let events = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_send_current_action",
                "data": { "in_card": 0x11, "is_nine_cards": false }
            }),
        );
        assert!(matches!(
            &events[..],
            [MjaiEvent::Tsumo { actor: 2, pai }] if pai == "1s"
        ));
    }

    /// Mid-hand reconnect: the next kyoku boundary must NOT emit a second
    /// `start_game` (the tracker holds the game from the original
    /// connection), but the round math must stay correct through the
    /// reconnect-resolved `shift`.
    #[test]
    fn mid_hand_reconnect_next_kyoku_has_no_start_game_and_correct_oya() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        reconnect_room(&mut b, json!({ "ben_chang_num": 0, "chang_ci": 2 }));
        // East-2: dealer_pos 3 (roster coords) with shift 2 → oya 1.
        let events = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_start",
                "data": {
                    "quan_feng": 0x31, "bao_pai_card": 0x21, "dealer_pos": 3,
                    "ben_chang_num": 0, "li_zhi_bang_num": 0,
                    "user_info_list": [
                        {"user_id": 1001, "hand_points": 25000},
                        {"user_id": 1002, "hand_points": 25000},
                        {"user_id": 1003, "hand_points": 25000},
                        {"user_id": 1004, "hand_points": 25000}
                    ],
                    "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04]
                }
            }),
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, MjaiEvent::StartGame { .. })),
            "the tracker already holds this game's start_game"
        );
        let kyoku = events.iter().find_map(|e| match e {
            MjaiEvent::StartKyoku { oya, .. } => Some(*oya),
            _ => None,
        });
        assert_eq!(kyoku, Some(1), "East-2 dealer is actor 1 via shift 2");
    }

    /// Pre-deal reconnect (`hand_status` null — e.g. during the ready
    /// countdown): the coming `cmd_game_start` still owes a `start_game`
    /// (this connection's tracker has nothing), using the seat the
    /// reconnect already resolved — and must not re-rotate the rotated
    /// roster.
    #[test]
    fn pre_deal_reconnect_still_emits_start_game_without_double_rotation() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        reconnect_room(&mut b, Value::Null);
        assert!(b.status.game_start, "start_game still owed");
        assert_eq!(b.status.seat, 2);
        let events = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_start",
                "data": {
                    "quan_feng": 0x31, "bao_pai_card": 0x21, "dealer_pos": 2,
                    "ben_chang_num": 0, "li_zhi_bang_num": 0,
                    "user_info_list": [
                        {"user_id": 1001, "hand_points": 25000},
                        {"user_id": 1002, "hand_points": 25000},
                        {"user_id": 1003, "hand_points": 25000},
                        {"user_id": 1004, "hand_points": 25000}
                    ],
                    "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04]
                }
            }),
        );
        match events.iter().find_map(|e| match e {
            MjaiEvent::StartGame { names, id, .. } => Some((names.clone(), *id)),
            _ => None,
        }) {
            Some((names, id)) => {
                assert_eq!(id, Some(2));
                // Rotated actor order, not re-rotated.
                assert_eq!(names, vec!["carol", "dave", "alice", "bob"]);
            }
            None => panic!("pre-deal reconnect still owes a start_game"),
        }
    }

    #[test]
    fn yonma_game_start_emits_start_game_kyoku_tsumo() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        enter_room_4p(&mut b);
        // dealer_pos 0 → no rotation; ME is index 0 → seat 0 → we are dealer
        // (14-card hand).
        let events = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_start",
                "data": {
                    "quan_feng": 0x31,        // East
                    "bao_pai_card": 0x21,     // 1m
                    "dealer_pos": 0,
                    "ben_chang_num": 0,
                    "li_zhi_bang_num": 0,
                    "user_info_list": [
                        {"user_id": 1001, "hand_points": 25000},
                        {"user_id": 1002, "hand_points": 25000},
                        {"user_id": 1003, "hand_points": 25000},
                        {"user_id": 1004, "hand_points": 25000}
                    ],
                    "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04,0x05]
                }
            }),
        );
        assert_eq!(events.len(), 3);
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                num_players,
                names,
                game_meta,
                ..
            } => {
                assert_eq!(*id, Some(0));
                assert_eq!(*num_players, 4);
                // dealer_pos 0 → no rotation, so names stay in enter-room
                // order.
                assert_eq!(
                    names,
                    &vec![
                        "alice".to_string(),
                        "bob".to_string(),
                        "carol".to_string(),
                        "dave".to_string(),
                    ]
                );
                match &game_meta.as_ref().expect("riichi city meta").match_info {
                    Some(MatchInfo::RiichiCity {
                        room_id,
                        classify_id,
                        stage_type,
                        game_play,
                    }) => {
                        assert_eq!(room_id.as_deref(), Some("tabletoken0001"));
                        assert_eq!(classify_id.as_deref(), Some("classifytoken0001"));
                        assert_eq!(*stage_type, Some(1));
                        assert_eq!(*game_play, Some(1001));
                    }
                    other => panic!("expected RiichiCity match_info, got {other:?}"),
                }
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::StartKyoku {
                bakaze,
                dora_marker,
                kyoku,
                oya,
                scores,
                tehais,
                num_players,
                ..
            } => {
                assert_eq!(bakaze, "E");
                assert_eq!(dora_marker, "1m");
                assert_eq!(*kyoku, 1);
                assert_eq!(*oya, 0);
                assert_eq!(*num_players, 4);
                assert_eq!(scores, &vec![25000; 4]);
                assert_eq!(tehais.len(), 4);
                assert_eq!(tehais[0][0], "1m");
                assert_eq!(tehais[0][9], "1p");
                assert_eq!(tehais[1], vec!["?".to_string(); 13]);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        match &events[2] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "5p"); // the drawn 14th tile
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Regression for #215: `user_info_list` stays in fixed wire-seat order,
    /// while MJAI actors are rotated so actor 0 is the first dealer.
    #[test]
    fn game_start_maps_scores_to_rotated_actor_order() {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        enter_room_4p(&mut b);

        let events = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_start",
                "data": {
                    "quan_feng": 0x31,
                    "bao_pai_card": 0x21,
                    "dealer_pos": 2,
                    "ben_chang_num": 0,
                    "li_zhi_bang_num": 0,
                    "user_info_list": [
                        {"user_id": 1001, "hand_points": 11100},
                        {"user_id": 1002, "hand_points": 22200},
                        {"user_id": 1003, "hand_points": 33300},
                        {"user_id": 1004, "hand_points": 44400}
                    ],
                    "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04]
                }
            }),
        );

        let scores = events.iter().find_map(|event| match event {
            MjaiEvent::StartKyoku { scores, .. } => Some(scores),
            _ => None,
        });
        assert_eq!(scores, Some(&vec![33300, 44400, 11100, 22200]));

        // Names follow the same dealer rotation: enter-room order
        // [alice, bob, carol, dave] rotated by dealer_pos 2.
        let names = events.iter().find_map(|event| match event {
            MjaiEvent::StartGame { names, .. } => Some(names),
            _ => None,
        });
        assert_eq!(
            names,
            Some(&vec![
                "carol".to_string(),
                "dave".to_string(),
                "alice".to_string(),
                "bob".to_string(),
            ])
        );
    }

    #[test]
    fn sanma_uses_native_length_three() {
        let mut b = RiichiCityBridge::new(None, None);
        feed(&mut b, CMD_AUTH, json!({ "uid": "1002" }));
        feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_enter_room",
                "data": {
                    "options": { "classify_id": 1, "player_count": 3 },
                    "players": [
                        {"user": {"user_id": 1001}},
                        {"user": {"user_id": 1002}},
                        {"user": {"user_id": 1003}}
                    ]
                }
            }),
        );
        // dealer_pos 0; ME=1002 → seat 1; 13-card hand (not dealer).
        let events = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_start",
                "data": {
                    "quan_feng": 0x31, "bao_pai_card": 0x21, "dealer_pos": 0,
                    "ben_chang_num": 0, "li_zhi_bang_num": 0,
                    "user_info_list": [
                        {"user_id": 1001, "hand_points": 35000},
                        {"user_id": 1002, "hand_points": 35000},
                        {"user_id": 1003, "hand_points": 35000}
                    ],
                    "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04]
                }
            }),
        );
        let sg = events.iter().find_map(|e| match e {
            MjaiEvent::StartGame {
                id, num_players, ..
            } => Some((*id, *num_players)),
            _ => None,
        });
        assert_eq!(sg, Some((Some(1), 3)));
        let sk = events.iter().find_map(|e| match e {
            MjaiEvent::StartKyoku {
                scores,
                tehais,
                num_players,
                ..
            } => Some((scores.len(), tehais.len(), *num_players)),
            _ => None,
        });
        assert_eq!(sk, Some((3, 3, 3)));
        // Not the dealer → opening tsumo is the dealer's hidden draw.
        let tsumo = events.iter().find_map(|e| match e {
            MjaiEvent::Tsumo { actor, pai } => Some((*actor, pai.clone())),
            _ => None,
        });
        assert_eq!(tsumo, Some((0, "?".to_string())));
    }

    fn started_4p() -> RiichiCityBridge {
        let mut b = RiichiCityBridge::new(None, None);
        auth(&mut b);
        enter_room_4p(&mut b);
        feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_start",
                "data": {
                    "quan_feng": 0x31, "bao_pai_card": 0x21, "dealer_pos": 0,
                    "ben_chang_num": 0, "li_zhi_bang_num": 0,
                    "user_info_list": [
                        {"user_id": 1001, "hand_points": 25000},
                        {"user_id": 1002, "hand_points": 25000},
                        {"user_id": 1003, "hand_points": 25000},
                        {"user_id": 1004, "hand_points": 25000}
                    ],
                    "hand_cards": [0x21,0x22,0x23,0x24,0x25,0x26,0x27,0x28,0x29,0x01,0x02,0x03,0x04,0x05]
                }
            }),
        );
        b
    }

    #[test]
    fn other_player_draw_is_hidden() {
        let mut b = started_4p();
        let events = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_in_card_brc", "data": {"user_id": 1002, "card": 0}}),
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "?");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Tsumogiri is inferred from the rack-slot sentinel plus a live
    /// draw: `move_cards_pos[0]` equal to the seat's rack size
    /// (`13 - 3*melds + 1`) on a turn that drew — the client racks the
    /// held draw last and clamps tedashi below it.
    #[test]
    fn tsumogiri_needs_a_draw_and_the_drawn_slot() {
        let mut b = started_4p();
        // Seat 1 draws, then discards the drawn slot → tsumogiri.
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_in_card_brc", "data": {"user_id": 1002, "card": 0}}),
        );
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1002, "card": 0x25, "move_cards_pos": [14], "is_li_zhi": false}
            ]}}),
        );
        assert!(matches!(
            &e[0],
            MjaiEvent::Dahai {
                actor: 1,
                tsumogiri: true,
                ..
            }
        ));

        // Seat 2 draws but discards a lower slot → tedashi.
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_in_card_brc", "data": {"user_id": 1003, "card": 0}}),
        );
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1003, "card": 0x25, "move_cards_pos": [13], "is_li_zhi": false}
            ]}}),
        );
        assert!(matches!(
            &e[0],
            MjaiEvent::Dahai {
                actor: 2,
                tsumogiri: false,
                ..
            }
        ));
    }

    /// After a call the rack shrinks: the tsumogiri sentinel follows the
    /// seat's meld count, and a post-call discard (no draw) is never
    /// tsumogiri even when it names the rack's last slot.
    #[test]
    fn tsumogiri_slot_follows_the_melded_rack() {
        let mut b = started_4p();
        // Seat 1 draws and discards; we pon it and must now discard from
        // an 11-slot rack without drawing.
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_in_card_brc", "data": {"user_id": 1002, "card": 0}}),
        );
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1002, "card": 0x25, "move_cards_pos": [14], "is_li_zhi": false}
            ]}}),
        );
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 5, "user_id": 1001, "card": 0x25, "group_cards": [0x25, 0x25]}
            ]}}),
        );
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1001, "card": 0x31, "move_cards_pos": [11], "is_li_zhi": false}
            ]}}),
        );
        assert!(
            matches!(
                &e[0],
                MjaiEvent::Dahai {
                    actor: 0,
                    tsumogiri: false,
                    ..
                }
            ),
            "post-call discard is tedashi even at the last slot"
        );

        // Next turn we draw; slot 11 now IS the drawn slot.
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_send_current_action", "data": {"in_card": 0x05}}),
        );
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1001, "card": 0x05, "move_cards_pos": [11], "is_li_zhi": false}
            ]}}),
        );
        assert!(matches!(
            &e[0],
            MjaiEvent::Dahai {
                actor: 0,
                tsumogiri: true,
                ..
            }
        ));
    }

    #[test]
    fn reach_dahai_defers_reach_accepted_until_next_action() {
        let mut b = started_4p();
        // We discard declaring riichi.
        let e1 = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_action_brc",
                "data": {"action_info": [
                    {"action": 11, "user_id": 1001, "card": 0x29, "move_cards_pos": [5], "is_li_zhi": true}
                ]}
            }),
        );
        assert!(matches!(e1[0], MjaiEvent::Reach { actor: 0, .. }));
        assert!(
            matches!(&e1[1], MjaiEvent::Dahai { actor: 0, pai, tsumogiri: false } if pai == "9m")
        );
        // The next draw flushes the deferred reach_accepted first.
        let e2 = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_in_card_brc", "data": {"user_id": 1002, "card": 0}}),
        );
        assert!(matches!(e2[0], MjaiEvent::ReachAccepted { actor: 0 }));
        assert!(matches!(e2[1], MjaiEvent::Tsumo { actor: 1, .. }));
    }

    #[test]
    fn pon_targets_last_discarder() {
        let mut b = started_4p();
        // Seat 1 discards (sets last_dahai_actor = 1)…
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1002, "card": 0x25, "move_cards_pos": [14], "is_li_zhi": false}
            ]}}),
        );
        // …then we pon it.
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 5, "user_id": 1001, "card": 0x25, "group_cards": [0x25, 0x25]}
            ]}}),
        );
        match &e[0] {
            MjaiEvent::Pon {
                actor,
                target,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(*target, 1);
                assert_eq!(pai, "5m");
                assert_eq!(consumed, &["5m".to_string(), "5m".to_string()]);
            }
            other => panic!("expected Pon, got {other:?}"),
        }
    }

    #[test]
    fn ankan_marks_red_five() {
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 8, "user_id": 1001, "card": 0x25}
            ]}}),
        );
        match &e[0] {
            MjaiEvent::Ankan { actor, consumed } => {
                assert_eq!(*actor, 0);
                assert_eq!(
                    consumed,
                    &[
                        "5mr".to_string(),
                        "5m".to_string(),
                        "5m".to_string(),
                        "5m".to_string()
                    ]
                );
            }
            other => panic!("expected Ankan, got {other:?}"),
        }
    }

    #[test]
    fn kakan_with_red_added_tile_yields_plain_consumed() {
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 9, "user_id": 1001, "card": 0x125}
            ]}}),
        );
        match &e[0] {
            MjaiEvent::Kakan {
                actor,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "5mr");
                assert_eq!(
                    consumed,
                    &["5m".to_string(), "5m".to_string(), "5m".to_string()]
                );
            }
            other => panic!("expected Kakan, got {other:?}"),
        }
    }

    #[test]
    fn kan_dora_flushes_after_next_discard() {
        let mut b = started_4p();
        // Kan-dora indicator arrives (deferred)…
        let none = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_gang_bao_brc", "data": {"cards": [0x22]}}),
        );
        assert!(none.is_empty());
        // …and surfaces right after the next discard.
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1001, "card": 0x05, "move_cards_pos": [14], "is_li_zhi": false}
            ]}}),
        );
        assert!(matches!(&e[0], MjaiEvent::Dahai { pai, .. } if pai == "5p"));
        assert!(matches!(&e[1], MjaiEvent::Dora { dora_marker } if dora_marker == "2m"));
    }

    #[test]
    fn nukidora_maps_to_kita() {
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 13, "user_id": 1001, "card": 0x61}
            ]}}),
        );
        match &e[0] {
            MjaiEvent::Kita { actor, pai } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai.as_deref(), Some("N"));
            }
            other => panic!("expected Kita, got {other:?}"),
        }
    }

    #[test]
    fn win_action_alone_does_not_end_kyoku() {
        // Actions 7/10/12 only flag the end; the kyoku closes on cmd_game_end.
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 7, "user_id": 1001}
            ]}}),
        );
        assert!(e.is_empty(), "win action must not emit end_kyoku by itself");
    }

    #[test]
    fn game_end_ron_emits_hora_with_deltas_and_ura() {
        let mut b = started_4p(); // seat 0 = us (1001), scores all 25000
                                  // Seat 2 (1003) discards → ron target.
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [
                {"action": 11, "user_id": 1003, "card": 0x25, "move_cards_pos": [14], "is_li_zhi": false}
            ]}}),
        );
        // Ron flag (no-op) then the settlement.
        feed(
            &mut b,
            18,
            json!({"cmd": "cmd_game_action_brc", "data": {"action_info": [{"action": 7, "user_id": 1001}]}}),
        );
        let e = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_end",
                "data": {
                    "end_type": 0,
                    "win_info": [{"user_id": 1001, "all_point": 12000, "li_bao_card": [0x51]}],
                    // Winner riichi'd: point_profit 14000 (12000 + 2 sticks),
                    // loser -12000. user_point is set to a DIFFERENT running diff
                    // (+13000 / -13000) to prove deltas come from point_profit,
                    // not a user_point diff (the −1000 is at reach_accepted).
                    "user_profit": [
                        {"user_id": 1001, "user_point": 38000, "point_profit": 14000, "li_zhi_profit": 1000},
                        {"user_id": 1002, "user_point": 25000, "point_profit": 0, "li_zhi_profit": 0},
                        {"user_id": 1003, "user_point": 12000, "point_profit": -12000, "li_zhi_profit": -1000},
                        {"user_id": 1004, "user_point": 25000, "point_profit": 0, "li_zhi_profit": 0}
                    ]
                }
            }),
        );
        assert_eq!(e.len(), 2);
        match &e[0] {
            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ura_markers,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(*target, 2, "ron target is the last discarder");
                assert_eq!(
                    deltas.as_ref().unwrap(),
                    &vec![14000, 0, -12000, 0],
                    "deltas must be point_profit (excludes own riichi stick), not the user_point diff",
                );
                assert_eq!(ura_markers.as_deref(), Some(["W".to_string()].as_slice()));
            }
            other => panic!("expected Hora, got {other:?}"),
        }
        assert!(matches!(e[1], MjaiEvent::EndKyoku));
    }

    #[test]
    fn game_end_tsumo_targets_self_no_ura() {
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_end",
                "data": {
                    "end_type": 1,
                    "win_info": [{"user_id": 1003, "all_point": 6000}],
                    "user_profit": [
                        {"user_id": 1001, "point_profit": -2000},
                        {"user_id": 1002, "point_profit": -2000},
                        {"user_id": 1003, "point_profit": 6000},
                        {"user_id": 1004, "point_profit": -2000}
                    ]
                }
            }),
        );
        match &e[0] {
            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ura_markers,
            } => {
                assert_eq!(*actor, 2);
                assert_eq!(*target, 2, "tsumo target == winner");
                assert_eq!(deltas.as_ref().unwrap(), &vec![-2000, -2000, 6000, -2000]);
                assert!(ura_markers.is_none());
            }
            other => panic!("expected Hora, got {other:?}"),
        }
        assert!(matches!(e[1], MjaiEvent::EndKyoku));
    }

    #[test]
    fn game_end_abortive_draw_emits_ryukyoku() {
        // 九種九牌: end_type 6, empty win_info, no point change.
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_end",
                "data": {
                    "end_type": 6,
                    "win_info": [],
                    "user_profit": [
                        {"user_id": 1001, "point_profit": 0},
                        {"user_id": 1002, "point_profit": 0},
                        {"user_id": 1003, "point_profit": 0},
                        {"user_id": 1004, "point_profit": 0}
                    ]
                }
            }),
        );
        assert_eq!(e.len(), 2);
        match &e[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                assert_eq!(deltas.as_ref().unwrap(), &vec![0, 0, 0, 0]);
            }
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
        assert!(matches!(e[1], MjaiEvent::EndKyoku));
    }

    /// Regression (from a real capture): 荒牌流局 arrives as end_type 7 with
    /// a non-empty win_info that lists the *tenpai* players (all_point == 0).
    /// It must become one ryukyoku, not a hora per "winner".
    #[test]
    fn game_end_exhaustive_draw_with_tenpai_list_is_ryukyoku() {
        let mut b = started_4p(); // scores all 25000
        let e = feed(
            &mut b,
            18,
            json!({
                "cmd": "cmd_game_end",
                "data": {
                    "end_type": 7,
                    // Seats 1 and 3 tenpai (revealed), but all_point == 0.
                    "win_info": [
                        {"user_id": 1002, "all_point": 0, "all_fu": 0, "all_fang_num": 0},
                        {"user_id": 1004, "all_point": 0, "all_fu": 0, "all_fang_num": 0}
                    ],
                    // point_profit = tenpai +1500 / noten −1500. Seats 1 and 3
                    // also declared riichi (li_zhi_profit −1000), but that −1000
                    // is applied at reach_accepted, NOT in the ryukyoku deltas.
                    "user_profit": [
                        {"user_id": 1001, "point_profit": -1500, "li_zhi_profit": 0},
                        {"user_id": 1002, "point_profit": 1500, "li_zhi_profit": -1000},
                        {"user_id": 1003, "point_profit": -1500, "li_zhi_profit": 0},
                        {"user_id": 1004, "point_profit": 1500, "li_zhi_profit": -1000}
                    ]
                }
            }),
        );
        assert_eq!(e.len(), 2, "exactly one ryukyoku + end_kyoku, no hora");
        match &e[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                assert_eq!(
                    deltas.as_ref().unwrap(),
                    &vec![-1500, 1500, -1500, 1500],
                    "tenpai payment is +1500; the riichi −1000 is at reach_accepted",
                );
            }
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
        assert!(matches!(e[1], MjaiEvent::EndKyoku));
    }

    #[test]
    fn room_end_emits_end_game() {
        let mut b = started_4p();
        let e = feed(&mut b, 18, json!({"cmd": "cmd_room_end", "data": {}}));
        assert_eq!(e.len(), 1);
        assert!(matches!(e[0], MjaiEvent::EndGame { .. }));
    }

    #[test]
    fn unknown_cmd_is_ignored() {
        let mut b = started_4p();
        let e = feed(
            &mut b,
            18,
            json!({"cmd": "cmd_some_future_thing", "data": {}}),
        );
        assert!(e.is_empty());
    }
}
