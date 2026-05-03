//! Majsoul protocol bridge.
//!
//! Wire format (see `parser.rs` for full layout):
//!   `[type byte] [msg_id u16 LE?] [Wrapper protobuf] ...`
//!
//! Type byte: 1=Notify, 2=Request, 3=Response. Request/Response carry a
//! little-endian u16 message id at offset 1..3; Notify does not.
//!
//! Each WebSocket flow has its own id sequence and pending request map, so
//! create one `MajsoulBridge` per connection.

pub mod parser;
pub mod tile;

use super::{Bridge, Direction, ParseResult};
use crate::{
    config::Platform,
    logger::{FlowLogger, Session},
    schema::{mjai::Actor, MjaiEvent},
};
use anyhow::{bail, Context, Result};
use chrono::Local;
use parser::{LiqiParser, MessageType, ParsedMessage};
use serde_json::{json, Value as JsonValue};
use std::{collections::HashMap, sync::Arc};
use tile::{compare_pai, ms_to_mjai};
use tracing::{info, warn};

const METHOD_AUTH_GAME: &str = ".lq.FastTest.authGame";
const METHOD_ACTION_PROTOTYPE: &str = ".lq.ActionPrototype";
const METHOD_NOTIFY_GAME_END_RESULT: &str = ".lq.NotifyGameEndResult";
const ACTION_NEW_ROUND: &str = "ActionNewRound";
const ACTION_DEAL_TILE: &str = "ActionDealTile";
const ACTION_DISCARD_TILE: &str = "ActionDiscardTile";
const ACTION_CHI_PENG_GANG: &str = "ActionChiPengGang";
const ACTION_AN_GANG_ADD_GANG: &str = "ActionAnGangAddGang";
const ACTION_HULE: &str = "ActionHule";
const ACTION_NO_TILE: &str = "ActionNoTile";
const ACTION_LIU_JU: &str = "ActionLiuJu";
const ACTION_BA_BEI: &str = "ActionBaBei";

// ChiPengGang.type
const CHI_PENG_GANG_CHI: u64 = 0;
const CHI_PENG_GANG_PENG: u64 = 1;
const CHI_PENG_GANG_GANG: u64 = 2;

// AnGangAddGang.type
const AN_GANG_ADD_GANG_AN: u64 = 3;
const AN_GANG_ADD_GANG_ADD: u64 = 2;
const TEHAI_SIZE: usize = 13;
const TSUMO_TEHAI_SIZE: usize = 14;
const UNKNOWN_TILE: &str = "?";

/// Per-flow Majsoul state. Holds the liqi parser and the game state mirror
/// needed to emit mjai events.
///
/// `account_id` is captured from the outbound `authGame` request; `seat` is
/// resolved on the matching response by indexing into `seat_list`. Once the
/// seat is known the bridge emits `start_game` with `id = seat` so downstream
/// bots know which seat is theirs.
///
/// `session`, when supplied, lets the bridge rotate a fresh
/// `<session>/majsoul/majsoul_<ts>.mjai.jsonl` file every time a new
/// `start_game` event is emitted. Each subsequent emitted `MjaiEvent` is
/// appended as one JSON line to that file.
/// Dora-flip scheduling for kan declarations. Mjai distinguishes ankan
/// (即乗り — dora flips *before* the rinshan tsumo) from kakan/daiminkan
/// (後乗り — dora flips *after* the rinshan tsumo, just before the next
/// dahai). Both Akagi-Python and AkagiNG conflate the two and emit dora
/// at the wrong moment for kakan/daiminkan; this state machine fixes that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoraTiming {
    /// Ankan just declared. The next `ActionDealTile` carries the new dora
    /// marker — emit `dora` *before* the rinshan `tsumo`.
    PendingBeforeRinshan,
    /// Kakan or daiminkan just declared. The next `ActionDealTile` still
    /// emits `tsumo` first; the new dora marker is held back and flipped
    /// just before the next `dahai`.
    PendingAfterRinshan,
}

pub struct MajsoulBridge {
    parser: LiqiParser,
    flow_log: Option<Arc<FlowLogger>>,
    session: Option<Arc<Session>>,
    mjai_log: Option<Arc<FlowLogger>>,
    account_id: Option<u64>,
    seat: Option<Actor>,
    /// Mjai-mapped dora indicators seen so far this kyoku. Used to detect
    /// new dora markers in subsequent `ActionDealTile.doras`.
    doras: Vec<String>,
    /// Pending kan-dora timing, set by meld actions and consumed by
    /// `build_tsumo` / `build_dahai`.
    dora_timing: Option<DoraTiming>,
    /// Mjai-mapped dora marker held for emission immediately before the
    /// next `dahai` (kakan / daiminkan flow).
    deferred_dora: Option<String>,
    /// Seat of the actor whose most recent action *revealed* a tile that
    /// another seat could ron — i.e. the legitimate `target` for a hora.
    /// Updated by:
    ///
    /// - `ActionDiscardTile` — normal ron.
    /// - `ActionAnGangAddGang(kakan)` — 搶槓 (chankan).
    /// - `ActionAnGangAddGang(ankan)` — 国士無双 robbing an ankan
    ///   (Majsoul-specific rule; only valid for kokushi musou).
    /// - `ActionBaBei` (3p) — 胡拔北; will be wired when 3p support lands.
    ///
    /// `HuleInfo` doesn't carry the target seat itself, so without this
    /// tracking a ron on a kan / babei would attribute the win to the
    /// wrong player. Reset at every `start_kyoku`.
    last_revealed_tile_actor: Option<Actor>,
    /// Seat that just declared riichi via `ActionDiscardTile.is_liqi`.
    /// Drained as a `reach_accepted` event prepended to the *next* action
    /// (per mjai state machine: declaration tile must pass through before
    /// the seat is debited 1000 points). Cleared without emission if the
    /// next action is `ActionHule` — a ron on the declaration tile voids
    /// the riichi.
    pending_reach_accepted: Option<Actor>,
    /// 3 (sanma) or 4 (yonma). Resolved from `seat_list.len()` on the
    /// `authGame` response. Defaults to 4 so per-flow code that runs before
    /// auth (none today, but be defensive) sees a sane value.
    num_players: u8,
}

impl MajsoulBridge {
    pub fn new(flow_log: Option<Arc<FlowLogger>>, session: Option<Arc<Session>>) -> Self {
        Self {
            parser: LiqiParser::new(),
            flow_log,
            session,
            mjai_log: None,
            account_id: None,
            seat: None,
            doras: Vec::new(),
            dora_timing: None,
            deferred_dora: None,
            last_revealed_tile_actor: None,
            pending_reach_accepted: None,
            num_players: 4,
        }
    }

    /// Open a fresh `majsoul_<ts>.mjai.jsonl` in the platform subdir and
    /// install it as the current mjai log. No-op when no session is wired.
    fn rotate_mjai_log(&mut self) {
        let Some(session) = &self.session else { return };
        let ts = Local::now().format("%Y%m%d-%H%M%S%.3f").to_string();
        let file_name = format!("majsoul_{ts}.mjai.jsonl");
        let label = format!("majsoul mjai {ts}");
        match session.flow_logger(Platform::Majsoul.subdir(), &file_name, label) {
            Ok(log) => {
                info!(
                    target: "akagi::bridge::majsoul",
                    "opened mjai log {file_name}"
                );
                self.mjai_log = Some(log);
            }
            Err(e) => {
                warn!(
                    target: "akagi::bridge::majsoul",
                    "failed to open mjai log {file_name}: {e:#}"
                );
                self.mjai_log = None;
            }
        }
    }

    /// Append every `MjaiEvent` in `events` as a JSON line to the current
    /// mjai log (if any).
    fn write_mjai(&self, events: &[MjaiEvent]) {
        let Some(log) = &self.mjai_log else { return };
        for ev in events {
            match serde_json::to_string(ev) {
                Ok(line) => log.writeln(&line),
                Err(e) => warn!(
                    target: "akagi::bridge::majsoul",
                    "failed to serialize MjaiEvent: {e:#}"
                ),
            }
        }
    }

    /// Translate `msg` into 0+ mjai events, mutating bridge state as needed.
    fn dispatch(&mut self, msg: &ParsedMessage) -> Vec<MjaiEvent> {
        match (&msg.msg_type, msg.method_name.as_ref()) {
            (MessageType::Request, METHOD_AUTH_GAME) => {
                self.account_id = msg.payload.get("account_id").and_then(JsonValue::as_u64);
                if self.account_id.is_none() {
                    warn!(
                        target: "akagi::bridge::majsoul",
                        "authGame request missing account_id: {}",
                        msg.payload
                    );
                }
                Vec::new()
            }
            (MessageType::Response, METHOD_AUTH_GAME) => {
                self.handle_auth_game_response(&msg.payload)
            }
            (MessageType::Notify, METHOD_ACTION_PROTOTYPE) => self.handle_action_prototype(msg),
            (MessageType::Notify, METHOD_NOTIFY_GAME_END_RESULT) => {
                // `result.players[]` carries final standings. Mjai
                // `end_game` has no payload — the standings live in the
                // flow log if anyone needs them. Emitting an empty event
                // is sufficient to terminate the mjai stream.
                info!(
                    target: "akagi::bridge::majsoul",
                    "game ended: {}", msg.payload
                );
                vec![MjaiEvent::EndGame]
            }
            _ => Vec::new(),
        }
    }

    fn handle_action_prototype(&mut self, msg: &ParsedMessage) -> Vec<MjaiEvent> {
        let action_name = msg.payload.get("name").and_then(JsonValue::as_str);
        let action_data = msg.payload.get("data");
        let (Some(action_name), Some(action_data)) = (action_name, action_data) else {
            warn!(
                target: "akagi::bridge::majsoul",
                "ActionPrototype payload missing name/data: {}", msg.payload
            );
            return Vec::new();
        };

        // Drain queued reach_accepted before the next non-Hule action. A
        // Hule on the riichi declaration tile voids the riichi, so we
        // discard the queued event in that case.
        let pending_reach = if action_name == ACTION_HULE || action_name == ACTION_DISCARD_TILE {
            // ActionDiscardTile right after a riichi declaration shouldn't
            // happen in practice (the declarer can't immediately discard
            // again), but if it does we leave the queue alone so the next
            // legitimate action drains it.
            // ActionHule clears the queue.
            if action_name == ACTION_HULE {
                self.pending_reach_accepted = None;
            }
            None
        } else {
            self.pending_reach_accepted.take()
        };

        let result = match action_name {
            ACTION_NEW_ROUND => self.build_start_kyoku(action_data),
            ACTION_DEAL_TILE => self.build_tsumo(action_data),
            ACTION_DISCARD_TILE => self.build_dahai(action_data),
            ACTION_CHI_PENG_GANG => self.build_chi_peng_gang(action_data),
            ACTION_AN_GANG_ADD_GANG => self.build_an_gang_add_gang(action_data),
            ACTION_NO_TILE => self.build_no_tile(action_data),
            ACTION_LIU_JU => self.build_liu_ju(action_data),
            ACTION_HULE => self.build_hule(action_data),
            ACTION_BA_BEI => self.build_kita(action_data),
            _ => return Vec::new(),
        };
        let events = match result {
            Ok(events) => events,
            Err(e) => {
                warn!(
                    target: "akagi::bridge::majsoul",
                    "{action_name} → mjai conversion failed: {e:#}"
                );
                Vec::new()
            }
        };

        match pending_reach {
            Some(actor) => {
                let mut combined = Vec::with_capacity(events.len() + 1);
                combined.push(MjaiEvent::ReachAccepted { actor });
                combined.extend(events);
                combined
            }
            None => events,
        }
    }

