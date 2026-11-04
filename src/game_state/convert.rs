//! Convert Akagi's `schema::MjaiEvent` into `riichienv_core::replay::MjaiEvent`.
//!
//! Both enums are `serde(tag = "type")` with the mjai protocol's
//! snake-case discriminants, so a JSON round-trip handles 14 of the 15
//! variants directly. Two cases need care:
//!
//! - **`StartGame.id`** — Akagi types it as `Option<u8>` (seat number);
//!   riichienv types it as `Option<String>`. We patch the JSON before
//!   deserialization.
//! - **`MjaiEvent::None`** — Akagi-only sentinel for a bot's "no action"
//!   reply. Not a protocol event; we return `Ok(None)` so the tracker
//!   skips it without producing an error.
//!
//! Choosing JSON-roundtrip over a hand-written `From` keeps this file
//! ~30 lines. Cost is one `serde_json::to_value` + one
//! `serde_json::from_value` per event — negligible against a hand of
//! mahjong (a few thousand events per game).

use crate::schema::MjaiEvent as AkagiEvent;
use anyhow::{Context, Result};
use riichienv_core::replay::MjaiEvent as RiEvent;
use serde_json::Value;

/// Returns `Ok(None)` for `MjaiEvent::None` (intentional skip), `Ok(Some(_))`
/// for any protocol event, `Err` for malformed input.
pub fn to_riichienv(ev: &AkagiEvent) -> Result<Option<RiEvent>> {
    if matches!(ev, AkagiEvent::None) {
        return Ok(None);
    }

    let mut v = serde_json::to_value(ev).context("serialize akagi mjai event")?;

    // Patch StartGame.id: Akagi `u8` → riichienv `String`.
    if v.get("type").and_then(Value::as_str) == Some("start_game") {
        if let Some(id) = v.get_mut("id") {
            if let Some(n) = id.as_u64() {
                *id = Value::String(n.to_string());
            }
        }
    }

    let ri: RiEvent = serde_json::from_value(v).context("deserialize as riichienv mjai event")?;
    Ok(Some(ri))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dahai(actor: u8) -> AkagiEvent {
        AkagiEvent::Dahai {
            actor,
            pai: "1m".into(),
            tsumogiri: false,
        }
    }

    #[test]
    fn none_returns_none() {
        assert!(to_riichienv(&AkagiEvent::None).unwrap().is_none());
    }

    #[test]
    fn dahai_round_trips() {
        let out = to_riichienv(&dahai(2)).unwrap().unwrap();
        match out {
            RiEvent::Dahai {
                actor,
                pai,
                tsumogiri,
            } => {
                assert_eq!(actor, 2);
                assert_eq!(pai, "1m");
                assert!(!tsumogiri);
            }
            other => panic!("expected Dahai, got {other:?}"),
        }
    }

    #[test]
    fn start_game_id_u8_becomes_string() {
        let ev = AkagiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(2),
            num_players: 4,
            game_meta: None,
        };
        let out = to_riichienv(&ev).unwrap().unwrap();
        match out {
            RiEvent::StartGame { id, .. } => {
                assert_eq!(id.as_deref(), Some("2"));
            }
            other => panic!("expected StartGame, got {other:?}"),
        }
    }

    #[test]
    fn start_game_no_id_works() {
        let ev = AkagiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: None,
            num_players: 4,
            game_meta: None,
        };
        let out = to_riichienv(&ev).unwrap().unwrap();
        assert!(matches!(out, RiEvent::StartGame { id: None, .. }));
    }

    #[test]
    fn daiminkan_maps_to_kan_via_alias() {
        // Akagi serializes the variant name (snake_case) → "daiminkan".
        // riichienv accepts that as an alias for its `Kan` variant.
        let ev = AkagiEvent::Daiminkan {
            actor: 0,
            target: 2,
            pai: "5m".into(),
            consumed: ["5m".into(), "5m".into(), "5mr".into()],
        };
        let out = to_riichienv(&ev).unwrap().unwrap();
        match out {
            RiEvent::Kan {
                actor,
                target,
                pai,
                consumed,
            } => {
                assert_eq!(actor, 0);
                assert_eq!(target, 2);
                assert_eq!(pai, "5m");
                assert_eq!(consumed, vec!["5m", "5m", "5mr"]);
            }
            other => panic!("expected Kan, got {other:?}"),
        }
    }

    #[test]
    fn start_kyoku_round_trips() {
        let one_hand: Vec<String> = (0..13).map(|i| format!("{}m", (i % 9) + 1)).collect();
        let ev = AkagiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![25000, 25000, 25000, 25000],
            tehais: vec![
                one_hand.clone(),
                one_hand.clone(),
                one_hand.clone(),
                one_hand,
            ],
            num_players: 4,
            catch_up: 0,
        };
        let out = to_riichienv(&ev).unwrap().unwrap();
        match out {
            RiEvent::StartKyoku {
                bakaze,
                kyoku,
                honba,
                kyoutaku,
                oya,
                scores,
                ..
            } => {
                assert_eq!(bakaze, "E");
                assert_eq!(kyoku, 1);
                assert_eq!(honba, 0);
                assert_eq!(kyoutaku, 0);
                assert_eq!(oya, 0);
                assert_eq!(scores, vec![25000, 25000, 25000, 25000]);
            }
            other => panic!("expected StartKyoku, got {other:?}"),
        }
    }

    #[test]
    fn end_game_works() {
        let out = to_riichienv(&AkagiEvent::end_game()).unwrap().unwrap();
        assert!(matches!(out, RiEvent::EndGame));
    }

    #[test]
    fn hora_with_deltas_round_trips() {
        let ev = AkagiEvent::Hora {
            actor: 2,
            target: 3,
            deltas: Some(vec![0, 0, 2300, -1300]),
            ura_markers: Some(vec!["C".into()]),
        };
        let out = to_riichienv(&ev).unwrap().unwrap();
        match out {
            RiEvent::Hora {
                actor,
                target,
                delta,
                uradora_markers,
                ..
            } => {
                assert_eq!(actor, 2);
                assert_eq!(target, 3);
                assert_eq!(delta, Some(vec![0, 0, 2300, -1300]));
                assert_eq!(uradora_markers, Some(vec!["C".into()]));
            }
            other => panic!("expected Hora, got {other:?}"),
        }
    }
}
