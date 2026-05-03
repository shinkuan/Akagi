//! mjai JSONL protocol types.
//!
//! See `reference/reference_mjai.md` for the 4-player spec and
//! `reference/reference_mjai_3p.md` for the 3-player (sanma) variant.
//! `reference/Mortal/libriichi/src/mjai/event.rs` is a richer reference
//! impl (typed `Tile`, bounded actor, augmentation, metadata).
//!
//! This impl keeps tiles as `String` so the bridge can stay decoupled from any
//! tile-encoding library. JSON output puts `"type"` first because
//! `#[serde(tag = "type")]` (internally-tagged enum) emits the tag field first.
//!
//! Player-count-shaped fields (`names`, `scores`, `tehais`, `deltas`) use
//! `Vec<T>` of native length (3 for sanma, 4 for yonma). `StartGame` /
//! `StartKyoku` carry a `num_players` field with serde default = 4 so older
//! 4-player log lines still parse unchanged.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// mjai tile string. Examples: `"1m"`, `"5mr"` (red 5), `"E"`, `"P"`, `"?"`.
pub type Tile = String;

/// Seat index, 0..=3 (4p) or 0..=2 (3p).
pub type Actor = u8;

/// Default `num_players` when absent from the wire — 4p, the historical
/// behaviour. New 3p emitters always set the field explicitly.
fn default_num_players() -> u8 {
    4
}