    /// `ActionDealTile` → mjai `tsumo`. Server omits the `tile` field for
    /// other players' draws (we don't see what they got), so an empty /
    /// missing tile becomes `"?"`. Our own draws carry the real tile.
    ///
    /// When this deal is the rinshan after a kan, the new dora marker
    /// arrives in `data.doras`. Timing depends on the kan type set by the
    /// preceding meld action (see `DoraTiming`).
    fn build_tsumo(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let self_seat = self.seat.context("seat unresolved at ActionDealTile")?;
        let actor = data.get("seat").and_then(JsonValue::as_u64).unwrap_or(0) as Actor;
        let tile_raw = data.get("tile").and_then(JsonValue::as_str).unwrap_or("");
        let pai = if actor == self_seat && !tile_raw.is_empty() {
            ms_to_mjai(tile_raw)?.to_string()
        } else {
            UNKNOWN_TILE.into()
        };

        let new_marker = self.consume_new_dora(data)?;
        let timing = self.dora_timing.take();
        let mut events = Vec::with_capacity(2);

        match (new_marker, timing) {
            (Some(marker), Some(DoraTiming::PendingBeforeRinshan)) => {
                // Ankan: 即乗り — flip dora, then rinshan tsumo.
                events.push(MjaiEvent::Dora {
                    dora_marker: marker,
                });
                events.push(MjaiEvent::Tsumo { actor, pai });
            }
            (Some(marker), Some(DoraTiming::PendingAfterRinshan)) => {
                // Kakan/daiminkan: 後乗り — rinshan tsumo first, dora
                // deferred until the next dahai.
                events.push(MjaiEvent::Tsumo { actor, pai });
                self.deferred_dora = Some(marker);
            }
            (Some(marker), None) => {
                // Unexpected dora bump outside a kan flow. Fall back to
                // emitting it before tsumo (Akagi-style) and warn so the
                // protocol drift is visible in logs.
                warn!(
                    target: "akagi::bridge::majsoul",
                    "new dora marker {marker} without preceding kan; emitting before tsumo"
                );
                events.push(MjaiEvent::Dora {
                    dora_marker: marker,
                });
                events.push(MjaiEvent::Tsumo { actor, pai });
            }
            (None, Some(DoraTiming::PendingBeforeRinshan))
            | (None, Some(DoraTiming::PendingAfterRinshan)) => {
                // Kan declared but rinshan deal carried no new dora — the
                // wire format always includes it for kans, so this is
                // unusual. Emit tsumo and move on.
                warn!(
                    target: "akagi::bridge::majsoul",
                    "rinshan deal after kan missing new dora marker"
                );
                events.push(MjaiEvent::Tsumo { actor, pai });
            }
            (None, None) => {
                events.push(MjaiEvent::Tsumo { actor, pai });
            }
        }
        Ok(events)
    }

    /// `ActionDiscardTile` → mjai `dahai`. `moqie` (default false) maps
    /// directly to `tsumogiri`. `seat` defaults to 0 — Majsoul omits it
    /// for the dealer's first discard.
    ///
    /// If a dora marker is queued for 後乗り (open-kan flow), it is
    /// flushed *before* the dahai event.
    ///
    /// Riichi: when `is_liqi` (or `is_wliqi`, double riichi) is set, a
    /// `reach` event precedes the `dahai` and `pending_reach_accepted` is
    /// queued for the *next* action — the mjai spec puts `reach_accepted`
    /// only after the declaration tile passes through (i.e. before the
    /// next tsumo / chi / pon / daiminkan), and a ron on that tile voids
    /// the riichi.
    fn build_dahai(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let actor = data.get("seat").and_then(JsonValue::as_u64).unwrap_or(0) as Actor;
        let tile_raw = data
            .get("tile")
            .and_then(JsonValue::as_str)
            .context("ActionDiscardTile missing tile")?;
        if tile_raw.is_empty() {
            bail!("ActionDiscardTile.tile is empty");
        }
        let pai = ms_to_mjai(tile_raw)?.to_string();
        let tsumogiri = data
            .get("moqie")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let is_riichi = data
            .get("is_liqi")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
            || data
                .get("is_wliqi")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);

