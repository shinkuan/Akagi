//! ホロじゃん / HoloMahjong protocol bridge. Observe-only.
//!
//! The battle server is a MagicOnion StreamingHub reached over gRPC on
//! cleartext HTTP/2; the proxy relays and taps that connection itself (see
//! `proxy::h2c`) and hands this bridge one hub message per call. [`hub`]
//! decodes the envelope; this module turns the server's broadcasts into
//! mjai events. Tile, hand and meld strings follow majiang-core's notation —
//! see [`tile`].
//!
//! Bodies are MessagePack-CSharp objects serialized as arrays indexed by
//! their `[Key(n)]`. Event DTOs start at key 1, so index 0 is always nil;
//! nested structs start at key 0. The layouts in the comments below are
//! those keys.
//!
//! Seats on the wire are fixed for the whole game. mjai actor 0 is the
//! first dealer, so seats are rotated by that seat, which is recovered from
//! each deal (`dealer seat - hand number`) rather than assumed.

pub mod hub;
pub mod tile;

use super::{shown, Bridge, Direction, ParseResult};
use crate::{
    config::Platform,
    logger::{FlowLogger, Session},
    schema::{mjai::Tile, GameEndReason, MjaiEvent, ParsedFrame},
};
use chrono::Local;
use hub::HubMessage;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tile::Meld;
use tracing::{debug, info, warn};

const LOG: &str = "akagi::bridge::holomahjong";
const BAKAZE: [&str; 4] = ["E", "S", "W", "N"];
const HAND_SIZE: usize = 13;

pub struct HoloMahjongBridge {
    /// Our seat, from `OnKaiju` (or the deal, when we joined mid-game).
    my_seat: Option<u8>,
    /// Display names by seat, from `OnKaiju`.
    names: Vec<String>,
    num_players: u8,
    /// Seat of the first dealer; `None` until a deal has been seen.
    qijia: Option<u8>,
    /// This hand's seat for each wind (`PingjuEvent` reports by wind).
    seat_by_wind: Vec<u8>,
    /// `start_game` has gone out and no `end_game` yet.
    game_open: bool,
    /// Inside a hand: between `start_kyoku` and `end_kyoku`.
    kyoku_open: bool,
    flow_log: Option<Arc<FlowLogger>>,
    session: Option<Arc<Session>>,
    mjai_log: Option<Arc<FlowLogger>>,
}

impl HoloMahjongBridge {
    pub fn new(flow_log: Option<Arc<FlowLogger>>, session: Option<Arc<Session>>) -> Self {
        Self {
            my_seat: None,
            names: Vec::new(),
            num_players: 4,
            qijia: None,
            seat_by_wind: Vec::new(),
            game_open: false,
            kyoku_open: false,
            flow_log,
            session,
            mjai_log: None,
        }
    }

    fn actor(&self, seat: u8) -> Option<u8> {
        let n = self.num_players;
        let qijia = self.qijia?;
        (seat < n).then(|| (seat + n - qijia) % n)
    }

    /// Per-seat values (`[[seat, value, …], …]`, any order) as a vector in
    /// actor order. `seat_at`/`value_at` index into each entry.
    fn by_actor(&self, entries: &JsonValue, seat_at: usize, value_at: usize) -> Option<Vec<i32>> {
        let mut out = vec![0; self.num_players as usize];
        for e in entries.as_array()? {
            let seat = seat_of(e.get(seat_at)?)?;
            out[self.actor(seat)? as usize] = i32::try_from(e.get(value_at)?.as_i64()?).ok()?;
        }
        Some(out)
    }

