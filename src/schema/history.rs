//! Persisted game-history records.
//!
//! `GameRecord` is what `crate::history::recorder` writes to
//! `<history_root>/index.jsonl` (one JSON line per finalised game) and what
//! the frontend reads back via Tauri commands. It mirrors the shape of
//! the `Stat` record from Mortal's `libriichi` for per-game counts so
//! summing across records gives stat-equivalent aggregates.
//!
//! Wire format is internally-tagged where useful (`Platform`, `KyokuMode`,
//! `HistoryEvent`) to keep the JSON shape stable as variants are added.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::schema::MjaiEvent;

// ---------- Platform / KyokuMode ----------

/// Bridge that produced the record. `Unknown` is the safety net for a
/// future bridge whose tag is added before the schema knows about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Majsoul,
    Tenhou,
    RiichiCity,
    HoloMahjong,
    Mjai,
    Unknown,
}

/// Game length. Uses a platform-declared match mode when available and falls
/// back to the highest `bakaze` observed in `start_kyoku` for legacy streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KyokuMode {
    /// Only `"E"` rounds — tonpuu (east-only).
    EastOnly,
    /// At least one `"S"` round — hanchan (east-south).
    EastSouth,
    /// Saw `"W"` or `"N"` — west / north overtime, treated as hanchan
    /// for scoring (Majsoul never uses these except as continuation).
    Other,
}

// ---------- MatchInfo ----------

/// Platform-specific match identity captured at `start_game`: which room /
/// rank lobby the game was played in, plus the platform's own game id.
///
/// Values are stored **raw** (numeric ids, wire strings). Mapping them to
/// display labels ("Gold Room East", "Houou") is a frontend concern so an
/// id the app doesn't know yet degrades to showing the number instead of a
/// wrong label. Tagged like [`Platform`] so the JSON is self-describing;
/// every field is optional because each bridge fills only what its wire
/// actually carried.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum MatchInfo {
    /// Mahjong Soul. `mode_id` is `game_config.meta.mode_id` — the ranked
    /// matchmaking mode (1..=28 covers Bronze..Throne and Melee, 4p and 3p);
    /// 0 for non-matchmade tables. `room_id` is the friendly-room number and
    /// `contest_uid` the tournament id, when applicable.
    Majsoul {
        /// Raw `ReqAuthGame.game_uuid` — the paifu (replay) identifier.
        #[serde(default)]
        game_uuid: Option<String>,
        #[serde(default)]
        mode_id: Option<u32>,
        #[serde(default)]
        room_id: Option<u32>,
        #[serde(default)]
        contest_uid: Option<u32>,
    },
    /// Tenhou. `log_id` is the paifu id from `<TAIKYOKU log=…>`; `go_type`
    /// is the raw `<GO type=…>` rule/room bitfield (room tier in bits 0x20 /
    /// 0x80); `lobby` is the lobby number from the same message.
    Tenhou {
        #[serde(default)]
        log_id: Option<String>,
        #[serde(default)]
        go_type: Option<u32>,
        #[serde(default)]
        lobby: Option<u32>,
    },
    /// Riichi City. `stage_type` / `game_play` / `classify_id` come from
    /// `cmd_enter_room.options` and identify the matchmaking room;
    /// `stage_type` 1..=4 = Star / Moon / Sun / Galaxy (新星/霞月/炎陽/銀河,
    /// the ranked room tiers — the client's tier enum is exhaustive at
    /// four) and `game_play` is the client's `GamePlayType` enum (its
    /// game logic ships as Lua; the full id→mode mapping with the
    /// client's own localized names lives in the frontend's
    /// `matchInfo.ts`).
    /// `room_id` is the table-instance token from the `cmd_enter_room`
    /// wrapper. All raw wire values.
    RiichiCity {
        #[serde(default)]
        room_id: Option<String>,
        #[serde(default)]
        classify_id: Option<String>,
        #[serde(default)]
        stage_type: Option<i64>,
        #[serde(default)]
        game_play: Option<i64>,
    },
}

