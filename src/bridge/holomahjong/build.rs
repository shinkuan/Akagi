//! Our decisions as the client's own hub calls.
//!
//! Each is a fire-and-forget StreamingHub call, `[method, body]`, built the
//! way the client builds it:
//!
//! | Decision | Call | Body |
//! |---|---|---|
//! | discard, riichi | `SelectDapai` | `[seat, tile, riichi_type]` |
//! | chi, pon, open kan | `SelectFulou` | `[nil, meld]` |
//! | closed or added kan | `SelectAnGangAddGang` | `[nil, meld]` |
//! | tsumo, ron | `SelectHule` | `[nil, "-"]` |
//! | kita | `SelectBabei` | `[seat, tile]` |
//! | nine-terminal abort | `SelectDaopai` | `["-"]` |
//! | pass | `SelectActionSkip` | `[]` |
//!
//! A discard of the tile just drawn carries `_` (`"m3_"`), as the server's
//! list of legal discards does. Melds and kita tiles are never composed
//! here: the window lists the exact strings the server will accept, and the
//! one matching the decision is sent back as it came. A decision the window
//! does not allow is refused rather than sent.

use super::{hub, tile};
use crate::autoplay::holomahjong::link::{Window, WindowKind};
use crate::schema::mjai::Tile;
use crate::schema::MjaiEvent;
use rmpv::Value;
use tile::Meld;

/// `LizhiType::Normal`; the server works out a double riichi itself.
const RIICHI: i64 = 1;

/// The hub message for `action` in `window`. `seat` is our wire seat and
/// `hand` our concealed tiles in mjai notation (the draw included).
pub fn encode(
    action: &MjaiEvent,
    window: &Window,
    seat: u8,
    hand: &[Tile],
) -> Result<Vec<u8>, String> {
    let turn = match &window.kind {
        WindowKind::Turn { drawn } => Some(drawn.as_deref()),
        WindowKind::Claim => None,
    };
    let seat = Value::from(seat);
    let (method, body) = match (action, turn) {
        (MjaiEvent::Dahai { pai, tsumogiri, .. }, Some(drawn)) => (
            hub::SELECT_DAPAI,
            vec![
                seat,
                discard(pai, *tsumogiri, drawn, hand)?.into(),
                0.into(),
            ],
        ),
        (MjaiEvent::Reach { pai: Some(pai), .. }, Some(drawn)) => (
            hub::SELECT_DAPAI,
            vec![
                seat,
                discard(pai, false, drawn, hand)?.into(),
                RIICHI.into(),
            ],
        ),
        (MjaiEvent::Reach { pai: None, .. }, _) => {
            return Err("riichi without its discard".into());
        }
        (MjaiEvent::Chi { pai, consumed, .. }, None) => (
            hub::SELECT_FULOU,
            vec![Value::Nil, offer(&window.chi, "chi", pai, consumed)?],
        ),
        (MjaiEvent::Pon { pai, consumed, .. }, None) => (
            hub::SELECT_FULOU,
            vec![Value::Nil, offer(&window.pon, "pon", pai, consumed)?],
        ),
        (MjaiEvent::Daiminkan { pai, consumed, .. }, None) => (
            hub::SELECT_FULOU,
            vec![Value::Nil, offer(&window.kan, "kan", pai, consumed)?],
        ),
        (MjaiEvent::Ankan { consumed, .. }, Some(_)) => (
            hub::SELECT_AN_GANG_ADD_GANG,
            vec![
                Value::Nil,
                offer(&window.kan, "kan", &consumed[0], &consumed[1..])?,
            ],
        ),
        (MjaiEvent::Kakan { pai, consumed, .. }, Some(_)) => (
            hub::SELECT_AN_GANG_ADD_GANG,
            vec![Value::Nil, offer(&window.kan, "kan", pai, consumed)?],
        ),
        (MjaiEvent::Hora { .. }, _) if window.can_win => {
            (hub::SELECT_HULE, vec![Value::Nil, "-".into()])
        }
        (MjaiEvent::Hora { .. }, _) => return Err("the server offers no win".into()),
        (MjaiEvent::Kita { .. }, Some(_)) => match window.kita.first() {
            Some(k) => (hub::SELECT_BABEI, vec![seat, k.as_str().into()]),
            None => return Err("the server offers no kita".into()),
        },
        (MjaiEvent::Ryukyoku { deltas: None }, Some(_)) => (hub::SELECT_DAOPAI, vec!["-".into()]),
        (MjaiEvent::None, None) => (hub::SELECT_ACTION_SKIP, Vec::new()),
        (other, _) => {
            return Err(format!(
                "{other:?} does not answer a {} window",
                if turn.is_some() { "turn" } else { "claim" }
            ));
        }
    };
    let message = Value::Array(vec![Value::from(method), Value::Array(body)]);
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, &message).map_err(|e| e.to_string())?;
    Ok(out)
}