        let mut events = Vec::with_capacity(3);
        if let Some(marker) = self.deferred_dora.take() {
            events.push(MjaiEvent::Dora {
                dora_marker: marker,
            });
        }
        if is_riichi {
            events.push(MjaiEvent::Reach { actor, pai: None });
        }
        events.push(MjaiEvent::Dahai {
            actor,
            pai,
            tsumogiri,
        });
        if is_riichi {
            self.pending_reach_accepted = Some(actor);
        }
        self.last_revealed_tile_actor = Some(actor);
        Ok(events)
    }

    /// Compare `data.doras` against `self.doras`. If a new marker has
    /// appeared (length grew), map the last entry to mjai, push it onto
    /// `self.doras`, and return it. Returns `None` when nothing is new.
    fn consume_new_dora(&mut self, data: &JsonValue) -> Result<Option<String>> {
        let arr = match data.get("doras").and_then(JsonValue::as_array) {
            Some(a) => a,
            None => return Ok(None),
        };
        if arr.len() <= self.doras.len() {
            return Ok(None);
        }
        let last = arr
            .last()
            .and_then(JsonValue::as_str)
            .context("doras[last] not a string")?;
        let mjai = ms_to_mjai(last)?.to_string();
        self.doras.push(mjai.clone());
        Ok(Some(mjai))
    }

    /// `ActionChiPengGang` → `chi` / `pon` / `daiminkan`. `froms[i]`
    /// identifies whose discard supplied tile `tiles[i]`; the sole
    /// non-actor seat is the meld's `target`, its tile is `pai`, and the
    /// remaining tiles are `consumed`.
    fn build_chi_peng_gang(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let actor = data
            .get("seat")
            .and_then(JsonValue::as_u64)
            .context("ActionChiPengGang missing seat")? as Actor;
        let kind = data
            .get("type")
            .and_then(JsonValue::as_u64)
            .context("ActionChiPengGang missing type")?;
        let tiles = data
            .get("tiles")
            .and_then(JsonValue::as_array)
            .context("ActionChiPengGang missing tiles")?;
        let froms = data
            .get("froms")
            .and_then(JsonValue::as_array)
            .context("ActionChiPengGang missing froms")?;
        if tiles.len() != froms.len() {
            bail!(
                "ActionChiPengGang tiles/froms length mismatch: {} vs {}",
                tiles.len(),
                froms.len()
            );
        }

        let mut target = actor;
        let mut pai = String::new();
        let mut consumed: Vec<String> = Vec::new();
        for (idx, from) in froms.iter().enumerate() {
            let from_seat =
                from.as_u64()
                    .context("ActionChiPengGang.froms[i] not a uint")? as Actor;
            let tile_raw = tiles[idx]
                .as_str()
                .context("ActionChiPengGang.tiles[i] not a string")?;
            let tile = ms_to_mjai(tile_raw)?.to_string();
            if from_seat == actor {
                consumed.push(tile);
            } else {
                target = from_seat;
                pai = tile;
            }
        }
        if target == actor {
            bail!("ActionChiPengGang: no foreign seat in froms");
        }
        if pai.is_empty() {
            bail!("ActionChiPengGang: target tile not found");
        }

        let event = match kind {
            CHI_PENG_GANG_CHI => {
                if self.num_players == 3 {
                    // Sanma has no chi. Majsoul shouldn't send it for 3p
                    // tables but log loudly and drop if it does, rather than
                    // propagate a malformed mjai stream.
                    warn!(
                        target: "akagi::bridge::majsoul",
                        "received ActionChiPengGang(chi) in 3p flow; dropping (data={data})"
                    );
                    return Ok(Vec::new());
                }
                if consumed.len() != 2 {
                    bail!("chi expects 2 consumed tiles, got {}", consumed.len());
                }
                let consumed: [String; 2] = consumed
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("chi consumed array conversion failed"))?;
                MjaiEvent::Chi {
                    actor,
                    target,
                    pai,
                    consumed,
                }
            }
            CHI_PENG_GANG_PENG => {
                if consumed.len() != 2 {
                    bail!("pon expects 2 consumed tiles, got {}", consumed.len());
                }
                let consumed: [String; 2] = consumed
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("pon consumed array conversion failed"))?;
                MjaiEvent::Pon {
                    actor,
                    target,
                    pai,
                    consumed,
                }
            }
            CHI_PENG_GANG_GANG => {
                if consumed.len() != 3 {
                    bail!("daiminkan expects 3 consumed tiles, got {}", consumed.len());
                }
                let consumed: [String; 3] = consumed
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("daiminkan consumed array conversion failed"))?;
                // 後乗り: dora is flipped after the rinshan tsumo, before
                // the next dahai.
                self.dora_timing = Some(DoraTiming::PendingAfterRinshan);
                MjaiEvent::Daiminkan {
                    actor,
                    target,
                    pai,
                    consumed,
                }
            }
            other => bail!("unknown ActionChiPengGang.type: {other}"),
        };
        Ok(vec![event])
    }

    /// `ActionAnGangAddGang` → `ankan` (type 3) or `kakan` (type 2). The
    /// `tiles` field is a single tile string (not a list) — for ankan it
    /// names the four-of-a-kind, for kakan it names the new tile being
    /// added on top of an existing pon.
    fn build_an_gang_add_gang(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let actor = data
            .get("seat")
            .and_then(JsonValue::as_u64)
            .context("ActionAnGangAddGang missing seat")? as Actor;
        let kind = data
            .get("type")
            .and_then(JsonValue::as_u64)
            .context("ActionAnGangAddGang missing type")?;
        let tile_raw = data
            .get("tiles")
            .and_then(JsonValue::as_str)
            .context("ActionAnGangAddGang.tiles not a string")?;
        let pai = ms_to_mjai(tile_raw)?.to_string();

        let event = match kind {
            AN_GANG_ADD_GANG_AN => {
                let consumed = ankan_consumed(&pai);
                // 即乗り: dora flips before the rinshan tsumo.
                self.dora_timing = Some(DoraTiming::PendingBeforeRinshan);
                MjaiEvent::Ankan { actor, consumed }
            }
            AN_GANG_ADD_GANG_ADD => {
                let consumed = kakan_consumed(&pai);
                // 後乗り: dora flips after rinshan tsumo, before dahai.
                self.dora_timing = Some(DoraTiming::PendingAfterRinshan);
                MjaiEvent::Kakan {
                    actor,
                    pai,
                    consumed,
                }
            }
            other => bail!("unknown ActionAnGangAddGang.type: {other}"),
        };
        // Both kan types reveal a tile that another seat may rob:
        //   - kakan → 搶槓 (chankan).
        //   - ankan → 国士無双搶暗槓 (only kokushi can rob; the server
        //     would only emit `ActionHule` in that valid case anyway, so
        //     unconditional tracking is safe).
        // If a ron follows, `build_hule` uses this seat as the `target`.
        self.last_revealed_tile_actor = Some(actor);
        Ok(vec![event])
    }

    /// `ActionNoTile` (荒牌流局, exhaustive draw) → `[ryukyoku{deltas},
    /// end_kyoku]`. The mjai spec text only mentions ryukyoku for
    /// 九種九牌, but libriichi/Mortal use the same event for noten
    /// payments and carry the deltas in the optional `deltas` field; we
    /// follow that convention so downstream stat code can attribute the
    /// payment correctly.
    ///
    /// `data.scores[]` is an array of `NoTileScoreInfo` — each entry
    /// carries its own `delta_scores` array sized to `num_players`. Multiple
    /// entries can occur (rare — e.g. tenpai redistribution + nagashi mangan
    /// in the same frame); the bridge sums them per seat.
    fn build_no_tile(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let deltas = sum_delta_scores(data, self.num_players)?;
        Ok(vec![MjaiEvent::Ryukyoku { deltas }, MjaiEvent::EndKyoku])
    }

    /// `ActionHule` (胡牌) → one `hora` per entry in `data.hules[]`,
    /// followed by `end_kyoku`.
    ///
    /// Per mjai semantics:
    /// - `actor = hule.seat`.
    /// - `target = actor` when `hule.zimo` (self-tsumo); otherwise the
    ///   discarder, which mjai requires us to track ourselves —
    ///   `HuleInfo` doesn't carry it. We use `self.last_revealed_tile_actor` set by
    ///   the most recent `build_dahai`.
    /// - `deltas = data.delta_scores` (top-level, the round's net point
    ///   change). For multi-ron we attach the same total to each `hora`
    ///   event; the consumer can dedupe if it cares (Mortal-style stats
    ///   only count points once anyway).
    /// - `ura_markers = hule.li_doras` when `hule.liqi` is true (riichi
    ///   reveals the ura), else `None`. An empty `li_doras` array under
    ///   `liqi=true` still becomes `Some(vec![])` so consumers can tell
    ///   "had a riichi but no ura markers" from "no riichi".
    fn build_hule(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let hules = data
            .get("hules")
            .and_then(JsonValue::as_array)
            .context("ActionHule missing hules")?;
        if hules.is_empty() {
            bail!("ActionHule with empty hules");
        }
        let deltas = data
            .get("delta_scores")
            .and_then(JsonValue::as_array)
            .map(|a| parse_deltas(a))
            .transpose()?;

        let mut events = Vec::with_capacity(hules.len() + 1);
        for hule in hules {
            let actor = hule
                .get("seat")
                .and_then(JsonValue::as_u64)
                .context("HuleInfo missing seat")? as Actor;
            let zimo = hule
                .get("zimo")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            let target = if zimo {
                actor
            } else {
                self.last_revealed_tile_actor
                    .context("ron win without preceding tile-revealing action")?
            };
            let liqi = hule
                .get("liqi")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false);
            let ura_markers = if liqi {
                let arr = hule
                    .get("li_doras")
                    .and_then(JsonValue::as_array)
                    .map(|a| -> Result<Vec<String>> {
                        a.iter()
                            .map(|v| {
                                v.as_str()
                                    .context("li_doras entry not a string")
                                    .and_then(|s| ms_to_mjai(s).map(String::from))
                            })
                            .collect()
                    })
                    .transpose()?
                    .unwrap_or_default();
                Some(arr)
            } else {
                None
            };
            events.push(MjaiEvent::Hora {
                actor,
                target,
                deltas: deltas.clone(),
                ura_markers,
            });
        }
        events.push(MjaiEvent::EndKyoku);
        Ok(events)
    }

    /// `ActionLiuJu` (途中流局, abortive draw) → `[ryukyoku, end_kyoku]`
    /// without deltas. Covers 九種九牌 / 四風連打 / 四家立直 / 四開槓 /
    /// 三家和了 (4p) and 九種九牌 / 四開槓 (3p — sufuurenta and suucha
    /// riichi need 4 players and don't apply to sanma).
    ///
    /// The proto carries `ActionLiuJu.type: uint32` for the abortive cause
    /// but no enum exists in `liqi.proto`. Both Akagi-Python and AkagiNG
    /// ignore the field entirely and emit a generic ryukyoku. We follow
    /// suit until real captures of each type value arrive — at that point
    /// we can surface a `reason` field on `MjaiEvent::Ryukyoku`.
    fn build_liu_ju(&mut self, _data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        Ok(vec![
            MjaiEvent::Ryukyoku { deltas: None },
            MjaiEvent::EndKyoku,
        ])
    }

    /// Convert one Majsoul `ActionNewRound` payload into `start_kyoku`
    /// (followed by `tsumo` for the dealer's first draw — known tile when
    /// we're the dealer, `"?"` when we aren't).
    ///
    /// The seat must already be resolved via the prior `authGame` exchange;
    /// otherwise we have no way to know which tehai slot to fill in.
    ///
    /// Resets per-kyoku dora-tracking state so a stale `deferred_dora` from
    /// a previous kyoku can't bleed into this one.
    fn build_start_kyoku(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let seat = self.seat.context("seat unresolved at ActionNewRound")?;

        // Missing protobuf uint fields default to 0 in the JSON payload.
        let chang = data.get("chang").and_then(JsonValue::as_u64).unwrap_or(0) as usize;
        let ju = data.get("ju").and_then(JsonValue::as_u64).unwrap_or(0) as u8;
        let ben = data.get("ben").and_then(JsonValue::as_u64).unwrap_or(0) as u8;
        let liqibang = data
            .get("liqibang")
            .and_then(JsonValue::as_u64)
            .unwrap_or(0) as u8;

        let bakaze: String = match chang {
            0 => "E",
            1 => "S",
            2 => "W",
            3 => "N",
            other => bail!("invalid chang value: {other}"),
        }
        .into();

        let dora_marker = data
            .get("doras")
            .and_then(JsonValue::as_array)
            .and_then(|a| a.first())
            .and_then(JsonValue::as_str)
            .context("ActionNewRound missing doras[0]")?;
        let dora_marker = ms_to_mjai(dora_marker)?.to_string();

        let scores = parse_scores(data)?;

        let tiles_raw = data
            .get("tiles")
            .and_then(JsonValue::as_array)
            .context("ActionNewRound missing tiles")?;
        let my_tiles: Vec<String> = tiles_raw
            .iter()
            .map(|v| {
                v.as_str()
                    .context("non-string tile in ActionNewRound.tiles")
                    .and_then(|s| ms_to_mjai(s).map(|t| t.to_string()))
            })
            .collect::<Result<Vec<_>>>()?;

        let oya = ju;
        let n = self.num_players as usize;
        let mut tehais: Vec<Vec<String>> = (0..n)
            .map(|_| vec![UNKNOWN_TILE.to_string(); TEHAI_SIZE])
            .collect();

        let tsumo_event = match my_tiles.len() {
            TEHAI_SIZE => {
                // Non-dealer: our 13 tiles fill our row; dealer's first draw
                // is unknown to us.
                let mut my_row = my_tiles.clone();
                my_row.sort_by(|a, b| compare_pai(a, b));
                fill_seat_row(&mut tehais, seat, my_row)?;
                if oya == seat {
                    bail!("dealer must receive 14 tiles, got 13");
                }
                MjaiEvent::Tsumo {
                    actor: oya,
                    pai: UNKNOWN_TILE.into(),
                }
            }
            TSUMO_TEHAI_SIZE => {
                // Dealer: first 13 tiles → our tehai (sorted), 14th tile is
                // the dealer's opening tsumo.
                if oya != seat {
                    bail!("non-dealer must receive 13 tiles, got 14");
                }
                let mut my_row: Vec<String> = my_tiles[..TEHAI_SIZE].to_vec();
                my_row.sort_by(|a, b| compare_pai(a, b));
                let tsumo_pai = my_tiles[TEHAI_SIZE].clone();
                fill_seat_row(&mut tehais, seat, my_row)?;
                MjaiEvent::Tsumo {
                    actor: seat,
                    pai: tsumo_pai,
                }
            }
            n => bail!("unexpected tile count {n} in ActionNewRound (expected 13 or 14)"),
        };

        info!(
            target: "akagi::bridge::majsoul",
            "start_kyoku bakaze={bakaze} kyoku={kyoku} oya={oya} honba={ben} kyotaku={liqibang}",
            kyoku = oya + 1,
        );

        // Fresh kyoku — reset dora + riichi + discard bookkeeping.
        self.doras = vec![dora_marker.clone()];
        self.dora_timing = None;
        self.deferred_dora = None;
        self.last_revealed_tile_actor = None;
        self.pending_reach_accepted = None;

        Ok(vec![
            MjaiEvent::StartKyoku {
                bakaze,
                dora_marker,
                kyoku: oya + 1,
                honba: ben,
                kyotaku: liqibang,
                oya,
                scores,
                tehais,
                num_players: self.num_players,
            },
            tsumo_event,
        ])
    }

    /// `ActionBaBei` (3p 北抜き / nukidora) → mjai `kita`.
    ///
    /// Captured Majsoul payload (3p AI room, 2026-04-29):
    ///
    /// ```json
    /// {"data":{"doras":[],"moqie":false,"seat":0,"tile_state":0,
    ///          "tingpais":[],"zhenting":false},
    ///  "name":"ActionBaBei","step":2}
    /// ```
    ///
    /// Notes:
    /// - No `tile` field — kita is always the North tile, so we hardcode
    ///   `pai = "N"` per `reference/reference_mjai_3p.md`.
    /// - `doras: []` — kita does NOT flip a new dora indicator (Tenhou
    ///   rule, confirmed live). Skip dora bookkeeping entirely.
    /// - `last_revealed_tile_actor` is updated so a follow-up ron-on-kita
    ///   (chankan-style) gets the correct `target` in `build_hule`.
    ///
    /// Outside 3p tables Majsoul should never emit this; if it does, log
    /// a warning but still emit so the data isn't silently lost.
    fn build_kita(&mut self, data: &JsonValue) -> Result<Vec<MjaiEvent>> {
        let actor = data
            .get("seat")
            .and_then(JsonValue::as_u64)
            .context("ActionBaBei missing seat")? as Actor;
        if self.num_players != 3 {
            warn!(
                target: "akagi::bridge::majsoul",
                "ActionBaBei received in {}p flow (expected 3p): {data}",
                self.num_players
            );
        }
        self.last_revealed_tile_actor = Some(actor);
        Ok(vec![MjaiEvent::Kita {
            actor,
            pai: Some("N".to_string()),
        }])
    }

    fn handle_auth_game_response(&mut self, payload: &JsonValue) -> Vec<MjaiEvent> {
        let Some(account_id) = self.account_id else {
            warn!(
                target: "akagi::bridge::majsoul",
                "authGame response received without prior request — cannot resolve seat"
            );
            return Vec::new();
        };
        let Some(seat_list) = payload.get("seat_list").and_then(JsonValue::as_array) else {
            warn!(
                target: "akagi::bridge::majsoul",
                "authGame response missing seat_list: {payload}"
            );
            return Vec::new();
        };
        let position = seat_list
            .iter()
            .position(|v| v.as_u64() == Some(account_id));
        let Some(seat) = position else {
            warn!(
                target: "akagi::bridge::majsoul",
                "account_id {account_id} not found in seat_list {seat_list:?}"
            );
            return Vec::new();
        };
        let seat = seat as Actor;
        self.seat = Some(seat);
        // 3p tables produce length-3 seat_list; 4p length-4. Anything else
        // would be a protocol surprise — clamp into [3, 4] but log loudly.
        let detected = seat_list.len() as u8;
        if detected != 3 && detected != 4 {
            warn!(
                target: "akagi::bridge::majsoul",
                "unexpected seat_list length {detected}; defaulting num_players=4"
            );
            self.num_players = 4;
        } else {
            self.num_players = detected;
        }
        let mode_id = payload
            .pointer("/game_config/meta/mode_id")
            .and_then(JsonValue::as_u64);
        let names = names_from_payload(payload, seat_list);
        info!(
            target: "akagi::bridge::majsoul",
            "seat resolved: account_id={account_id} seat={seat} num_players={np} mode_id={mode_id:?} names={names:?}",
            np = self.num_players,
        );
        vec![MjaiEvent::StartGame {
            names,
            kyoku_first: None,
            aka_flag: None,
            id: Some(seat),
            num_players: self.num_players,
        }]
    }
}

/// Parse `data.scores` into a `Vec<i32>` of native length (3 for sanma, 4 for
/// yonma). No padding — the wire format reflects the table's actual seat count.
fn parse_scores(data: &JsonValue) -> Result<Vec<i32>> {
    let arr = data
        .get("scores")
        .and_then(JsonValue::as_array)
        .context("ActionNewRound missing scores")?;
    arr.iter()
        .map(|v| {
            v.as_i64()
                .context("non-integer score")?
                .try_into()
                .context("score out of i32 range")
        })
        .collect()
}

/// Place a 13-tile row into `tehais[seat]`. Errors if `seat` is out of range
/// for the supplied tehais Vec or the row isn't exactly 13 tiles long.
fn fill_seat_row(tehais: &mut [Vec<String>], seat: Actor, row: Vec<String>) -> Result<()> {
    if (seat as usize) >= tehais.len() {
        bail!("seat {seat} out of range");
    }
    if row.len() != TEHAI_SIZE {
        bail!(
            "expected {TEHAI_SIZE} tiles for seat row, got {}",
            row.len()
        );
    }
    tehais[seat as usize] = row;
    Ok(())
}

/// Parse a JSON array of integers into `Vec<i32>` of native length.
fn parse_deltas(arr: &[JsonValue]) -> Result<Vec<i32>> {
    arr.iter()
        .map(|v| {
            v.as_i64()
                .context("delta_scores entry not an integer")?
                .try_into()
                .context("delta_scores entry out of i32 range")
        })
        .collect()
}