// ---------- GameStats ----------

/// Per-game counters mirroring `libriichi::stat::Stat`. All fields are
/// from the *recorded player's* perspective. Summing across records
/// (frontend aggregation) yields the same numbers `stat.rs` would
/// compute when processing the underlying mjai logs directly.
///
/// Δscore semantics follow the reference notes:
/// - Riichi Δscores cover all kyotaku *except* the 1000-point sengenhai
///   stake of the riichi declaration itself.
/// - Every other Δscore covers all kyotaku.
/// - Ankan does not count as fuuro.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameStats {
    pub round: i64,
    pub oya: i64,

    pub fuuro: i64,
    pub fuuro_num: i64,
    pub fuuro_point: i64,
    pub fuuro_agari: i64,
    pub fuuro_agari_jun: i64,
    pub fuuro_agari_point: i64,
    pub fuuro_houjuu: i64,

    pub agari: i64,
    pub agari_as_oya: i64,
    pub agari_jun: i64,
    pub agari_point_oya: i64,
    pub agari_point_ko: i64,

    pub houjuu: i64,
    pub houjuu_jun: i64,
    pub houjuu_to_oya: i64,
    pub houjuu_point_to_oya: i64,
    pub houjuu_point_to_ko: i64,

    pub riichi: i64,
    pub riichi_as_oya: i64,
    pub riichi_jun: i64,
    pub riichi_agari: i64,
    pub riichi_agari_point: i64,
    pub riichi_agari_jun: i64,
    pub riichi_houjuu: i64,
    pub riichi_ryukyoku: i64,
    pub riichi_point: i64,
    pub chasing_riichi: i64,
    pub riichi_got_chased: i64,

    pub dama_agari: i64,
    pub dama_agari_jun: i64,
    pub dama_agari_point: i64,

    pub ryukyoku: i64,
    pub ryukyoku_point: i64,

    pub yakuman: i64,
    pub nagashi_mangan: i64,
}

// ---------- GameRecord ----------

/// One finalised game persisted to `index.jsonl`. The full mjai event
/// stream is stored separately at `games/<id>.mjai.jsonl` (see
/// `log_path`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameRecord {
    /// ULID; lexicographically sortable by start time. Doubles as
    /// filename stem under `games/`.
    pub id: String,

    /// Wall-clock time the recorder saw the game's first event.
    pub started_at: DateTime<Utc>,
    /// Wall-clock time `EndGame` arrived.
    pub ended_at: DateTime<Utc>,

    pub platform: Platform,
    /// 3 (sanma) or 4 (yonma).
    pub num_players: u8,
    pub kyoku_mode: KyokuMode,

    /// Player display names, indexed by seat (length = `num_players`).
    pub names: Vec<String>,

    /// The recorded player's seat, taken from `start_game.id`. `None`
    /// when the bridge was in observer/replay mode and no own-seat was
    /// declared — in that case `our_rank`/`our_delta` are also `None`
    /// and the frontend skips the game in cumulative-PT charts.
    pub our_seat: Option<u8>,

    /// Final scores per seat. Authoritative platform standings are preferred;
    /// otherwise Mortal-style 100k (4p) / 105k (3p) normalisation is used.
    /// Length = `num_players`.
    pub final_scores: Vec<i32>,

    /// Final rank (1..=num_players) per seat. Uses authoritative platform
    /// standings when available, otherwise descending score with an ascending
    /// seat tiebreak.
    pub final_ranks: Vec<u8>,

    pub our_rank: Option<u8>,
    /// `final_score[our_seat] - starting_score`. Starting = 25_000 (4p)
    /// / 35_000 (3p). Used by frontend PT formulas as the "(score-25000)
    /// /1000" base term.
    pub our_delta: Option<i32>,

    pub stats: GameStats,

    /// Platform-specific match identity (rank room, game/paifu id). `None`
    /// for records written before this field existed and for bridges that
    /// don't provide it.
    #[serde(default)]
    pub match_info: Option<MatchInfo>,

    /// Path of the mjai.jsonl copy, relative to the history root —
    /// always `"games/<id>.mjai.jsonl"`.
    pub log_path: String,
}