    fn dispatch(&mut self, msg: &HubMessage) -> Vec<MjaiEvent> {
        let b = &msg.body;
        let events = match msg.method {
            hub::ON_KAIJU => self.on_kaiju(b),
            // A reconnect opens with the game's KaijuEvent; the hand in
            // progress is not rebuilt, play resumes at the next deal.
            hub::ON_ZHUNBEI_CHONGLIAN if !self.game_open => b.get(1).and_then(|k| self.on_kaiju(k)),
            hub::ON_QIPAI => self.on_qipai(b),
            hub::ON_ZIMO if self.kyoku_open => self.on_zimo(b),
            hub::ON_DAPAI if self.kyoku_open => self.on_dapai(b),
            hub::ON_FULOU if self.kyoku_open => self.on_fulou(b),
            hub::ON_AN_GANG_ADD_GANG if self.kyoku_open => self.on_fulou(b),
            hub::ON_ADD_BAOPAI if self.kyoku_open => self.on_add_baopai(b),
            hub::ON_LIZHI if self.kyoku_open => self.on_lizhi(b),
            hub::ON_HULE if self.kyoku_open => self.on_hule(b),
            hub::ON_PINGJU if self.kyoku_open => self.on_pingju(b),
            hub::ON_JIEJU if self.game_open => self.on_jieju(b),
            hub::ON_UPDATE_END_GAME_VOTE if self.game_open => Some(self.on_end_game_vote(b)),
            _ => Some(Vec::new()),
        };
        events.unwrap_or_else(|| {
            warn!(target: LOG, "unexpected {} body shape; skipped", msg.label());
            Vec::new()
        })
    }

    /// KaijuEvent `[_, game_info, seat, rule, timeout, players, extended_rules,
    /// default_field]`. `rule[1]` is the player count; each player is a
    /// MessagePack union `[0, [seat, account, detail, connected, is_ai]]`
    /// with `detail[1]` the display name.
    fn on_kaiju(&mut self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let mut events = Vec::new();
        if self.game_open {
            // A game that ended without a result (the end-of-game vote).
            events.push(end_game(GameEndReason::Terminated));
        }
        let n = b
            .get(3)
            .and_then(|r| r.get(1))
            .and_then(JsonValue::as_u64)
            .unwrap_or(4) as u8;
        if n != 4 {
            warn!(target: LOG, "{n}-player game: only 4-player is supported; ignoring it");
        }
        let mut names = vec![String::new(); n as usize];
        for p in b.get(5)?.as_array()? {
            let info = p.get(1)?;
            let seat = seat_of(info.get(0)?)? as usize;
            if let (Some(slot), Some(name)) = (
                names.get_mut(seat),
                info.get(2)
                    .and_then(|x| x.get(1))
                    .and_then(JsonValue::as_str),
            ) {
                *slot = name.to_string();
            }
        }
        *self = Self {
            my_seat: Some(seat_of(b.get(2)?)?),
            names,
            num_players: n,
            flow_log: self.flow_log.take(),
            session: self.session.take(),
            ..Self::new(None, None)
        };
        info!(target: LOG, "game opened: seat {:?}", self.my_seat);
        Some(events)
    }

    /// QipaiEvent `[_, seat, server_time, deadline, field, players, timeout]`
    /// with field `[round_wind, hand_no, honba, riichi_sticks, dora_indicators,
    /// wall, …]` and each player `[account, seat, wind, score, chip, hand,
    /// discards, wins, riichi, kita]`.
    fn on_qipai(&mut self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        if self.num_players != 4 {
            return Some(Vec::new());
        }
        let n = self.num_players;
        let field = b.get(4)?;
        let round_wind = field.get(0)?.as_u64()? as usize;
        let hand_no = field.get(1)?.as_u64()? as u8;
        let honba = field.get(2)?.as_u64()? as u8;
        let kyotaku = kyotaku(field.get(3)?)?;
        let dora_marker = tile::tile(field.get(4)?.get(0)?.as_str()?)?;
        let players = b.get(5)?.as_array()?;
        let dealer = players
            .iter()
            .find(|p| p.get(2).and_then(JsonValue::as_u64) == Some(0))
            .and_then(|p| seat_of(p.get(1)?))?;
        let me = seat_of(b.get(1)?)?;
        self.my_seat.get_or_insert(me);
        let mut seat_by_wind = vec![0; n as usize];
        for p in players {
            let wind = p.get(2)?.as_u64()? as usize;
            *seat_by_wind.get_mut(wind)? = seat_of(p.get(1)?)?;
        }
        self.seat_by_wind = seat_by_wind;
        self.qijia = Some((dealer + n - hand_no % n) % n);

        let mut events = Vec::new();
        if !self.game_open {
            self.rotate_mjai_log();
            let names = (0..n)
                .map(|a| {
                    let seat = ((a + self.qijia?) % n) as usize;
                    Some(self.names.get(seat).cloned().unwrap_or_default())
                })
                .collect::<Option<Vec<_>>>()?;
            events.push(MjaiEvent::StartGame {
                names,
                kyoku_first: None,
                aka_flag: None,
                id: self.actor(self.my_seat?),
                num_players: n,
                game_meta: None,
            });
            self.game_open = true;
        }
        let mut scores = vec![0; n as usize];
        let mut tehais = vec![vec!["?".to_string(); HAND_SIZE]; n as usize];
        for p in players {
            let seat = seat_of(p.get(1)?)?;
            let a = self.actor(seat)? as usize;
            scores[a] = i32::try_from(p.get(3)?.as_i64()?).ok()?;
            if Some(seat) == self.my_seat {
                let hand = tile::hand(p.get(5)?.as_str()?)?;
                if hand.len() != HAND_SIZE {
                    return None;
                }
                tehais[a] = hand;
            }
        }
        events.push(MjaiEvent::StartKyoku {
            bakaze: BAKAZE.get(round_wind)?.to_string(),
            dora_marker,
            kyoku: hand_no + 1,
            honba,
            kyotaku,
            oya: self.actor(dealer)?,
            scores,
            tehais,
            num_players: n,
            catch_up: 0,
        });
        self.kyoku_open = true;
        Some(events)
    }