/// Sum every `delta_scores` array under `data.scores[]` into a single
/// `Vec<i32>` sized to `num_players`. Returns `None` when no entries are
/// present (no point change — kept distinguishable from an explicit all-zero
/// delta). Each entry's length is expected to match `num_players`; entries
/// shorter than that are zero-extended (defensive — Majsoul should always
/// emit the right length).
fn sum_delta_scores(data: &JsonValue, num_players: u8) -> Result<Option<Vec<i32>>> {
    let arr = match data.get("scores").and_then(JsonValue::as_array) {
        Some(a) if !a.is_empty() => a,
        _ => return Ok(None),
    };
    let n = num_players as usize;
    let mut total = vec![0i32; n];
    for entry in arr {
        let deltas = match entry.get("delta_scores").and_then(JsonValue::as_array) {
            Some(d) => d,
            None => continue,
        };
        for (i, v) in deltas.iter().take(n).enumerate() {
            let val: i32 = v
                .as_i64()
                .context("non-integer delta_scores entry")?
                .try_into()
                .context("delta_scores out of i32 range")?;
            total[i] = total[i].saturating_add(val);
        }
    }
    Ok(Some(total))
}

/// Mjai uses one red-five token (`5mr`/`5pr`/`5sr`) and three normals when a
/// red is in the kan. `pai` may itself be either form. Returns 4 tiles with
/// at most one red five, placed at index 0 when present.
fn ankan_consumed(pai: &str) -> [String; 4] {
    let normal = pai.trim_end_matches('r').to_string();
    let mut out = std::array::from_fn(|_| normal.clone());
    if pai_has_red_form(&normal) {
        out[0] = format!("{normal}r");
    }
    out
}

/// Same shape as `ankan_consumed`, but for the 3 tiles already in the
/// existing pon. The kan'd tile (`pai`) is the new addition and reported
/// separately on the `kakan` event.
fn kakan_consumed(pai: &str) -> [String; 3] {
    let normal = pai.trim_end_matches('r').to_string();
    let mut out = std::array::from_fn(|_| normal.clone());
    if pai_has_red_form(&normal) && !pai.ends_with('r') {
        out[0] = format!("{normal}r");
    }
    out
}

/// True if `pai` is a numbered 5 in m/p/s suits (the only tiles with a red
/// counterpart). Honors and non-fives never have a red form.
fn pai_has_red_form(pai: &str) -> bool {
    matches!(pai, "5m" | "5p" | "5s")
}

/// Resolve seat → display name via `payload.players[]` (account_id → nickname).
/// Robot seats are absent from `players` (they live under `robots[]` without
/// a nickname), so they get an empty string. Returned `Vec` length matches
/// `seat_list.len()` (3 for sanma, 4 for yonma).
fn names_from_payload(payload: &JsonValue, seat_list: &[JsonValue]) -> Vec<String> {
    let mut nick: HashMap<u64, String> = HashMap::new();
    if let Some(players) = payload.get("players").and_then(JsonValue::as_array) {
        for p in players {
            if let (Some(id), Some(name)) = (
                p.get("account_id").and_then(JsonValue::as_u64),
                p.get("nickname").and_then(JsonValue::as_str),
            ) {
                nick.insert(id, name.to_string());
            }
        }
    }
    seat_list
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|id| nick.get(&id).cloned())
                .unwrap_or_default()
        })
        .collect()
}

impl Default for MajsoulBridge {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl Bridge for MajsoulBridge {
    /// Parse a raw Majsoul WS binary frame, log the decoded message to the
    /// flow log (if any), and emit any resulting mjai events.
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult {
        use crate::schema::ParsedFrame;
        match self.parser.parse(content) {
            Ok(msg) => {
                let kind = match msg.msg_type {
                    MessageType::Notify => "NOTIFY",
                    MessageType::Request => "REQUEST",
                    MessageType::Response => "RESPONSE",
                };
                let id_str = msg
                    .msg_id
                    .map(|i| format!("#{i}"))
                    .unwrap_or_else(|| "-".into());
                info!(
                    target: "akagi::bridge::majsoul",
                    "{} {kind} {id_str} {} {}",
                    direction.as_str(),
                    msg.method_name,
                    msg.payload
                );
                if let Some(log) = &self.flow_log {
                    let line = json!({
                        "ts": Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        "dir": direction.as_str(),
                        "type": kind,
                        "msg_id": msg.msg_id,
                        "method": msg.method_name.as_ref(),
                        "payload": msg.payload,
                    });
                    log.writeln(&line.to_string());
                }
                // Build the inspector-facing parsed view BEFORE dispatch
                // consumes the payload — `dispatch` only borrows `&msg`,
                // so we can clone the bits we want and still let it run.
                let parsed = Some(ParsedFrame {
                    method: msg.method_name.to_string(),
                    args: json!({
                        "kind": kind,
                        "msg_id": msg.msg_id,
                        "payload": msg.payload.clone(),
                    }),
                });
                let events = self.dispatch(&msg);
                // Rotate before writing so the StartGame event itself lands
                // in the freshly-opened file, not the previous game's file.
                if events
                    .iter()
                    .any(|e| matches!(e, MjaiEvent::StartGame { .. }))
                {
                    self.rotate_mjai_log();
                }
                self.write_mjai(&events);
                ParseResult { events, parsed }
            }
            Err(e) => {
                warn!(
                    target: "akagi::bridge::majsoul",
                    "{} liqi parse failed (len={}): {e:#}",
                    direction.as_str(),
                    content.len()
                );
                if let Some(log) = &self.flow_log {
                    let line = json!({
                        "ts": Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                        "dir": direction.as_str(),
                        "type": "PARSE_ERROR",
                        "len": content.len(),
                        "error": format!("{e:#}"),
                    });
                    log.writeln(&line.to_string());
                }
                ParseResult::empty()
            }
        }
    }

    fn build(&mut self, _command: &MjaiEvent) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parser::ParsedMessage;
    use serde_json::json;

    fn req(method: &str, payload: JsonValue) -> ParsedMessage {
        ParsedMessage {
            msg_type: MessageType::Request,
            msg_id: Some(1),
            method_name: Arc::from(method),
            payload,
        }
    }

    fn resp(method: &str, payload: JsonValue) -> ParsedMessage {
        ParsedMessage {
            msg_type: MessageType::Response,
            msg_id: Some(1),
            method_name: Arc::from(method),
            payload,
        }
    }