// ---------- HistoryFilter ----------

/// Filter for `list_game_history` / `get_game_history_aggregate`. All
/// fields are optional — `Default` matches everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryFilter {
    pub platform: Option<Platform>,
    /// 3 or 4. Filters by `num_players`.
    pub num_players: Option<u8>,
    pub kyoku_mode: Option<KyokuMode>,
    /// Inclusive lower bound on `started_at`.
    pub started_after: Option<DateTime<Utc>>,
    /// Exclusive upper bound on `started_at`.
    pub started_before: Option<DateTime<Utc>>,
}

impl HistoryFilter {
    /// True when `record` passes every populated filter clause.
    pub fn matches(&self, record: &GameRecord) -> bool {
        if let Some(p) = self.platform {
            if record.platform != p {
                return false;
            }
        }
        if let Some(n) = self.num_players {
            if record.num_players != n {
                return false;
            }
        }
        if let Some(m) = self.kyoku_mode {
            if record.kyoku_mode != m {
                return false;
            }
        }
        if let Some(after) = self.started_after {
            if record.started_at < after {
                return false;
            }
        }
        if let Some(before) = self.started_before {
            if record.started_at >= before {
                return false;
            }
        }
        true
    }
}

// ---------- HistoryEvent ----------

/// Backend → frontend notification when a new record lands. Forwarded as
/// the Tauri `history-recorded` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryEvent {
    /// A finalised game was just appended to the index. Payload is the
    /// full record so the frontend can prepend without an extra fetch.
    Recorded { record: Box<GameRecord> },
    /// A record (and its mjai log copy) was deleted via the IPC command.
    Deleted { id: String },
}

// ---------- HistoryEventLog ----------