    /// ZimoEvent `[_, seat, tile, kan_offers, kita_offers, tsumo_offer,
    /// timeout]`; the tile is `""` for another seat's draw.
    fn on_zimo(&self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let actor = self.actor(seat_of(b.get(1)?)?)?;
        let pai = match b.get(2)?.as_str()? {
            "" => "?".to_string(),
            t => tile::tile(t)?,
        };
        Some(vec![MjaiEvent::Tsumo { actor, pai }])
    }

    /// DapaiEvent `[_, seat, discard, chi_offers, pon_offers, kan_offers,
    /// ron_offer, call_advice, riichi_type, hand_after, timeout]`.
    fn on_dapai(&self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let actor = self.actor(seat_of(b.get(1)?)?)?;
        let d = tile::discard(b.get(2)?.as_str()?)?;
        let mut events = Vec::new();
        let riichi_type = b.get(8).and_then(JsonValue::as_u64).unwrap_or(0);
        if d.riichi || riichi_type != 0 {
            events.push(MjaiEvent::Reach { actor, pai: None });
        }
        events.push(MjaiEvent::Dahai {
            actor,
            pai: d.pai,
            tsumogiri: d.tsumogiri,
        });
        Some(events)
    }

    /// FulouEvent and AnGangAddGangEvent `[_, seat, kind, meld, …]`; the
    /// meld string alone says which call it is.
    fn on_fulou(&self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let seat = seat_of(b.get(1)?)?;
        self.meld_event(seat, b.get(3)?.as_str()?).map(|e| vec![e])
    }

    fn meld_event(&self, seat: u8, s: &str) -> Option<MjaiEvent> {
        let actor = self.actor(seat)?;
        let target = |from: tile::Source| self.actor(from.seat(seat, self.num_players));
        Some(match tile::meld(s)? {
            Meld::Chi {
                from,
                pai,
                consumed,
            } => MjaiEvent::Chi {
                actor,
                target: target(from)?,
                pai,
                consumed,
            },
            Meld::Pon {
                from,
                pai,
                consumed,
            } => MjaiEvent::Pon {
                actor,
                target: target(from)?,
                pai,
                consumed,
            },
            Meld::Daiminkan {
                from,
                pai,
                consumed,
            } => MjaiEvent::Daiminkan {
                actor,
                target: target(from)?,
                pai,
                consumed,
            },
            Meld::Kakan { pai, consumed } => MjaiEvent::Kakan {
                actor,
                pai,
                consumed,
            },
            Meld::Ankan { consumed } => MjaiEvent::Ankan { actor, consumed },
        })
    }

    /// LizhiEvent `[_, seat, riichi_type, riichi_sticks, scores_by_seat]` —
    /// sent once the riichi discard has passed, i.e. the stick is down.
    fn on_lizhi(&self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let actor = self.actor(seat_of(b.get(1)?)?)?;
        Some(vec![MjaiEvent::ReachAccepted { actor }])
    }