/// One mjai event. Serialized as `{"type":"<snake_case>", ...}` with `type` first.
///
/// Mirrors the 15 event types in `reference/reference_mjai.md` plus the
/// 3p-only `Kita` variant from `reference/reference_mjai_3p.md`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MjaiEvent {
    StartGame {
        names: Vec<String>,
        kyoku_first: Option<u8>,
        aka_flag: Option<bool>,
        /// Bot's own seat (0..=3 for 4p, 0..=2 for 3p). Mjai.app convention:
        /// when this start_game is the *bot's view*, `id` tells the bot which
        /// seat it plays. Omitted on neutral/server-side logs.
        id: Option<Actor>,
        /// 3 (sanma) or 4 (yonma). Default 4 for backward-compatible
        /// deserialization of pre-3p log lines.
        #[serde(default = "default_num_players")]
        num_players: u8,
    },
    StartKyoku {
        bakaze: Tile,
        dora_marker: Tile,
        /// 1..=4
        kyoku: u8,
        honba: u8,
        kyotaku: u8,
        oya: Actor,
        scores: Vec<i32>,
        /// Per-seat 13-tile starting hand. Outer length = `num_players`.
        tehais: Vec<Vec<Tile>>,
        /// 3 (sanma) or 4 (yonma). Default 4 for backward-compatible
        /// deserialization of pre-3p log lines.
        #[serde(default = "default_num_players")]
        num_players: u8,
    },

    Tsumo {
        actor: Actor,
        pai: Tile,
    },
    Dahai {
        actor: Actor,
        pai: Tile,
        tsumogiri: bool,
    },

    Chi {
        actor: Actor,
        target: Actor,
        pai: Tile,
        consumed: [Tile; 2],
    },
    Pon {
        actor: Actor,
        target: Actor,
        pai: Tile,
        consumed: [Tile; 2],
    },
    Daiminkan {
        actor: Actor,
        target: Actor,
        pai: Tile,
        consumed: [Tile; 3],
    },
    Kakan {
        actor: Actor,
        pai: Tile,
        consumed: [Tile; 3],
    },
    Ankan {
        actor: Actor,
        consumed: [Tile; 4],
    },
    Dora {
        dora_marker: Tile,
    },

    Reach {
        actor: Actor,
        /// Non-spec extension: when set, names the tile the bot would
        /// discard if the riichi declaration is accepted. Bots that
        /// peek the post-reach dahai can populate this so the HUD can
        /// surface the predicted riichi tile up-front (Majsoul fuses
        /// declaring + discarding into one click). Bridge-emitted reach
        /// events leave it None.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pai: Option<Tile>,
    },
    ReachAccepted {
        actor: Actor,
    },

    Hora {
        actor: Actor,
        target: Actor,
        deltas: Option<Vec<i32>>,
        ura_markers: Option<Vec<Tile>>,
    },
    Ryukyoku {
        deltas: Option<Vec<i32>>,
    },

    /// 3-player only — 北抜き (BaBei / nukidora). Player sets aside a North
    /// tile from hand and draws a rinshan replacement. `pai` is `"N"` when
    /// emitted natively but kept optional so consumers that strip the
    /// (redundant) tile field still parse.
    Kita {
        actor: Actor,
        pai: Option<Tile>,
    },

    EndKyoku,
    EndGame,

    /// Non-spec: bot's "no action this turn" reply.
    ///
    /// Not part of the 15 mjai protocol events and never produced by the
    /// Majsoul bridge. Only emitted by a [`crate::bot::BotRunner`] when the
    /// bot has no decision to make. Kept in this enum so bot replies
    /// round-trip through the same type as bridge events.
    None,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{self, Value};

    /// Round-trip every event variant through JSON, asserting `type` is the
    /// first key in the serialized output and the value matches the spec.
    #[test]
    fn json_consistency_and_type_first() {
        let lines = [
            // 4p start_game with explicit num_players=4 round-trips identically.
            r#"{"type":"start_game","names":["NIKUYA","ひぐち","龍10","cxytml"],"kyoku_first":0,"aka_flag":true,"id":2,"num_players":4}"#,
            r#"{"type":"start_kyoku","bakaze":"E","dora_marker":"2m","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["5mr","9m","3p","1s","4s","4s","5sr","6s","6s","7s","E","E","P"],["4m","4m","6m","7m","8m","3p","5p","5pr","2s","9s","S","S","C"],["1m","4p","4p","5p","7p","8p","2s","3s","3s","5s","7s","7s","F"],["1m","1m","4m","5m","1p","2p","2s","6s","8s","W","N","F","C"]],"num_players":4}"#,
            r#"{"type":"tsumo","actor":0,"pai":"C"}"#,
            r#"{"type":"dahai","actor":0,"pai":"9m","tsumogiri":false}"#,
            r#"{"type":"chi","actor":3,"target":2,"pai":"3m","consumed":["4m","5mr"]}"#,
            r#"{"type":"pon","actor":1,"target":0,"pai":"4m","consumed":["4m","4m"]}"#,
            r#"{"type":"kakan","actor":0,"pai":"6m","consumed":["6m","6m","6m"]}"#,
            r#"{"type":"ankan","actor":0,"consumed":["F","F","F","F"]}"#,
            r#"{"type":"daiminkan","actor":0,"target":2,"pai":"5m","consumed":["5m","5m","5mr"]}"#,
            r#"{"type":"dora","dora_marker":"3s"}"#,
            r#"{"type":"reach","actor":0}"#,
            r#"{"type":"reach_accepted","actor":0}"#,
            r#"{"type":"hora","actor":2,"target":3,"deltas":[0,0,2300,-1300],"ura_markers":["C"]}"#,
            r#"{"type":"hora","actor":2,"target":3}"#,
            r#"{"type":"ryukyoku","deltas":[0,1500,0,-1500]}"#,
            r#"{"type":"ryukyoku"}"#,
            r#"{"type":"kita","actor":1,"pai":"N"}"#,
            r#"{"type":"kita","actor":2}"#,
            r#"{"type":"end_kyoku"}"#,
            r#"{"type":"end_game"}"#,
            r#"{"type":"none"}"#,
        ];

        for line in lines {
            let event: MjaiEvent = serde_json::from_str(line).expect("deserialize");
            let out = serde_json::to_string(&event).expect("serialize");

            assert!(
                out.starts_with(r#"{"type":""#),
                "type must be first key, got: {out}"
            );

            let expected: Value = serde_json::from_str(line).unwrap();
            let actual: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(expected, actual, "round-trip mismatch for {line}");
        }
    }

    /// 3-player wire format: length-3 names/scores/tehais/deltas, num_players=3,
    /// no padding. Mirrors the reference doc.
    #[test]
    fn three_player_round_trips_native_length() {
        let lines = [
            r#"{"type":"start_game","names":["P1","P2","P3"],"kyoku_first":0,"aka_flag":true,"num_players":3}"#,
            r#"{"type":"start_kyoku","bakaze":"E","dora_marker":"1m","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[35000,35000,35000],"tehais":[["1m","9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","E","E"],["1m","1m","2p","3p","4p","5p","6p","7p","8p","9p","S","S","C"],["9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","W","W","F"]],"num_players":3}"#,
            r#"{"type":"hora","actor":1,"target":0,"deltas":[-2000,2000,0]}"#,
            r#"{"type":"ryukyoku","deltas":[1000,-500,-500]}"#,
        ];
        for line in lines {
            let ev: MjaiEvent = serde_json::from_str(line).expect("deserialize 3p");
            let out = serde_json::to_string(&ev).expect("serialize 3p");
            let expected: Value = serde_json::from_str(line).unwrap();
            let actual: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(expected, actual, "3p round-trip mismatch: {line}");
        }
    }

    /// Reach is wire-compatible with bare `{"type":"reach","actor":N}` (the
    /// shape every bridge emits) but accepts an optional `pai` field that
    /// the Mortal bot wrapper populates with the speculated riichi-discard
    /// tile so the HUD can show it up-front. Bridge-shaped reach must
    /// round-trip without inventing a `pai` key.
    #[test]
    fn reach_pai_is_optional_and_round_trips() {
        // Bridge form: no pai. Deserializes to None, serializes back without pai.
        let plain = r#"{"type":"reach","actor":0}"#;
        let ev: MjaiEvent = serde_json::from_str(plain).unwrap();
        match &ev {
            MjaiEvent::Reach { actor, pai } => {
                assert_eq!(*actor, 0);
                assert!(pai.is_none(), "bridge reach must have pai=None");
            }
            other => panic!("expected Reach, got {other:?}"),
        }
        let out = serde_json::to_string(&ev).unwrap();
        assert_eq!(out, plain, "None pai must not be serialized");

        // Bot form: pai populated by post-reach speculation.
        let with_pai = r#"{"type":"reach","actor":1,"pai":"5p"}"#;
        let ev: MjaiEvent = serde_json::from_str(with_pai).unwrap();
        match &ev {
            MjaiEvent::Reach { actor, pai } => {
                assert_eq!(*actor, 1);
                assert_eq!(pai.as_deref(), Some("5p"));
            }
            other => panic!("expected Reach, got {other:?}"),
        }
        let out = serde_json::to_string(&ev).unwrap();
        assert_eq!(out, with_pai);

        // Explicit null pai (a buggy bot might emit this) deserializes
        // to None and re-serializes without the key, identical to the
        // bridge form.
        let null_pai = r#"{"type":"reach","actor":2,"pai":null}"#;
        let ev: MjaiEvent = serde_json::from_str(null_pai).unwrap();
        match &ev {
            MjaiEvent::Reach { actor, pai } => {
                assert_eq!(*actor, 2);
                assert!(pai.is_none(), "explicit null must deserialize to None");
            }
            other => panic!("expected Reach, got {other:?}"),
        }
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"type":"reach","actor":2}"#,
            "null pai must serialize as omitted",
        );

        // Empty-string pai is currently accepted as Some("") since
        // Tile is an unvalidated alias for String. Frontend treats
        // empty as falsy and falls back to glyph-only rendering, so
        // round-trip preserves it without crashing the schema layer.
        let empty_pai = r#"{"type":"reach","actor":3,"pai":""}"#;
        let ev: MjaiEvent = serde_json::from_str(empty_pai).unwrap();
        match &ev {
            MjaiEvent::Reach { actor, pai } => {
                assert_eq!(*actor, 3);
                assert_eq!(pai.as_deref(), Some(""));
            }
            other => panic!("expected Reach, got {other:?}"),
        }
        assert_eq!(serde_json::to_string(&ev).unwrap(), empty_pai);
    }

    /// Backward compat: 4p log lines from before `num_players` was added must
    /// deserialize unchanged. The defaulted `num_players` is omitted from the
    /// re-serialized output unless explicit, so the JSON can be re-emitted
    /// either way — but the in-memory value is 4.
    #[test]
    fn legacy_four_player_lines_default_num_players_to_four() {
        let line = r#"{"type":"start_game","names":["a","b","c","d"],"kyoku_first":0,"aka_flag":true,"id":2}"#;
        let ev: MjaiEvent = serde_json::from_str(line).unwrap();
        match ev {
            MjaiEvent::StartGame {
                num_players, names, ..
            } => {
                assert_eq!(num_players, 4, "missing num_players → defaults to 4");
                assert_eq!(names.len(), 4);
            }
            other => panic!("expected StartGame, got {other:?}"),
        }

        let line = r#"{"type":"start_kyoku","bakaze":"E","dora_marker":"2m","kyoku":1,"honba":0,"kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],"tehais":[["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"],["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"],["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"],["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"]]}"#;
        let ev: MjaiEvent = serde_json::from_str(line).unwrap();
        match ev {
            MjaiEvent::StartKyoku {
                num_players,
                scores,
                tehais,
                ..
            } => {
                assert_eq!(num_players, 4);
                assert_eq!(scores.len(), 4);
                assert_eq!(tehais.len(), 4);
                for hand in &tehais {
                    assert_eq!(hand.len(), 13);
                }
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }
}