/// On-disk shape of `games/<id>.mjai.jsonl` — exactly the buffered mjai
/// stream of a finalised game. Type alias kept to make the storage
/// contract explicit.
pub type HistoryEventLog = Vec<MjaiEvent>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_round_trips_lowercase() {
        let j = serde_json::to_string(&Platform::Majsoul).unwrap();
        assert_eq!(j, "\"majsoul\"");
        let back: Platform = serde_json::from_str(&j).unwrap();
        assert_eq!(back, Platform::Majsoul);
    }

    #[test]
    fn kyoku_mode_round_trips() {
        for m in [KyokuMode::EastOnly, KyokuMode::EastSouth, KyokuMode::Other] {
            let j = serde_json::to_string(&m).unwrap();
            let back: KyokuMode = serde_json::from_str(&j).unwrap();
            assert_eq!(back, m);
        }
    }

    #[test]
    fn match_info_round_trips_and_omits_none_fields() {
        let infos = [
            MatchInfo::Majsoul {
                game_uuid: Some("240101-abcd".into()),
                mode_id: Some(9),
                room_id: None,
                contest_uid: None,
            },
            MatchInfo::Tenhou {
                log_id: Some("2026082300gm-00a9-0000-deadbeef".into()),
                go_type: Some(169),
                lobby: Some(0),
            },
            MatchInfo::RiichiCity {
                room_id: Some("tabletoken0001".into()),
                classify_id: Some("classifytoken0001".into()),
                stage_type: Some(1),
                game_play: Some(1001),
            },
        ];
        for info in infos {
            let j = serde_json::to_value(&info).unwrap();
            let back: MatchInfo = serde_json::from_value(j).unwrap();
            assert_eq!(back, info);
        }

        let j = serde_json::to_value(MatchInfo::Majsoul {
            game_uuid: None,
            mode_id: Some(12),
            room_id: None,
            contest_uid: None,
        })
        .unwrap();
        assert_eq!(j["platform"], "majsoul");
        assert_eq!(j["mode_id"], 12);
        assert!(j.get("game_uuid").is_none(), "None fields must be omitted");

        // A stored variant missing optional fields entirely still parses.
        let back: MatchInfo = serde_json::from_str(r#"{"platform":"tenhou"}"#).unwrap();
        assert_eq!(
            back,
            MatchInfo::Tenhou {
                log_id: None,
                go_type: None,
                lobby: None
            }
        );
    }

    /// Records written before `match_info` existed (no such key in the JSON
    /// line) keep parsing, defaulting to `None`.
    #[test]
    fn game_record_without_match_info_still_parses() {
        let r = GameRecord {
            id: "OLD".into(),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            platform: Platform::Majsoul,
            num_players: 4,
            kyoku_mode: KyokuMode::EastSouth,
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            our_seat: Some(0),
            final_scores: vec![30000, 25000, 25000, 20000],
            final_ranks: vec![1, 2, 3, 4],
            our_rank: Some(1),
            our_delta: Some(5000),
            stats: GameStats::default(),
            match_info: Some(MatchInfo::Majsoul {
                game_uuid: Some("u".into()),
                mode_id: Some(8),
                room_id: None,
                contest_uid: None,
            }),
            log_path: "games/OLD.mjai.jsonl".into(),
        };
        let mut j = serde_json::to_value(&r).unwrap();
        j.as_object_mut().unwrap().remove("match_info");
        let back: GameRecord = serde_json::from_value(j).unwrap();
        assert_eq!(back.match_info, None);
        assert_eq!(back.id, r.id);
    }

    #[test]
    fn history_filter_default_matches_everything() {
        let r = GameRecord {
            id: "01ARZ".into(),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            platform: Platform::Majsoul,
            num_players: 4,
            kyoku_mode: KyokuMode::EastOnly,
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            our_seat: Some(0),
            final_scores: vec![30000, 25000, 25000, 20000],
            final_ranks: vec![1, 2, 3, 4],
            our_rank: Some(1),
            our_delta: Some(5000),
            stats: GameStats::default(),
            match_info: None,
            log_path: "games/01ARZ.mjai.jsonl".into(),
        };
        assert!(HistoryFilter::default().matches(&r));
    }

    #[test]
    fn history_filter_platform_mismatch() {
        let r = GameRecord {
            id: "x".into(),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            platform: Platform::Majsoul,
            num_players: 4,
            kyoku_mode: KyokuMode::EastOnly,
            names: vec![],
            our_seat: None,
            final_scores: vec![],
            final_ranks: vec![],
            our_rank: None,
            our_delta: None,
            stats: GameStats::default(),
            match_info: None,
            log_path: "x".into(),
        };
        let f = HistoryFilter {
            platform: Some(Platform::Tenhou),
            ..Default::default()
        };
        assert!(!f.matches(&r));
    }

    #[test]
    fn history_event_recorded_round_trips() {
        let rec = GameRecord {
            id: "rec1".into(),
            started_at: Utc::now(),
            ended_at: Utc::now(),
            platform: Platform::Majsoul,
            num_players: 4,
            kyoku_mode: KyokuMode::EastSouth,
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            our_seat: Some(2),
            final_scores: vec![25000, 25000, 25000, 25000],
            final_ranks: vec![1, 2, 3, 4],
            our_rank: Some(3),
            our_delta: Some(0),
            stats: GameStats::default(),
            match_info: None,
            log_path: "games/rec1.mjai.jsonl".into(),
        };
        let ev = HistoryEvent::Recorded {
            record: Box::new(rec),
        };
        let j = serde_json::to_string(&ev).unwrap();
        assert!(j.contains(r#""kind":"recorded""#));
        let back: HistoryEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(back, ev);
    }
}