    #[test]
    fn auth_game_resolves_seat_and_names() {
        let mut bridge = MajsoulBridge::new(None, None);

        // Req captures account_id, no events yet.
        let events = bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 12345 })));
        assert!(events.is_empty());
        assert_eq!(bridge.account_id, Some(12345));
        assert_eq!(bridge.seat, None);

        // Res shape mirrors a real authGame response against AI: one human in
        // `players`, three robots referenced only by id in `seat_list`.
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({
                "players": [{ "account_id": 12345, "nickname": "player_a" }],
                "seat_list": [1, 3, 12345u64, 2],
            }),
        ));
        assert_eq!(bridge.seat, Some(2));
        assert_eq!(events.len(), 1);
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                names,
                num_players,
                ..
            } => {
                assert_eq!(*id, Some(2));
                assert_eq!(*num_players, 4);
                assert_eq!(
                    names,
                    &vec![
                        "".to_string(),
                        "".to_string(),
                        "player_a".to_string(),
                        "".to_string()
                    ]
                );
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
        assert_eq!(bridge.num_players, 4);
    }

    #[test]
    fn names_filled_for_four_human_players() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 100 })));
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({
                "players": [
                    { "account_id": 100, "nickname": "alice" },
                    { "account_id": 200, "nickname": "bob" },
                    { "account_id": 300, "nickname": "carol" },
                    { "account_id": 400, "nickname": "dave" },
                ],
                "seat_list": [200u64, 100u64, 400u64, 300u64],
            }),
        ));
        match &events[0] {
            MjaiEvent::StartGame { id, names, .. } => {
                assert_eq!(*id, Some(1));
                assert_eq!(
                    names,
                    &vec![
                        "bob".to_string(),
                        "alice".to_string(),
                        "dave".to_string(),
                        "carol".to_string(),
                    ]
                );
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
    }

    /// Real captured 3p authGame response (single human + 2 robots, AI casual
    /// match `mode_id=0`). Verifies (a) num_players resolves to 3 from
    /// `seat_list.len()`, (b) emitted names array is length 3 with the human
    /// nickname at the right seat, (c) seat resolves to the human's index in
    /// the seat_list. `mode_id=0` is normal for AI rooms (only ranked sanma
    /// rooms use 21/22/26) — bridge logs it but doesn't gate on it.
    #[test]
    fn three_player_real_auth_game_payload() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 12345 })));
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({
                "game_config": {
                    "category": 1,
                    "meta": { "contest_uid": 0, "mode_id": 0, "room_id": 33123 },
                    "mode": { "ai": true, "mode": 12 }
                },
                "players": [{ "account_id": 12345, "nickname": "player_a" }],
                "robots": [
                    { "account_id": 1, "nickname": "" },
                    { "account_id": 2, "nickname": "" },
                ],
                "seat_list": [2u64, 12345u64, 1u64],
            }),
        ));
        assert_eq!(bridge.seat, Some(1));
        assert_eq!(bridge.num_players, 3);
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                names,
                num_players,
                ..
            } => {
                assert_eq!(*id, Some(1));
                assert_eq!(*num_players, 3);
                assert_eq!(
                    names,
                    &vec!["".to_string(), "player_a".to_string(), "".to_string()]
                );
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
    }

    /// 3-player table: `seat_list` is length 3, names array stays length 3
    /// (no padding to 4), and `num_players` is stamped on the StartGame event.
    #[test]
    fn three_player_seat_list_emits_length_three_names_and_num_players() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 50 })));
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({
                "players": [{ "account_id": 50, "nickname": "me" }],
                "seat_list": [1, 50u64, 2],
            }),
        ));
        match &events[0] {
            MjaiEvent::StartGame {
                id,
                names,
                num_players,
                ..
            } => {
                assert_eq!(*id, Some(1));
                assert_eq!(*num_players, 3);
                assert_eq!(
                    names,
                    &vec!["".to_string(), "me".to_string(), "".to_string()]
                );
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
        assert_eq!(bridge.num_players, 3);
    }

    #[test]
    fn auth_game_response_without_request_warns_and_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({ "seat_list": [1, 2, 3, 4] }),
        ));
        assert!(events.is_empty());
        assert_eq!(bridge.seat, None);
    }

    #[test]
    fn auth_game_account_id_not_in_seat_list_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 999 })));
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({ "seat_list": [1, 2, 3, 4] }),
        ));
        assert!(events.is_empty());
        assert_eq!(bridge.seat, None);
    }

    #[test]
    fn unrelated_methods_pass_through() {
        let mut bridge = MajsoulBridge::new(None, None);
        let events = bridge.dispatch(&req(
            ".lq.Lobby.heatbeat",
            json!({ "no_operation_counter": 0 }),
        ));
        assert!(events.is_empty());
    }

    /// Each `start_game` event should land in a freshly-opened
    /// `majsoul_<ts>.mjai.jsonl` file under `<session>/majsoul/`. The
    /// previous game's file must NOT receive subsequent events.
    #[test]
    fn start_game_rotates_mjai_log_and_appends_event() {
        use crate::logger::FlowLogger;
        use std::fs;
        use std::time::Duration;

        let tmp = tempfile::tempdir().unwrap();
        let mut bridge = MajsoulBridge::new(None, None);

        // Manually install a "first game" log to simulate what
        // rotate_mjai_log would have done — without needing a full Session.
        let first = Arc::new(
            FlowLogger::new(tmp.path(), "majsoul", "majsoul_first.mjai.jsonl", "first").unwrap(),
        );
        bridge.mjai_log = Some(first);
        bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 1 })));
        let events = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({
                "players": [{ "account_id": 1, "nickname": "p1" }],
                "seat_list": [1u64, 2, 3, 4],
            }),
        ));
        assert_eq!(events.len(), 1);
        bridge.write_mjai(&events);

        // Now simulate a second game on the same flow: rotate, then write.
        // Sleep > 1ms so the timestamp differs (filename includes millis).
        std::thread::sleep(Duration::from_millis(2));
        let second = Arc::new(
            FlowLogger::new(tmp.path(), "majsoul", "majsoul_second.mjai.jsonl", "second").unwrap(),
        );
        bridge.mjai_log = Some(second);
        bridge.dispatch(&req(METHOD_AUTH_GAME, json!({ "account_id": 2 })));
        let events2 = bridge.dispatch(&resp(
            METHOD_AUTH_GAME,
            json!({
                "players": [{ "account_id": 2, "nickname": "p2" }],
                "seat_list": [5u64, 2u64, 6, 7],
            }),
        ));
        bridge.write_mjai(&events2);

        let first_content =
            fs::read_to_string(tmp.path().join("majsoul/majsoul_first.mjai.jsonl")).unwrap();
        let second_content =
            fs::read_to_string(tmp.path().join("majsoul/majsoul_second.mjai.jsonl")).unwrap();

        // First file: exactly one start_game with id=0 (account 1 at index 0).
        let first_lines: Vec<&str> = first_content.lines().collect();
        assert_eq!(first_lines.len(), 1, "first file should have one line");
        assert!(first_lines[0].contains(r#""type":"start_game""#));
        assert!(first_lines[0].contains(r#""id":0"#));
        assert!(first_lines[0].contains(r#""p1""#));

        // Second file: exactly one start_game with id=1 (account 2 at index 1).
        let second_lines: Vec<&str> = second_content.lines().collect();
        assert_eq!(second_lines.len(), 1, "second file should have one line");
        assert!(second_lines[0].contains(r#""type":"start_game""#));
        assert!(second_lines[0].contains(r#""id":1"#));
        assert!(second_lines[0].contains(r#""p2""#));

        // First file must not have leaked the second game's data.
        assert!(!first_content.contains(r#""p2""#));
    }

    /// `rotate_mjai_log` is a no-op when no session is wired (parse-only mode).
    #[test]
    fn rotate_without_session_is_noop() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.rotate_mjai_log();
        assert!(bridge.mjai_log.is_none());
    }

    /// Helper: build an ActionPrototype Notify ParsedMessage carrying an
    /// already-decoded ActionNewRound payload (mirrors what the parser
    /// produces after `maybe_decode_action`).
    fn new_round_msg(action_data: JsonValue) -> ParsedMessage {
        ParsedMessage {
            msg_type: MessageType::Notify,
            msg_id: None,
            method_name: Arc::from(METHOD_ACTION_PROTOTYPE),
            payload: json!({
                "step": 1,
                "name": ACTION_NEW_ROUND,
                "data": action_data,
            }),
        }
    }

    /// Sample ActionNewRound payload from the user-supplied real Majsoul
    /// frame: human player (us) at seat 2, ju=0 → dealer is seat 0 (a bot).
    /// We're not the dealer, so we get exactly 13 tiles.
    #[test]
    fn action_new_round_non_dealer_emits_start_kyoku_then_unknown_tsumo() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = new_round_msg(json!({
            "doras": ["4s"],
            "left_tile_count": 69,
            "scores": [25000, 25000, 25000, 25000],
            "tiles": ["7p","3p","3s","1s","2z","2m","8m","6p","7m","5m","0s","6s","7z"],
        }));

        let events = bridge.dispatch(&msg);
        assert_eq!(events.len(), 2);

        match &events[0] {
            MjaiEvent::StartKyoku {
                bakaze,
                dora_marker,
                kyoku,
                honba,
                kyotaku,
                oya,
                scores,
                tehais,
                num_players,
            } => {
                assert_eq!(bakaze, "E");
                assert_eq!(dora_marker, "4s");
                assert_eq!(*kyoku, 1);
                assert_eq!(*honba, 0);
                assert_eq!(*kyotaku, 0);
                assert_eq!(*oya, 0);
                assert_eq!(*num_players, 4);
                assert_eq!(scores, &vec![25000, 25000, 25000, 25000]);
                // Other seats stay unknown.
                for s in [0, 1, 3] {
                    assert!(tehais[s].iter().all(|t| t == "?"));
                }
                // Our row: 13 tiles, sorted, mapped (0s → 5sr, 2z → S, 7z → C).
                assert_eq!(
                    tehais[2],
                    vec![
                        "2m", "5m", "7m", "8m", "3p", "6p", "7p", "1s", "3s", "5sr", "6s", "S",
                        "C",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>()
                );
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 0); // dealer
                assert_eq!(pai, "?");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Same shape but with us as dealer (seat 0, ju=0). We get 14 tiles;
    /// the 14th is our opening tsumo (kept raw, not sorted in).
    #[test]
    fn action_new_round_dealer_emits_start_kyoku_then_self_tsumo() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let msg = new_round_msg(json!({
            "doras": ["1m"],
            "scores": [25000, 25000, 25000, 25000],
            "tiles": [
                "1m","2m","3m","4m","5m","6m","7m","8m","9m",
                "1p","2p","3p","4p",
                "0p"
            ],
        }));

        let events = bridge.dispatch(&msg);
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::StartKyoku {
                oya,
                tehais,
                dora_marker,
                ..
            } => {
                assert_eq!(*oya, 0);
                assert_eq!(dora_marker, "1m");
                // First 13 tiles, sorted (already in mjai order here).
                assert_eq!(
                    tehais[0],
                    [
                        "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p",
                        "4p",
                    ]
                    .map(String::from)
                );
                for s in [1, 2, 3] {
                    assert!(tehais[s].iter().all(|t| t == "?"));
                }
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "5pr"); // 0p → red 5p
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// chang/ju/honba/liqibang explicit non-defaults are reflected verbatim.
    #[test]
    fn action_new_round_propagates_chang_ju_honba_liqibang() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(1);
        let msg = new_round_msg(json!({
            "chang": 1,
            "ju": 2,
            "ben": 3,
            "liqibang": 1,
            "doras": ["7m"],
            "scores": [24000, 26000, 25000, 25000],
            "tiles": ["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"],
        }));
        let events = bridge.dispatch(&msg);
        match &events[0] {
            MjaiEvent::StartKyoku {
                bakaze,
                kyoku,
                honba,
                kyotaku,
                oya,
                scores,
                ..
            } => {
                assert_eq!(bakaze, "S");
                assert_eq!(*kyoku, 3); // ju + 1
                assert_eq!(*oya, 2);
                assert_eq!(*honba, 3);
                assert_eq!(*kyotaku, 1);
                assert_eq!(scores, &vec![24000, 26000, 25000, 25000]);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }

    /// Real captured 3p `ActionNewRound` payload (East 1, honba 1, dealer at
    /// seat 0 = user). Confirms the bridge handles a real 3p deal end-to-end:
    /// length-3 scores, length-3 tehais, dealer's 14th tile becomes opening
    /// tsumo. dora_marker `4z` → `N`. `left_tile_count` 54 = 108 − 14 dead
    /// wall − 13×3 dealt − 1 dealer's first draw.
    #[test]
    fn action_new_round_three_player_real_payload() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        let msg = new_round_msg(json!({
            "al": false,
            "ben": 1,
            "chang": 0,
            "doras": ["4z"],
            "ju": 0,
            "left_tile_count": 54,
            "liqibang": 0,
            "scores": [37000, 34000, 34000],
            "tiles": ["7s","9m","6s","8s","1m","6p","9s","1p","5z","9s","8s","2s","4p","6p"],
            "tingpais0": [],
            "tingpais1": [],
        }));
        let events = bridge.dispatch(&msg);
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::StartKyoku {
                bakaze,
                dora_marker,
                kyoku,
                honba,
                kyotaku,
                oya,
                scores,
                tehais,
                num_players,
            } => {
                assert_eq!(bakaze, "E");
                assert_eq!(dora_marker, "N");
                assert_eq!(*kyoku, 1);
                assert_eq!(*honba, 1);
                assert_eq!(*kyotaku, 0);
                assert_eq!(*oya, 0);
                assert_eq!(*num_players, 3);
                assert_eq!(scores, &vec![37000, 34000, 34000]);
                assert_eq!(tehais.len(), 3, "no phantom 4th seat");
                // Dealer's 13 tiles, sorted, mapped (5z → P). Note the
                // captured payload has TWO 6p (index 5 in tehai and index
                // 13 = opening tsumo), so the sorted tehai has one 6p, two
                // 8s and two 9s.
                assert_eq!(
                    tehais[0],
                    vec![
                        "1m", "9m", "1p", "4p", "6p", "2s", "6s", "7s", "8s", "8s", "9s", "9s",
                        "P",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>()
                );
                for s in [1, 2] {
                    assert!(tehais[s].iter().all(|t| t == "?"));
                }
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 0);
                // 14th tile in payload (6p) is the dealer's opening tsumo.
                assert_eq!(pai, "6p");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Synthetic 3p table: native-length scores/tehais (no padding), with the
    /// user as dealer. Slimmer fixture than the real-payload test above.
    #[test]
    fn action_new_round_three_player_emits_native_length() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        let msg = new_round_msg(json!({
            "doras": ["1m"],
            "scores": [35000, 35000, 35000],
            "tiles": [
                "1m","9m",
                "1p","2p","3p","4p","5p","6p","7p","8p","9p",
                "1s","2s","3s"
            ],
        }));
        let events = bridge.dispatch(&msg);
        match &events[0] {
            MjaiEvent::StartKyoku {
                scores,
                tehais,
                num_players,
                ..
            } => {
                assert_eq!(*num_players, 3);
                assert_eq!(scores, &vec![35000, 35000, 35000]);
                assert_eq!(tehais.len(), 3, "no phantom 4th seat");
                for hand in tehais {
                    assert_eq!(hand.len(), 13);
                }
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }

    /// Tile count mismatch with seat role must error out (no events emitted).
    #[test]
    fn action_new_round_seat_role_mismatch_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        // We say we're seat 0 but ju=1 (dealer is seat 1) and we got 14 tiles.
        bridge.seat = Some(0);
        let msg = new_round_msg(json!({
            "ju": 1,
            "doras": ["1m"],
            "scores": [25000, 25000, 25000, 25000],
            "tiles": [
                "1m","2m","3m","4m","5m","6m","7m","8m","9m",
                "1p","2p","3p","4p","5p"
            ],
        }));
        let events = bridge.dispatch(&msg);
        assert!(events.is_empty(), "mismatch should produce no events");
    }

    /// ActionNewRound before authGame (seat unresolved) must skip gracefully.
    #[test]
    fn action_new_round_before_seat_resolved_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        let msg = new_round_msg(json!({
            "doras": ["1m"],
            "scores": [25000, 25000, 25000, 25000],
            "tiles": ["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"],
        }));
        let events = bridge.dispatch(&msg);
        assert!(events.is_empty());
    }

    fn action_msg(name: &str, data: JsonValue) -> ParsedMessage {
        ParsedMessage {
            msg_type: MessageType::Notify,
            msg_id: None,
            method_name: Arc::from(METHOD_ACTION_PROTOTYPE),
            payload: json!({ "step": 1, "name": name, "data": data }),
        }
    }

    /// Other player's draw — server omits the tile field. We must emit
    /// tsumo with pai = "?" rather than fabricating one.
    #[test]
    fn action_deal_tile_other_player_uses_unknown() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(
            ACTION_DEAL_TILE,
            json!({ "left_tile_count": 68, "seat": 1 }),
        );
        let events = bridge.dispatch(&msg);
        assert_eq!(events.len(), 1);
        match &events[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "?");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Our own draw — server tells us the actual tile, mapped through
    /// `ms_to_mjai`.
    #[test]
    fn action_deal_tile_self_uses_real_tile() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(
            ACTION_DEAL_TILE,
            json!({ "left_tile_count": 67, "seat": 2, "tile": "0p" }),
        );
        let events = bridge.dispatch(&msg);
        match &events[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 2);
                assert_eq!(pai, "5pr"); // 0p → red five
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Even if the server somehow includes a tile addressed to a different
    /// seat, we must not leak it — mjai sees `"?"` for non-self draws.
    #[test]
    fn action_deal_tile_ignores_tile_for_other_seat() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(ACTION_DEAL_TILE, json!({ "seat": 1, "tile": "5m" }));
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "?");
            }
            other => panic!("expected Tsumo, got {other:?}"),
        }
    }

    /// Dealer's first discard: Majsoul omits the seat field (default 0).
    /// `moqie` is also absent → tsumogiri = false.
    #[test]
    fn action_discard_tile_first_discard_defaults_to_seat_zero() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(ACTION_DISCARD_TILE, json!({ "tile": "9s" }));
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "9s");
                assert!(!*tsumogiri);
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    /// Tsumogiri (`moqie: true`) — drawn-and-immediately-discarded.
    #[test]
    fn action_discard_tile_tsumogiri_propagates() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(
            ACTION_DISCARD_TILE,
            json!({ "moqie": true, "seat": 1, "tile": "4z" }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "N"); // 4z → N (north)
                assert!(*tsumogiri);
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    /// Discard with `moqie:false` explicit — `tsumogiri` must be false.
    #[test]
    fn action_discard_tile_explicit_non_tsumogiri() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(
            ACTION_DISCARD_TILE,
            json!({ "moqie": false, "seat": 3, "tile": "0s" }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 3);
                assert_eq!(pai, "5sr"); // red 5s
                assert!(!*tsumogiri);
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    /// ActionDealTile before authGame must skip — we can't decide self vs other.
    #[test]
    fn action_deal_tile_before_seat_resolved_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        let msg = action_msg(ACTION_DEAL_TILE, json!({ "seat": 1, "tile": "5m" }));
        assert!(bridge.dispatch(&msg).is_empty());
    }

    /// Discard with empty/missing tile must be skipped, not turned into a
    /// `dahai("")` that would crash downstream.
    #[test]
    fn action_discard_tile_missing_tile_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        assert!(bridge
            .dispatch(&action_msg(ACTION_DISCARD_TILE, json!({})))
            .is_empty());
        assert!(bridge
            .dispatch(&action_msg(ACTION_DISCARD_TILE, json!({ "tile": "" })))
            .is_empty());
    }

    #[test]
    fn ankan_consumed_with_red_five() {
        // Ankan of 5m: must contain exactly one 5mr (the only red 5m in
        // the wall) and three regular 5m.
        let consumed = super::ankan_consumed("5m");
        assert_eq!(consumed, ["5mr", "5m", "5m", "5m"].map(String::from));
        let consumed = super::ankan_consumed("5mr");
        assert_eq!(consumed, ["5mr", "5m", "5m", "5m"].map(String::from));
    }

    #[test]
    fn ankan_consumed_no_red_form() {
        let consumed = super::ankan_consumed("E");
        assert_eq!(consumed, ["E", "E", "E", "E"].map(String::from));
        let consumed = super::ankan_consumed("1p");
        assert_eq!(consumed, ["1p", "1p", "1p", "1p"].map(String::from));
    }

    #[test]
    fn kakan_consumed_red_handling() {
        // Adding normal 5m on top of an existing pon → the pon must
        // already contain the red.
        assert_eq!(
            super::kakan_consumed("5m"),
            ["5mr", "5m", "5m"].map(String::from)
        );
        // Adding the red 5m → existing pon was three normal 5m.
        assert_eq!(
            super::kakan_consumed("5mr"),
            ["5m", "5m", "5m"].map(String::from)
        );
        // Non-five tiles never have a red form.
        assert_eq!(
            super::kakan_consumed("9p"),
            ["9p", "9p", "9p"].map(String::from)
        );
    }

    /// Pon by us on actor 1's discard. Three of the four entries in
    /// `froms` are `actor`; the one foreign seat is the discarder.
    #[test]
    fn action_chi_peng_gang_emits_pon() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(
            ACTION_CHI_PENG_GANG,
            json!({
                "seat": 2,
                "type": 1,
                "tiles": ["4m", "4m", "4m"],
                "froms": [2, 2, 1],
            }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Pon {
                actor,
                target,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 2);
                assert_eq!(*target, 1);
                assert_eq!(pai, "4m");
                assert_eq!(consumed, &["4m", "4m"].map(String::from));
            }
            other => panic!("expected Pon, got {other:?}"),
        }
        // pon doesn't schedule any kan-dora.
        assert!(bridge.dora_timing.is_none());
    }

    /// Chi from kamicha (seat 0 → seat 1). Carries red five in the run.
    #[test]
    fn action_chi_peng_gang_emits_chi_with_red() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(1);
        let msg = action_msg(
            ACTION_CHI_PENG_GANG,
            json!({
                "seat": 1,
                "type": 0,
                "tiles": ["3m", "4m", "0m"],
                "froms": [0, 1, 1],
            }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Chi {
                actor,
                target,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 1);
                assert_eq!(*target, 0);
                assert_eq!(pai, "3m");
                assert_eq!(consumed, &["4m", "5mr"].map(String::from));
            }
            other => panic!("expected Chi, got {other:?}"),
        }
    }

    /// 3p tables must never see a chi event. If Majsoul somehow sends one,
    /// drop it with a warning rather than propagating malformed mjai.
    #[test]
    fn action_chi_peng_gang_chi_dropped_in_three_player() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(1);
        bridge.num_players = 3;
        let msg = action_msg(
            ACTION_CHI_PENG_GANG,
            json!({
                "seat": 1,
                "type": 0,
                "tiles": ["3m", "4m", "5m"],
                "froms": [0, 1, 1],
            }),
        );
        let events = bridge.dispatch(&msg);
        assert!(events.is_empty(), "chi must be dropped in 3p");
    }

    /// `ActionBaBei` (3p kita) → mjai `kita` event with hardcoded `pai = "N"`
    /// (Majsoul omits the tile field; kita is always the North tile).
    /// Real payload captured from a live 3p AI match.
    #[test]
    fn action_ba_bei_emits_kita_event() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(1);
        bridge.num_players = 3;
        let events = bridge.dispatch(&action_msg(
            ACTION_BA_BEI,
            json!({
                "doras": [],
                "moqie": false,
                "seat": 0,
                "tile_state": 0,
                "tingpais": [],
                "zhenting": false,
            }),
        ));
        assert_eq!(events.len(), 1);
        match &events[0] {
            MjaiEvent::Kita { actor, pai } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai.as_deref(), Some("N"));
            }
            other => panic!("expected Kita, got {other:?}"),
        }
    }

    /// Kita updates `last_revealed_tile_actor` so a ron on the abstained
    /// north tile (chankan-style search-the-kita) targets the abstainer in
    /// the subsequent `ActionHule`.
    #[test]
    fn action_ba_bei_updates_last_revealed_tile_actor() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        bridge.num_players = 3;
        bridge.last_revealed_tile_actor = Some(99); // garbage to be overwritten
        bridge.dispatch(&action_msg(
            ACTION_BA_BEI,
            json!({ "seat": 0, "moqie": false, "doras": [], "tingpais": [], "zhenting": false, "tile_state": 0 }),
        ));
        assert_eq!(bridge.last_revealed_tile_actor, Some(0));
    }

    /// Kita does NOT flip a new dora marker (Tenhou rule). The bridge's
    /// dora bookkeeping must stay untouched after a kita event.
    #[test]
    fn action_ba_bei_does_not_change_dora_state() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        bridge.doras = vec!["E".to_string()];
        bridge.dispatch(&action_msg(
            ACTION_BA_BEI,
            json!({ "seat": 1, "moqie": false, "doras": [], "tingpais": [], "zhenting": false, "tile_state": 0 }),
        ));
        assert_eq!(bridge.doras, vec!["E".to_string()]);
        assert!(bridge.dora_timing.is_none());
        assert!(bridge.deferred_dora.is_none());
    }

    /// Riichi declaration tile passes through normally, then a kita
    /// happens. `reach_accepted` must be drained before the kita event
    /// (mjai state machine: declaration tile must pass through before any
    /// non-Hule action).
    #[test]
    fn action_ba_bei_drains_pending_reach_accepted() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 1, "tile": "5p", "is_liqi": true }),
        ));
        assert_eq!(bridge.pending_reach_accepted, Some(1));
        let events = bridge.dispatch(&action_msg(
            ACTION_BA_BEI,
            json!({ "seat": 2, "moqie": false, "doras": [], "tingpais": [], "zhenting": false, "tile_state": 0 }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::ReachAccepted { actor } => assert_eq!(*actor, 1),
            other => panic!("expected ReachAccepted first, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Kita { actor, .. } => assert_eq!(*actor, 2),
            other => panic!("expected Kita second, got {other:?}"),
        }
        assert!(bridge.pending_reach_accepted.is_none());
    }

    /// Daiminkan schedules dora 後乗り — flag must be set.
    #[test]
    fn action_chi_peng_gang_daiminkan_schedules_after_rinshan_dora() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let msg = action_msg(
            ACTION_CHI_PENG_GANG,
            json!({
                "seat": 2,
                "type": 2,
                "tiles": ["7s", "7s", "7s", "7s"],
                "froms": [2, 2, 2, 0],
            }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Daiminkan {
                actor,
                target,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 2);
                assert_eq!(*target, 0);
                assert_eq!(pai, "7s");
                assert_eq!(consumed, &["7s", "7s", "7s"].map(String::from));
            }
            other => panic!("expected Daiminkan, got {other:?}"),
        }
        assert_eq!(bridge.dora_timing, Some(DoraTiming::PendingAfterRinshan));
    }

    /// Ankan: schedules dora 即乗り (before rinshan). Consumed includes red.
    #[test]
    fn action_an_gang_add_gang_ankan() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(1);
        let msg = action_msg(
            ACTION_AN_GANG_ADD_GANG,
            json!({ "seat": 1, "type": 3, "tiles": "5m" }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Ankan { actor, consumed } => {
                assert_eq!(*actor, 1);
                assert_eq!(consumed, &["5mr", "5m", "5m", "5m"].map(String::from));
            }
            other => panic!("expected Ankan, got {other:?}"),
        }
        assert_eq!(bridge.dora_timing, Some(DoraTiming::PendingBeforeRinshan));
    }

    /// Kakan: schedules dora 後乗り; pai is the new tile, consumed is the
    /// existing 3 from the pon.
    #[test]
    fn action_an_gang_add_gang_kakan() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let msg = action_msg(
            ACTION_AN_GANG_ADD_GANG,
            json!({ "seat": 0, "type": 2, "tiles": "0p" }),
        );
        match &bridge.dispatch(&msg)[0] {
            MjaiEvent::Kakan {
                actor,
                pai,
                consumed,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(pai, "5pr");
                // New tile is the red, so existing 3 are normal.
                assert_eq!(consumed, &["5p", "5p", "5p"].map(String::from));
            }
            other => panic!("expected Kakan, got {other:?}"),
        }
        assert_eq!(bridge.dora_timing, Some(DoraTiming::PendingAfterRinshan));
    }

    /// Ankan flow end-to-end: ankan → ActionDealTile must produce
    /// `[Dora, Tsumo]` in that order (即乗り).
    #[test]
    fn ankan_then_rinshan_emits_dora_before_tsumo() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(1);
        bridge.doras = vec!["E".to_string()];

        // Ankan of 1z (E).
        bridge.dispatch(&action_msg(
            ACTION_AN_GANG_ADD_GANG,
            json!({ "seat": 1, "type": 3, "tiles": "1z" }),
        ));

        // Rinshan deal carries the new dora marker.
        let events = bridge.dispatch(&action_msg(
            ACTION_DEAL_TILE,
            json!({ "seat": 1, "tile": "5p", "doras": ["1z", "2z"] }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Dora { dora_marker } => assert_eq!(dora_marker, "S"),
            other => panic!("expected Dora first, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "5p");
            }
            other => panic!("expected Tsumo second, got {other:?}"),
        }
        assert!(bridge.dora_timing.is_none());
        assert!(bridge.deferred_dora.is_none());
    }

    /// Daiminkan flow: daiminkan → ActionDealTile must emit only Tsumo
    /// (dora deferred). Then ActionDiscardTile must prepend the dora
    /// before dahai (後乗り).
    #[test]
    fn daiminkan_then_rinshan_then_discard_emits_dora_before_dahai() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        bridge.doras = vec!["E".to_string()];

        bridge.dispatch(&action_msg(
            ACTION_CHI_PENG_GANG,
            json!({
                "seat": 2,
                "type": 2,
                "tiles": ["7s", "7s", "7s", "7s"],
                "froms": [2, 2, 2, 0],
            }),
        ));

        // Rinshan: tsumo only, dora deferred.
        let events = bridge.dispatch(&action_msg(
            ACTION_DEAL_TILE,
            json!({ "seat": 2, "tile": "9p", "doras": ["1z", "3z"] }),
        ));
        assert_eq!(events.len(), 1);
        match &events[0] {
            MjaiEvent::Tsumo { actor, pai } => {
                assert_eq!(*actor, 2);
                assert_eq!(pai, "9p");
            }
            other => panic!("expected single Tsumo, got {other:?}"),
        }
        assert_eq!(bridge.deferred_dora, Some("W".to_string()));
        assert!(bridge.dora_timing.is_none());

        // Next discard: dora flushed first, then dahai.
        let events = bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 2, "tile": "9p", "moqie": true }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Dora { dora_marker } => assert_eq!(dora_marker, "W"),
            other => panic!("expected Dora first, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 2);
                assert_eq!(pai, "9p");
                assert!(*tsumogiri);
            }
            other => panic!("expected Dahai second, got {other:?}"),
        }
        assert!(bridge.deferred_dora.is_none());
    }

    /// Kakan follows the same 後乗り timing as daiminkan.
    #[test]
    fn kakan_then_rinshan_then_discard_emits_dora_before_dahai() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.doras = vec!["E".to_string()];

        bridge.dispatch(&action_msg(
            ACTION_AN_GANG_ADD_GANG,
            json!({ "seat": 0, "type": 2, "tiles": "5m" }),
        ));

        let events = bridge.dispatch(&action_msg(
            ACTION_DEAL_TILE,
            json!({ "seat": 0, "tile": "1p", "doras": ["1z", "4z"] }),
        ));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MjaiEvent::Tsumo { .. }));
        assert_eq!(bridge.deferred_dora, Some("N".to_string()));

        let events = bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 0, "tile": "1p", "moqie": true }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Dora { dora_marker } => assert_eq!(dora_marker, "N"),
            other => panic!("expected Dora first, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::Dahai { .. }));
    }

    /// Riichi declaration: dahai with `is_liqi=true` produces
    /// `[reach, dahai]` and queues a reach_accepted for the next action.
    #[test]
    fn discard_with_is_liqi_emits_reach_then_dahai_and_queues_accept() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        let events = bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 1, "tile": "1z", "moqie": false, "is_liqi": true }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Reach { actor, .. } => assert_eq!(*actor, 1),
            other => panic!("expected Reach first, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai, "E");
                assert!(!*tsumogiri);
            }
            other => panic!("expected Dahai second, got {other:?}"),
        }
        assert_eq!(bridge.pending_reach_accepted, Some(1));
    }

    /// Double riichi: `is_wliqi` triggers the same flow as `is_liqi`.
    #[test]
    fn discard_with_is_wliqi_also_triggers_reach() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 0, "tile": "9m", "moqie": true, "is_wliqi": true }),
        ));
        assert!(matches!(events[0], MjaiEvent::Reach { actor: 0, .. }));
        assert_eq!(bridge.pending_reach_accepted, Some(0));
    }

    /// Declaration tile passes through to next player's draw — mjai spec
    /// requires `reach_accepted` *before* that next tsumo.
    #[test]
    fn reach_accepted_drains_before_next_tsumo() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 1, "tile": "1z", "is_liqi": true }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_DEAL_TILE,
            json!({ "seat": 2, "tile": "5p" }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::ReachAccepted { actor } => assert_eq!(*actor, 1),
            other => panic!("expected ReachAccepted first, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Tsumo { actor, .. } => assert_eq!(*actor, 2),
            other => panic!("expected Tsumo second, got {other:?}"),
        }
        assert!(bridge.pending_reach_accepted.is_none());
    }

    /// Declaration tile gets called (chi/pon/daiminkan) — riichi is still
    /// accepted, reach_accepted prepended to the call event.
    #[test]
    fn reach_accepted_drains_before_chi_pon_daiminkan() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 1, "tile": "5m", "is_liqi": true }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_CHI_PENG_GANG,
            json!({
                "seat": 2,
                "type": 1,
                "tiles": ["5m", "5m", "5m"],
                "froms": [2, 2, 1],
            }),
        ));
        assert!(matches!(events[0], MjaiEvent::ReachAccepted { actor: 1 }));
        assert!(matches!(
            events[1],
            MjaiEvent::Pon {
                actor: 2,
                target: 1,
                ..
            }
        ));
    }

    /// New kyoku must clear any leftover reach state.
    #[test]
    fn start_kyoku_resets_pending_reach_accepted() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.pending_reach_accepted = Some(2);
        bridge.dispatch(&new_round_msg(json!({
            "doras": ["1m"],
            "scores": [25000, 25000, 25000, 25000],
            "tiles": [
                "1m","2m","3m","4m","5m","6m","7m","8m","9m",
                "1p","2p","3p","4p","0p"
            ],
        })));
        assert!(bridge.pending_reach_accepted.is_none());
    }

    /// Real ActionNoTile sample (1 human in tenpai, 3 noten) →
    /// `[ryukyoku{deltas:[3000,-1000,-1000,-1000]}, end_kyoku]`.
    #[test]
    fn action_no_tile_emits_ryukyoku_with_deltas_then_end_kyoku() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(
            ACTION_NO_TILE,
            json!({
                "gameend": false,
                "liujumanguan": false,
                "players": [],
                "scores": [{
                    "delta_scores": [3000, -1000, -1000, -1000],
                    "old_scores": [25000, 17300, 32700, 25000],
                    "seat": 0,
                }],
            }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                assert_eq!(*deltas, Some(vec![3000, -1000, -1000, -1000]));
            }
            other => panic!("expected Ryukyoku first, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    /// Multiple `scores` entries (e.g. tenpai redistribution + nagashi
    /// mangan in the same frame) must be summed per seat.
    #[test]
    fn action_no_tile_sums_multiple_score_entries() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(
            ACTION_NO_TILE,
            json!({
                "scores": [
                    { "delta_scores": [1000, -500, 0, -500] },
                    { "delta_scores": [500, 1500, -1000, -1000] },
                ],
            }),
        ));
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                assert_eq!(*deltas, Some(vec![1500, 1000, -1000, -1500]));
            }
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
    }

    /// Real captured 3p `ActionNoTile` payload (1 tenpai / 2 noten).
    /// `delta_scores: [2000, -1000, -1000]` → length-3 deltas, no padding.
    /// Tenpai pool 2000 split per `reference/reference_mjai_3p.md`.
    #[test]
    fn action_no_tile_three_player_real_payload() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        let events = bridge.dispatch(&action_msg(
            ACTION_NO_TILE,
            json!({
                "gameend": false,
                "hules_history": [],
                "liujumanguan": false,
                "players": [
                    {
                        "already_hule": false,
                        "hand": ["2p","2p","2p","5p","6p","7p","8p","9p","4s","0s","6s","8s","8s"],
                        "tingpai": true,
                        "tings": [
                            { "tile": "7p", "fu": 40, "fu_zimo": 30 },
                            { "tile": "4p", "fu": 40, "fu_zimo": 30 },
                        ],
                    },
                    { "already_hule": false, "hand": [], "tingpai": false, "tings": [] },
                    { "already_hule": false, "hand": [], "tingpai": false, "tings": [] },
                ],
                "scores": [{
                    "delta_scores": [2000, -1000, -1000],
                    "doras": [],
                    "old_scores": [35000, 35000, 35000],
                    "seat": 0,
                }],
            }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                assert_eq!(*deltas, Some(vec![2000, -1000, -1000]));
            }
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    /// 3p ryukyoku: `delta_scores` arrives with 3 entries; emitted mjai
    /// `deltas` keeps native length 3 (no padding).
    #[test]
    fn action_no_tile_three_player_deltas_native_length() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        let events = bridge.dispatch(&action_msg(
            ACTION_NO_TILE,
            json!({
                "scores": [{ "delta_scores": [2000, -1000, -1000] }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => {
                assert_eq!(*deltas, Some(vec![2000, -1000, -1000]));
            }
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
    }

    /// Empty / missing `scores` → `deltas: None`.
    #[test]
    fn action_no_tile_without_scores_emits_no_deltas() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(ACTION_NO_TILE, json!({})));
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => assert!(deltas.is_none()),
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    /// `ActionLiuJu` (any abortive type) → `[ryukyoku{None}, end_kyoku]`.
    /// No point redistribution.
    #[test]
    fn action_liu_ju_emits_ryukyoku_no_deltas_then_end_kyoku() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(ACTION_LIU_JU, json!({ "type": 1, "seat": 0 })));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Ryukyoku { deltas } => assert!(deltas.is_none()),
            other => panic!("expected Ryukyoku, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    /// Riichi declared on the very last possible turn, then the round
    /// immediately ends in ryukyoku — `reach_accepted` must still be
    /// emitted (declaration tile passed through, no ron) before the
    /// terminal events.
    #[test]
    fn reach_accepted_drains_before_no_tile_ryukyoku() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 2, "tile": "9p", "is_liqi": true }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_NO_TILE,
            json!({ "scores": [{ "delta_scores": [-1500, -1500, 0, -1500] }] }),
        ));
        assert_eq!(events.len(), 3);
        match &events[0] {
            MjaiEvent::ReachAccepted { actor } => assert_eq!(*actor, 2),
            other => panic!("expected ReachAccepted first, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::Ryukyoku { .. }));
        assert!(matches!(events[2], MjaiEvent::EndKyoku));
        assert!(bridge.pending_reach_accepted.is_none());
    }

    /// Real ActionHule sample (single ron, non-riichi): seat 3 rons off
    /// seat 2's discard. Expected: `[Hora{actor:3, target:2, deltas, no
    /// ura}, EndKyoku]`.
    #[test]
    fn action_hule_ron_emits_hora_then_end_kyoku() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        // Simulate seat 2 having just discarded.
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 2, "tile": "0p" }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [0, 0, -2000, 2000],
                "old_scores": [25000, 25000, 25000, 25000],
                "scores": [25000, 25000, 23000, 27000],
                "hules": [{
                    "seat": 3,
                    "zimo": false,
                    "liqi": false,
                    "hu_tile": "0p",
                    "li_doras": [],
                }],
            }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ura_markers,
            } => {
                assert_eq!(*actor, 3);
                assert_eq!(*target, 2);
                assert_eq!(*deltas, Some(vec![0, 0, -2000, 2000]));
                assert!(ura_markers.is_none(), "no riichi → no ura_markers");
            }
            other => panic!("expected Hora first, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    /// Self-tsumo win: actor == target, deltas applies to self & all
    /// payers, no ura when no riichi.
    #[test]
    fn action_hule_tsumo_actor_equals_target() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [4000, -2000, -1000, -1000],
                "hules": [{ "seat": 0, "zimo": true, "liqi": false }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ura_markers,
            } => {
                assert_eq!(*actor, 0);
                assert_eq!(*target, 0);
                assert_eq!(*deltas, Some(vec![4000, -2000, -1000, -1000]));
                assert!(ura_markers.is_none());
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }

    /// Riichi win surfaces ura markers from `li_doras` (mjai-mapped).
    #[test]
    fn action_hule_riichi_win_emits_ura_markers() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 2, "tile": "1m" }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [0, 0, -8000, 8000],
                "hules": [{
                    "seat": 3,
                    "zimo": false,
                    "liqi": true,
                    "li_doras": ["7z", "0s"],
                }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Hora { ura_markers, .. } => {
                assert_eq!(
                    ura_markers.as_deref(),
                    Some(&["C".to_string(), "5sr".to_string()][..]),
                );
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }

    /// Riichi win with `li_doras: []` still produces `Some(vec![])` so
    /// consumers can tell apart "had riichi but no ura" from "no riichi".
    #[test]
    fn action_hule_riichi_with_empty_li_doras_is_some_empty() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 2, "tile": "1m" }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [0, 0, -1000, 1000],
                "hules": [{ "seat": 3, "zimo": false, "liqi": true, "li_doras": [] }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Hora { ura_markers, .. } => {
                assert_eq!(ura_markers.as_deref(), Some(&[][..]));
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }

    /// Multi-ron (double): two `Hora` events then one `EndKyoku`. Same
    /// total deltas attached to each.
    #[test]
    fn action_hule_double_ron_emits_two_hora_then_end_kyoku() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 0, "tile": "5m" }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [-9000, 4000, 0, 5000],
                "hules": [
                    { "seat": 1, "zimo": false, "liqi": false },
                    { "seat": 3, "zimo": false, "liqi": false },
                ],
            }),
        ));
        assert_eq!(events.len(), 3);
        match &events[0] {
            MjaiEvent::Hora { actor, target, .. } => {
                assert_eq!(*actor, 1);
                assert_eq!(*target, 0);
            }
            other => panic!("expected Hora, got {other:?}"),
        }
        match &events[1] {
            MjaiEvent::Hora { actor, target, .. } => {
                assert_eq!(*actor, 3);
                assert_eq!(*target, 0);
            }
            other => panic!("expected Hora, got {other:?}"),
        }
        assert!(matches!(events[2], MjaiEvent::EndKyoku));
    }

    /// Ron on the declaration tile: riichi voided, so no `reach_accepted`
    /// prepended even though `pending_reach_accepted` was set by the
    /// declarer's discard.
    #[test]
    fn action_hule_on_declaration_tile_voids_riichi() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 2, "tile": "1z", "is_liqi": true }),
        ));
        assert_eq!(bridge.pending_reach_accepted, Some(2));
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [0, 0, -8000, 8000],
                "hules": [{ "seat": 3, "zimo": false, "liqi": false, "hu_tile": "1z" }],
            }),
        ));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, MjaiEvent::ReachAccepted { .. })),
            "ron on declaration tile must NOT emit reach_accepted"
        );
        assert!(bridge.pending_reach_accepted.is_none());
        // Still emits the hora and end_kyoku.
        assert!(events.iter().any(|e| matches!(e, MjaiEvent::Hora { .. })));
        assert!(events.iter().any(|e| matches!(e, MjaiEvent::EndKyoku)));
    }

    /// Ron without any preceding `dahai` is malformed (no discarder to
    /// target). Skip with a warning instead of guessing.
    #[test]
    fn action_hule_ron_without_last_revealed_tile_actor_skips() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [0, 0, -2000, 2000],
                "hules": [{ "seat": 3, "zimo": false }],
            }),
        ));
        assert!(events.is_empty());
    }

    /// 搶槓 (chankan): seat 1 declares kakan; seat 3 rons. Target must be
    /// the kakan declarer (seat 1), not whoever happened to discard last.
    #[test]
    fn action_hule_chankan_targets_kakan_declarer() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        // Earlier in the round: seat 0 discarded (this should NOT be the
        // ron target if a kakan happens in between).
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 0, "tile": "9m" }),
        ));
        // Seat 1 declares kakan on 5m.
        bridge.dispatch(&action_msg(
            ACTION_AN_GANG_ADD_GANG,
            json!({ "seat": 1, "type": 2, "tiles": "5m" }),
        ));
        assert_eq!(bridge.last_revealed_tile_actor, Some(1));

        // Seat 3 rons (chankan).
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [0, -8000, 0, 8000],
                "hules": [{ "seat": 3, "zimo": false, "liqi": false, "hu_tile": "5m" }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Hora { actor, target, .. } => {
                assert_eq!(*actor, 3);
                assert_eq!(*target, 1, "chankan target must be the kakan declarer");
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }

    /// 国士無双搶暗槓: seat 2 declares ankan; seat 0 (holding kokushi)
    /// rons. Mjai target = ankan declarer.
    #[test]
    fn action_hule_kokushi_robs_ankan_targets_ankan_declarer() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 3, "tile": "9p" }),
        ));
        // Seat 2 declares ankan on East (1z → "E").
        bridge.dispatch(&action_msg(
            ACTION_AN_GANG_ADD_GANG,
            json!({ "seat": 2, "type": 3, "tiles": "1z" }),
        ));
        assert_eq!(bridge.last_revealed_tile_actor, Some(2));

        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [32000, 0, -32000, 0],
                "hules": [{
                    "seat": 0,
                    "zimo": false,
                    "liqi": false,
                    "hu_tile": "1z",
                    "yiman": true,
                }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Hora { actor, target, .. } => {
                assert_eq!(*actor, 0);
                assert_eq!(*target, 2, "kokushi rob ankan: target = ankan declarer");
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }

    /// Real captured 3p `ActionHule` payload: seat 2 rons off seat 0
    /// (riichi+haitei, dama with ura). `delta_scores: [-12000, 0, 13000]`
    /// length 3, with the +1000 honba/riichi-stick bundled into the winner's
    /// 13000. Verifies length-3 deltas, ura markers from `li_doras`.
    #[test]
    fn action_hule_three_player_real_ron_payload() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.num_players = 3;
        // Seat 0 just discarded — sets last_revealed_tile_actor for the ron target.
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 0, "tile": "3s", "moqie": false }),
        ));
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "baopai": 0,
                "delta_scores": [-12000, 0, 13000],
                "doras": [],
                "old_scores": [35000, 35000, 34000],
                "scores": [23000, 35000, 47000],
                "wait_timeout": 0,
                "hules": [{
                    "baida_changed": [],
                    "baopai": 0,
                    "baopai_seats": [],
                    "count": 7,
                    "dadian": 12000,
                    "doras": ["6s"],
                    "fans": [
                        { "id": 25, "val": 2 },
                        { "id": 2,  "val": 1 },
                        { "id": 33, "val": 2 },
                        { "id": 34, "val": 2 },
                    ],
                    "fu": 25,
                    "hand": ["9m","9m","3p","3p","6p","6p","2s","2s","3s","5s","5s","9s","9s"],
                    "hu_tile": "3s",
                    "hu_tile_bai_da_changed": "",
                    "li_doras": ["5p"],
                    "lines": [],
                    "liqi": true,
                    "ming": [],
                    "point_rong": 12000,
                    "point_sum": 12000,
                    "qinjia": false,
                    "seat": 2,
                    "yiman": false,
                    "zimo": false,
                }],
            }),
        ));
        assert_eq!(events.len(), 2);
        match &events[0] {
            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ura_markers,
            } => {
                assert_eq!(*actor, 2);
                assert_eq!(*target, 0);
                assert_eq!(*deltas, Some(vec![-12000, 0, 13000]));
                assert_eq!(ura_markers.as_deref(), Some(&["5p".to_string()][..]));
            }
            other => panic!("expected Hora, got {other:?}"),
        }
        assert!(matches!(events[1], MjaiEvent::EndKyoku));
    }

    /// **Ron on kita (搶北)** — Majsoul wire format: `ActionBaBei` followed
    /// by `ActionHule` with `hu_tile: "4z"`, `zimo: false`. The ron's
    /// `target` must attribute to the kita declarer (whoever set aside the
    /// north tile), not whoever discarded last.
    ///
    /// Confirmed against Akagi-Python and AkagiNG: no separate
    /// "kita_chankan" message exists; the hule arrives in the normal
    /// `ActionHule` slot. `build_kita` sets `last_revealed_tile_actor` so
    /// `build_hule` picks up the kita declarer as `target` automatically.
    /// `chankan` yaku is NOT awarded on ron-on-kita (Tenhou rule), but that
    /// distinction is the bot's concern — the bridge just emits the events.
    #[test]
    fn ron_on_kita_targets_kita_declarer() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(2); // observer is the ronner
        bridge.num_players = 3;
        // Earlier in the round seat 1 had discarded — must NOT be the ron target.
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 1, "tile": "5p", "moqie": false }),
        ));
        // Seat 0 declares kita.
        bridge.dispatch(&action_msg(
            ACTION_BA_BEI,
            json!({ "seat": 0, "moqie": false, "doras": [], "tingpais": [], "zhenting": false, "tile_state": 0 }),
        ));
        assert_eq!(bridge.last_revealed_tile_actor, Some(0));
        // Seat 2 rons the kita tile (hu_tile = 4z = N).
        let events = bridge.dispatch(&action_msg(
            ACTION_HULE,
            json!({
                "delta_scores": [-8000, 0, 8000],
                "hules": [{
                    "seat": 2,
                    "zimo": false,
                    "liqi": false,
                    "hu_tile": "4z",
                }],
            }),
        ));
        match &events[0] {
            MjaiEvent::Hora { actor, target, .. } => {
                assert_eq!(*actor, 2);
                assert_eq!(*target, 0, "ron-on-kita target = kita declarer");
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }

    /// `last_revealed_tile_actor` resets at start_kyoku — a ron in the new round must
    /// not target a discarder from the previous round.
    #[test]
    fn start_kyoku_resets_last_revealed_tile_actor() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.last_revealed_tile_actor = Some(2);
        bridge.dispatch(&new_round_msg(json!({
            "doras": ["1m"],
            "scores": [25000, 25000, 25000, 25000],
            "tiles": [
                "1m","2m","3m","4m","5m","6m","7m","8m","9m",
                "1p","2p","3p","4p","0p"
            ],
        })));
        assert!(bridge.last_revealed_tile_actor.is_none());
    }

    /// Earlier reach_voided test used an empty `{}` ActionHule payload.
    /// With proper hule support that returns Err (no hules); ensure
    /// queue still gets cleared regardless of build success.
    #[test]
    fn reach_voided_by_ron_on_declaration_tile() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dispatch(&action_msg(
            ACTION_DISCARD_TILE,
            json!({ "seat": 1, "tile": "5m", "is_liqi": true }),
        ));
        assert_eq!(bridge.pending_reach_accepted, Some(1));
        let events = bridge.dispatch(&action_msg(ACTION_HULE, json!({})));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, MjaiEvent::ReachAccepted { .. })),
            "no reach_accepted on ron of declaration tile"
        );
        assert!(bridge.pending_reach_accepted.is_none());
    }

    /// `NotifyGameEndResult` (top-level Notify, NOT ActionPrototype) →
    /// `[EndGame]`. Final standings live in the flow log; mjai's
    /// `end_game` carries no payload.
    #[test]
    fn notify_game_end_result_emits_end_game() {
        let mut bridge = MajsoulBridge::new(None, None);
        let msg = ParsedMessage {
            msg_type: MessageType::Notify,
            msg_id: None,
            method_name: Arc::from(METHOD_NOTIFY_GAME_END_RESULT),
            payload: json!({
                "result": {
                    "players": [
                        { "seat": 1, "total_point":  33800, "part_point_1": 43800 },
                        { "seat": 3, "total_point":   4700, "part_point_1": 24700 },
                        { "seat": 0, "total_point":  -9500, "part_point_1": 20500 },
                        { "seat": 2, "total_point": -29000, "part_point_1": 11000 },
                    ]
                }
            }),
        };
        let events = bridge.dispatch(&msg);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], MjaiEvent::EndGame));
    }

    /// State must reset on a new kyoku — a stray `deferred_dora` from the
    /// previous round can't leak into the next.
    #[test]
    fn start_kyoku_resets_dora_state() {
        let mut bridge = MajsoulBridge::new(None, None);
        bridge.seat = Some(0);
        bridge.dora_timing = Some(DoraTiming::PendingBeforeRinshan);
        bridge.deferred_dora = Some("S".into());

        bridge.dispatch(&new_round_msg(json!({
            "doras": ["1m"],
            "scores": [25000, 25000, 25000, 25000],
            "tiles": [
                "1m","2m","3m","4m","5m","6m","7m","8m","9m",
                "1p","2p","3p","4p","0p"
            ],
        })));
        assert!(bridge.dora_timing.is_none());
        assert!(bridge.deferred_dora.is_none());
        assert_eq!(bridge.doras, vec!["1m".to_string()]);
    }
}