/// The discard string: `_` when it is the drawn tile and either the bot says
/// so or the hand has no other copy (the server only lists `"m3"` for a copy
/// held before the draw).
fn discard(
    pai: &str,
    tsumogiri: bool,
    drawn: Option<&str>,
    hand: &[Tile],
) -> Result<String, String> {
    let wire = tile::to_wire(pai).ok_or_else(|| format!("unknown tile {pai}"))?;
    let copies = hand.iter().filter(|t| *t == pai).count();
    if drawn == Some(pai) && (tsumogiri || copies <= 1) {
        Ok(wire + "_")
    } else {
        Ok(wire)
    }
}

/// The offered meld made of `pai` and `consumed`, whichever its kind.
fn offer(offers: &[String], kind: &str, pai: &str, consumed: &[Tile]) -> Result<Value, String> {
    let mut want: Vec<&str> = consumed.iter().map(String::as_str).chain([pai]).collect();
    want.sort_unstable();
    offers
        .iter()
        .find(|o| {
            let Some(m) = tile::meld(o) else { return false };
            let (called, rest): (&str, Vec<&str>) = match &m {
                Meld::Chi { pai, consumed, .. } | Meld::Pon { pai, consumed, .. } => {
                    (pai, consumed.iter().map(String::as_str).collect())
                }
                Meld::Daiminkan { pai, consumed, .. } | Meld::Kakan { pai, consumed } => {
                    (pai, consumed.iter().map(String::as_str).collect())
                }
                Meld::Ankan { consumed } => (
                    &consumed[0],
                    consumed[1..].iter().map(String::as_str).collect(),
                ),
            };
            let same_kind = matches!(
                (&m, kind),
                (Meld::Chi { .. }, "chi")
                    | (Meld::Pon { .. }, "pon")
                    | (
                        Meld::Daiminkan { .. } | Meld::Kakan { .. } | Meld::Ankan { .. },
                        "kan"
                    )
            );
            let mut got: Vec<&str> = rest.into_iter().chain([called]).collect();
            got.sort_unstable();
            same_kind && got == want
        })
        .map(|o| Value::from(o.as_str()))
        .ok_or_else(|| {
            format!("the server offers no {kind} of {pai} with {consumed:?} (offers: {offers:?})")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn window(kind: WindowKind) -> Window {
        Window {
            opened_at: Instant::now(),
            kind,
            chi: Vec::new(),
            pon: Vec::new(),
            kan: Vec::new(),
            kita: Vec::new(),
            can_win: false,
            base_ms: 20000,
            bank_ms: 30000,
            total_ms: 50000,
            answered: false,
        }
    }

    fn turn(drawn: &str) -> Window {
        window(WindowKind::Turn {
            drawn: Some(drawn.into()),
        })
    }

    fn tiles(s: &[&str]) -> Vec<Tile> {
        s.iter().map(|t| t.to_string()).collect()
    }

    fn decoded(bytes: &[u8]) -> hub::HubMessage {
        hub::decode(bytes).expect("a hub message")
    }

    /// Byte for byte what the client sends: MessagePack-CSharp writes the
    /// method id as a uint32 and the body as a positional array.
    #[test]
    fn a_discard_is_the_clients_select_dapai() {
        let ev = MjaiEvent::Dahai {
            actor: 1,
            pai: "6p".into(),
            tsumogiri: false,
        };
        let bytes = encode(&ev, &turn("1m"), 2, &tiles(&["6p", "1m"])).unwrap();
        let mut want = vec![0x92, 0xce];
        want.extend_from_slice(&1_957_121_736u32.to_be_bytes());
        want.extend_from_slice(&[0x93, 0x02, 0xa2, b'p', b'6', 0x00]);
        assert_eq!(bytes, want);
    }

    #[test]
    fn signed_method_ids_and_empty_bodies_match_the_client() {
        let claim = window(WindowKind::Claim);
        let mut want = vec![0x92, 0xd2];
        want.extend_from_slice(&(hub::SELECT_ACTION_SKIP as i32).to_be_bytes());
        want.push(0x90);
        assert_eq!(encode(&MjaiEvent::None, &claim, 0, &[]).unwrap(), want);

        let mut t = turn("1m");
        t.can_win = true;
        let tsumo = MjaiEvent::Hora {
            actor: 0,
            target: 0,
            deltas: None,
            ura_markers: None,
        };
        let mut want = vec![0x92, 0xd2];
        want.extend_from_slice(&(hub::SELECT_HULE as i32).to_be_bytes());
        want.extend_from_slice(&[0x92, 0xc0, 0xa1, b'-']);
        assert_eq!(encode(&tsumo, &t, 0, &[]).unwrap(), want);
    }

    #[test]
    fn the_drawn_tile_is_marked_when_it_is_the_one_thrown() {
        let hand = tiles(&["3s", "3s", "5mr", "7z"]);
        let dahai = |pai: &str, tsumogiri| MjaiEvent::Dahai {
            actor: 0,
            pai: pai.into(),
            tsumogiri,
        };
        let body = |ev: &MjaiEvent, drawn: &str| {
            decoded(&encode(ev, &turn(drawn), 0, &hand).unwrap()).body
        };
        assert_eq!(body(&dahai("3s", true), "3s")[1], "s3_");
        assert_eq!(
            body(&dahai("3s", false), "3s")[1],
            "s3",
            "the copy held from before"
        );
        // The only red five is the drawn one: the server lists "m0_" alone.
        assert_eq!(body(&dahai("5mr", false), "5mr")[1], "m0_");
        assert_eq!(
            body(&dahai("3s", true), "5mr")[1],
            "s3",
            "not the drawn tile"
        );
    }

    #[test]
    fn riichi_is_a_discard_with_its_type() {
        let ev = MjaiEvent::Reach {
            actor: 0,
            pai: Some("E".into()),
        };
        let msg = decoded(&encode(&ev, &turn("E"), 1, &tiles(&["E", "9p"])).unwrap());
        assert_eq!(msg.method, hub::SELECT_DAPAI);
        assert_eq!(msg.body, serde_json::json!([1, "z1_", 1]));

        let bare = MjaiEvent::Reach {
            actor: 0,
            pai: None,
        };
        assert!(encode(&bare, &turn("E"), 1, &[]).is_err());
    }

    #[test]
    fn calls_send_back_the_offered_meld() {
        let mut w = window(WindowKind::Claim);
        w.chi = vec!["p56-7".into(), "p6-78".into(), "p406-".into()];
        w.pon = vec!["p666+".into()];
        let chi = |consumed: [&str; 2]| MjaiEvent::Chi {
            actor: 0,
            target: 3,
            pai: "6p".into(),
            consumed: [consumed[0].into(), consumed[1].into()],
        };
        let sent = |ev: &MjaiEvent| decoded(&encode(ev, &w, 0, &[]).unwrap());
        let msg = sent(&chi(["7p", "8p"]));
        assert_eq!(msg.method, hub::SELECT_FULOU);
        assert_eq!(msg.body, serde_json::json!([null, "p6-78"]));
        assert_eq!(sent(&chi(["5pr", "4p"])).body[1], "p406-");
        assert_eq!(sent(&chi(["5p", "7p"])).body[1], "p56-7");

        let pon = MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: "6p".into(),
            consumed: ["6p".into(), "6p".into()],
        };
        assert_eq!(sent(&pon).body[1], "p666+");

        let not_offered = MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: "6p".into(),
            consumed: ["6p".into(), "6p".into()],
        };
        w.pon.clear();
        assert!(
            encode(&not_offered, &w, 0, &[]).is_err(),
            "never a call the server did not list"
        );
    }

    #[test]
    fn kans_go_out_by_the_window_they_answer() {
        let mut claim = window(WindowKind::Claim);
        claim.kan = vec!["s9999+".into()];
        let open = MjaiEvent::Daiminkan {
            actor: 0,
            target: 1,
            pai: "9s".into(),
            consumed: ["9s".into(), "9s".into(), "9s".into()],
        };
        let msg = decoded(&encode(&open, &claim, 0, &[]).unwrap());
        assert_eq!(msg.method, hub::SELECT_FULOU);
        assert_eq!(msg.body[1], "s9999+");

        let mut t = turn("5m");
        t.kan = vec!["m5550".into(), "z111=1".into()];
        let closed = MjaiEvent::Ankan {
            actor: 0,
            consumed: ["5m".into(), "5m".into(), "5m".into(), "5mr".into()],
        };
        let msg = decoded(&encode(&closed, &t, 0, &[]).unwrap());
        assert_eq!(msg.method, hub::SELECT_AN_GANG_ADD_GANG);
        assert_eq!(msg.body, serde_json::json!([null, "m5550"]));
        let added = MjaiEvent::Kakan {
            actor: 0,
            pai: "E".into(),
            consumed: ["E".into(), "E".into(), "E".into()],
        };
        assert_eq!(
            decoded(&encode(&added, &t, 0, &[]).unwrap()).body[1],
            "z111=1"
        );
        assert!(
            encode(&open, &t, 0, &[]).is_err(),
            "an open kan answers a claim"
        );
    }

    #[test]
    fn wins_kita_aborts_and_passes() {
        let mut t = turn("N");
        t.kita = vec!["z4_".into()];
        let kita = MjaiEvent::Kita {
            actor: 0,
            pai: Some("N".into()),
        };
        assert_eq!(
            decoded(&encode(&kita, &t, 2, &[]).unwrap()).body,
            serde_json::json!([2, "z4_"])
        );
        let tsumo = MjaiEvent::Hora {
            actor: 0,
            target: 0,
            deltas: None,
            ura_markers: None,
        };
        assert!(encode(&tsumo, &t, 2, &[]).is_err(), "no win on offer");
        t.can_win = true;
        let msg = decoded(&encode(&tsumo, &t, 2, &[]).unwrap());
        assert_eq!(
            (msg.method, msg.body),
            (hub::SELECT_HULE, serde_json::json!([null, "-"]))
        );

        let abort = MjaiEvent::Ryukyoku { deltas: None };
        let msg = decoded(&encode(&abort, &t, 2, &[]).unwrap());
        assert_eq!(
            (msg.method, msg.body),
            (hub::SELECT_DAOPAI, serde_json::json!(["-"]))
        );

        let claim = window(WindowKind::Claim);
        let msg = decoded(&encode(&MjaiEvent::None, &claim, 2, &[]).unwrap());
        assert_eq!(
            (msg.method, msg.body),
            (hub::SELECT_ACTION_SKIP, serde_json::json!([]))
        );
        assert!(
            encode(&MjaiEvent::None, &t, 2, &[]).is_err(),
            "a turn is never passed"
        );
    }
}