    /// HuleEvent `[_, results, scores, timeout]`; each result is `[seat,
    /// dealt_in_seat, hand, win_tile, fu, limit, han, yaku, points, chip, dora,
    /// ura_dora]` (`dealt_in_seat` nil on a tsumo) and scores `[[seat, delta,
    /// pao, chip], …]`, riichi sticks and honba included.
    fn on_hule(&mut self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let deltas = self.by_actor(b.get(2)?, 0, 1)?;
        let mut events = Vec::new();
        for (k, h) in b.get(1)?.as_array()?.iter().enumerate() {
            let ura: Vec<Tile> = h
                .get(11)
                .and_then(JsonValue::as_array)
                .map(|v| v.iter().filter_map(|t| tile::tile(t.as_str()?)).collect())
                .unwrap_or_default();
            let winner = seat_of(h.get(0)?)?;
            let dealt_in = match h.get(1)? {
                JsonValue::Null => winner,
                v => seat_of(v)?,
            };
            events.push(MjaiEvent::Hora {
                actor: self.actor(winner)?,
                target: self.actor(dealt_in)?,
                // The wire only carries the combined result; later winners
                // get zeros so the per-hand total stays right.
                deltas: Some(if k == 0 {
                    deltas.clone()
                } else {
                    vec![0; deltas.len()]
                }),
                ura_markers: (!ura.is_empty()).then_some(ura),
            });
        }
        if events.is_empty() {
            return None;
        }
        events.push(MjaiEvent::EndKyoku);
        self.kyoku_open = false;
        Some(events)
    }

    /// PingjuEvent `[_, kind, players, timeout]`; each player is `[_, wind,
    /// delta, hand, nagashi]` — by wind, not seat.
    fn on_pingju(&mut self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let mut deltas = vec![0; self.num_players as usize];
        for p in b.get(2)?.as_array()? {
            let wind = p.get(1)?.as_u64()? as usize;
            let seat = *self.seat_by_wind.get(wind)?;
            deltas[self.actor(seat)? as usize] = i32::try_from(p.get(2)?.as_i64()?).ok()?;
        }
        self.kyoku_open = false;
        Some(vec![
            MjaiEvent::Ryukyoku {
                deltas: Some(deltas),
            },
            MjaiEvent::EndKyoku,
        ])
    }

    /// AddBaopaiEvent `[_, dora_indicator]` — after a kan.
    fn on_add_baopai(&self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let dora_marker = tile::tile(b.get(1)?.as_str()?)?;
        Some(vec![MjaiEvent::Dora { dora_marker }])
    }

    /// JiejuEvent `[_, players, timeout, result]`; each player is `[_, seat,
    /// rank, score, points, …]`.
    fn on_jieju(&mut self, b: &JsonValue) -> Option<Vec<MjaiEvent>> {
        let n = self.num_players as usize;
        let (mut scores, mut ranks) = (vec![0; n], vec![0; n]);
        for p in b.get(1)?.as_array()? {
            let a = self.actor(seat_of(p.get(1)?)?)? as usize;
            ranks[a] = u8::try_from(p.get(2)?.as_u64()?).ok()?;
            scores[a] = i32::try_from(p.get(3)?.as_i64()?).ok()?;
        }
        self.game_open = false;
        self.kyoku_open = false;
        Some(vec![MjaiEvent::EndGame {
            reason: GameEndReason::Confirmed,
            final_scores: Some(scores),
            final_ranks: Some(ranks),
        }])
    }

    /// UpDateEndGameVoteEvent `[_, end_result, agreements]`; `end_result` 0
    /// means the table agreed to stop, which ends the game without a result.
    fn on_end_game_vote(&mut self, b: &JsonValue) -> Vec<MjaiEvent> {
        if b.get(1).and_then(JsonValue::as_u64) != Some(0) {
            return Vec::new();
        }
        self.game_open = false;
        self.kyoku_open = false;
        vec![end_game(GameEndReason::Terminated)]
    }

    /// Open a fresh `holomahjong_<ts>.mjai.jsonl` per game. No-op without a
    /// session (tests).
    fn rotate_mjai_log(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let ts = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
        let file_name = format!("holomahjong_{ts}.mjai.jsonl");
        match session.flow_logger(
            Platform::HoloMahjong.subdir(),
            &file_name,
            format!("holomahjong mjai {ts}"),
        ) {
            Ok(log) => self.mjai_log = Some(log),
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
}

fn seat_of(v: &JsonValue) -> Option<u8> {
    v.as_u64().and_then(|s| u8::try_from(s).ok())
}

/// mjai `kyotaku` (1000-point sticks) from LizhiBangStruct, which counts
/// sticks of 100 / 500 / 1000 / 5000 / 10000 points.
fn kyotaku(sticks: &JsonValue) -> Option<u8> {
    const VALUES: [u64; 5] = [100, 500, 1000, 5000, 10000];
    let counts = sticks.as_array()?;
    let points = VALUES
        .iter()
        .zip(counts)
        .map(|(v, c)| c.as_u64().map(|c| c * v))
        .sum::<Option<u64>>()?;
    u8::try_from(points / 1000).ok()
}

fn end_game(reason: GameEndReason) -> MjaiEvent {
    MjaiEvent::EndGame {
        reason,
        final_scores: None,
        final_ranks: None,
    }
}

/// Messages whose content may go into the shareable logs. `JoinRoomAsync`
/// carries the account and battle-session ids, `OnKaiju` and the reconnect
/// snapshots every player's account id and name, and `OnJieju` the result
/// screen's account data, so none of them is on it.
fn is_shareable(msg: &HubMessage) -> bool {
    if msg.method == hub::ON_QIPAI {
        // Each player carries an account slot; it has only been seen nil.
        return msg
            .body
            .get(5)
            .and_then(JsonValue::as_array)
            .is_some_and(|ps| ps.iter().all(|p| p.get(0).is_some_and(JsonValue::is_null)));
    }
    matches!(
        msg.method,
        hub::ON_ZIMO
            | hub::ON_DAPAI
            | hub::ON_FULOU
            | hub::ON_LIZHI
            | hub::ON_HULE
            | hub::ON_PINGJU
            | hub::ON_UPDATE_FIELD_INFO
            | hub::ON_UPDATE_PLAYER_INFO
            | hub::ON_END_GAME_VOTE_START
            | hub::ON_UPDATE_END_GAME_VOTE
            | hub::ON_RESPONSE_END_GAME_VOTE
            | hub::ON_AN_GANG_ADD_GANG
            | hub::ON_ADD_BAOPAI
            | hub::ON_HUANG_PAI
            | hub::ON_BABEI
            | hub::READY
            | hub::KAIJU_EVENT_COMPLETE
            | hub::QIPAI_EVENT_COMPLETE
            | hub::SELECT_DAPAI
            | hub::SELECT_FULOU
            | hub::SELECT_ACTION_SKIP
            | hub::SELECT_HULE
            | hub::SELECT_AN_GANG_ADD_GANG
            | hub::SELECT_DAOPAI
            | hub::RESULT_COMPLETE
            | hub::RESUME_CONTROL
            | hub::END_GAME_VOTE_START
    )
}

impl Bridge for HoloMahjongBridge {
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult {
        let Some(msg) = hub::decode(content) else {
            debug!(target: LOG, "{} non-hub message, {} bytes", direction.as_str(), content.len());
            return ParseResult::empty();
        };
        let shareable = is_shareable(&msg);
        if let Some(flow) = &self.flow_log {
            flow.writeln(&format!(
                "{} {} {}",
                direction.as_str(),
                msg.label(),
                shown(shareable, &msg.body)
            ));
        }
        // Only broadcasts drive play; requests, responses and our own calls
        // are recorded but carry nothing the broadcasts don't.
        let events = if direction == Direction::Down && msg.message_id.is_none() {
            self.dispatch(&msg)
        } else {
            Vec::new()
        };
        self.write_mjai(&events);
        let method = match msg.message_id {
            Some(_) if direction == Direction::Down => format!("{} (response)", msg.label()),
            _ => msg.label(),
        };
        ParseResult {
            events,
            parsed: Some(ParsedFrame {
                method,
                args: msg.body,
            }),
            shareable,
        }
    }

    fn build(&mut self, _command: &MjaiEvent) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests;
