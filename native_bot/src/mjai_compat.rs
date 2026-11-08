//! Compatibility fixes applied to incoming mjai events before they reach a
//! riichienv-core game state.
//!
//! Shared by the offline extractor ([`crate::replay`]) and the live engine
//! ([`crate::engine`]), so a log that replays cleanly can also be fed to the
//! engine without panicking.
//!
//! There are two fixups, and they run at different stages:
//!
//! - The nukidora rename happens on the parsed JSON **value**, before it becomes
//!   an event. It has to happen somewhere: riichienv's `MjaiEvent` knows only
//!   `kita` and carries a `#[serde(other)]` catch-all, so a Tenhou sanma log's
//!   `nukidora` would deserialize into `MjaiEvent::Other` and be dropped on the
//!   floor — no error, just a nukidora the engine never sees.
//! - [`sanitize_3p`] truncates 4-seat arrays **after** parsing.
//!
//! [`parse_line`] is the single door that applies both in the right order.

use riichienv_core::replay::MjaiEvent;

/// Parse one line of an mjai JSONL log into a riichienv event, applying both
/// compatibility fixups. `None` for blank lines, malformed JSON, and event types
/// riichienv doesn't model (`MjaiEvent::Other`) — all of which callers skip.
///
/// This is the only correct way to turn raw log text into an event: parsing with
/// `serde_json` directly loses sanma nukidora and can panic a `GameState3P` on a
/// 4-seat `start_kyoku` (see [`sanitize_3p`]).
pub fn parse_line(line: &str, num_players: u8) -> Option<MjaiEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // Tenhou sanma logs spell nukidora `nukidora`; riichienv's `MjaiEvent` only
    // knows `kita`. The rename is decided on the parsed value rather than on the
    // raw text: JSON allows any whitespace around `:`, and a missed rename is
    // not an error — the event folds into `MjaiEvent::Other` and evaporates,
    // which is indistinguishable from a blank line to every caller. Scoping it
    // to the `type` field also leaves a player named "nukidora" alone. Harmless
    // on 4p logs, which contain no nukidora at all.
    let mut value: serde_json::Value = serde_json::from_str(line).ok()?;
    if let Some(obj) = value.as_object_mut() {
        if obj.get("type").and_then(serde_json::Value::as_str) == Some("nukidora") {
            obj.insert("type".to_owned(), "kita".into());
        }
    }
    let mut ev: MjaiEvent = serde_json::from_value(value).ok()?;
    if matches!(ev, MjaiEvent::Other) {
        return None;
    }
    if num_players == 3 {
        sanitize_3p(&mut ev);
    }
    Some(ev)
}

/// Tenhou sanma logs use a 4-seat layout: `start_kyoku`/`hora`/`ryukyoku`
/// carry 4-element `scores`/`tehais`/`delta` (the 4th is a dummy dead seat),
/// which a 3-seat `GameState3P` would index out of bounds. Truncate them to 3.
///
/// A no-op for 4-player events, and for 3-element arrays that are already the
/// right shape — so it is safe to run over every event of a sanma stream.
pub fn sanitize_3p(ev: &mut MjaiEvent) {
    match ev {
        MjaiEvent::StartKyoku { scores, tehais, .. } => {
            scores.truncate(3);
            tehais.truncate(3);
        }
        MjaiEvent::Hora { delta, scores, .. } => {
            if let Some(d) = delta {
                d.truncate(3);
            }
            if let Some(s) = scores {
                s.truncate(3);
            }
        }
        MjaiEvent::Ryukyoku {
            delta,
            scores,
            tehais,
            ..
        } => {
            if let Some(d) = delta {
                d.truncate(3);
            }
            if let Some(s) = scores {
                s.truncate(3);
            }
            if let Some(t) = tehais {
                t.truncate(3);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(line: &str) -> MjaiEvent {
        serde_json::from_str(line).expect("valid mjai event")
    }

    /// 13 tiles, as a JSON array literal.
    fn hand13() -> String {
        r#"["1p","2p","3p","4p","5p","6p","7p","8p","9p","1s","2s","3s","4s"]"#.to_string()
    }

    #[test]
    fn start_kyoku_truncated_to_three_seats() {
        let h = hand13();
        let mut e = ev(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],
                 "tehais":[{h},{h},{h},{h}]}}"#
        ));
        sanitize_3p(&mut e);
        match e {
            MjaiEvent::StartKyoku { scores, tehais, .. } => {
                assert_eq!(scores.len(), 3);
                assert_eq!(tehais.len(), 3);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn already_three_seats_is_untouched() {
        let h = hand13();
        let mut e = ev(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[35000,35000,35000],"tehais":[{h},{h},{h}]}}"#
        ));
        sanitize_3p(&mut e);
        match e {
            MjaiEvent::StartKyoku { scores, tehais, .. } => {
                assert_eq!(scores.len(), 3);
                assert_eq!(tehais.len(), 3);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn hora_and_ryukyoku_arrays_truncated() {
        let mut e = ev(r#"{"type":"hora","actor":0,"target":1,"pai":"1m",
                "scores":[35000,35000,35000,0],"deltas":[1000,-1000,0,0]}"#);
        sanitize_3p(&mut e);
        match e {
            MjaiEvent::Hora { scores, delta, .. } => {
                assert_eq!(scores.unwrap().len(), 3);
                assert_eq!(delta.unwrap().len(), 3);
            }
            other => panic!("unexpected: {other:?}"),
        }

        let h = hand13();
        let mut e = ev(&format!(
            r#"{{"type":"ryukyoku","reason":"fanpai","tehais":[{h},{h},{h},{h}],
                 "scores":[35000,35000,35000,0],"deltas":[0,0,0,0]}}"#
        ));
        sanitize_3p(&mut e);
        match e {
            MjaiEvent::Ryukyoku {
                scores,
                delta,
                tehais,
                ..
            } => {
                assert_eq!(scores.unwrap().len(), 3);
                assert_eq!(delta.unwrap().len(), 3);
                assert_eq!(tehais.unwrap().len(), 3);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn other_events_pass_through() {
        let mut e = ev(r#"{"type":"dahai","actor":1,"pai":"1m","tsumogiri":false}"#);
        sanitize_3p(&mut e);
        assert!(matches!(e, MjaiEvent::Dahai { actor: 1, .. }));
    }

    /// Regression: Tenhou sanma logs spell nukidora `nukidora`, and riichienv's
    /// `MjaiEvent` has a `#[serde(other)]` catch-all — so parsing one straight
    /// through serde yields `Other` and the kita is silently dropped rather than
    /// erroring. The rename must therefore happen before serde sees the line.
    #[test]
    fn nukidora_is_renamed_to_kita_before_parsing() {
        let raw = r#"{"type":"nukidora","actor":2}"#;

        // What happens without the fixup: the event evaporates.
        let naive: MjaiEvent = serde_json::from_str(raw).unwrap();
        assert!(
            matches!(naive, MjaiEvent::Other),
            "serde folds an unknown type into Other, it does not fail"
        );

        assert!(matches!(
            parse_line(raw, 3).expect("kita survives"),
            MjaiEvent::Kita { actor: 2 }
        ));
        // Whitespace variant of the same field.
        assert!(matches!(
            parse_line(r#"{"type": "nukidora", "actor": 0}"#, 3).expect("kita survives"),
            MjaiEvent::Kita { actor: 0 }
        ));
    }

    /// The rename is scoped to the `type` field: a player named "nukidora"
    /// must not be rewritten.
    #[test]
    fn the_rename_only_touches_the_type_field() {
        let e = parse_line(
            r#"{"type":"start_game","names":["nukidora","b","c",""]}"#,
            4,
        )
        .expect("start_game parses");
        match e {
            MjaiEvent::StartGame { names, .. } => {
                assert_eq!(names.expect("names present")[0], "nukidora");
            }
            _ => panic!("expected start_game"),
        }
    }

    /// Regression: the rename used to be a byte-wise rewrite of two exact
    /// spellings (`"type":"nukidora"` and `"type": "nukidora"`), so any log that
    /// formatted the object differently — spaces around the colon, a tab, a
    /// pretty-printed line — kept `nukidora`, and the event then folded into
    /// `MjaiEvent::Other` and was dropped exactly like a blank line.
    #[test]
    fn nukidora_survives_any_spacing() {
        let lines = [
            r#"{"type":"nukidora","actor":1}"#,
            r#"{"type": "nukidora", "actor": 1}"#,
            r#"{ "type" : "nukidora", "actor" : 1 }"#,
            r#"{"type"  :  "nukidora",  "actor":  1}"#,
            "{\"type\"\t:\t\"nukidora\",\t\"actor\":1}",
            "{\n  \"type\": \"nukidora\",\n  \"actor\": 1\n}",
        ];
        for line in lines {
            assert!(
                matches!(parse_line(line, 3), Some(MjaiEvent::Kita { actor: 1 })),
                "nukidora lost for {line:?}"
            );
        }
    }

    /// `parse_line` also applies the sanma truncation, and skips the lines every
    /// caller skips (blank, malformed, unmodelled types).
    #[test]
    fn parse_line_sanitizes_and_skips() {
        let h = hand13();
        let e = parse_line(
            &format!(
                r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                     "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],
                     "tehais":[{h},{h},{h},{h}]}}"#
            ),
            3,
        )
        .expect("start_kyoku parses");
        match e {
            MjaiEvent::StartKyoku { scores, tehais, .. } => {
                assert_eq!(scores.len(), 3);
                assert_eq!(tehais.len(), 3);
            }
            other => panic!("unexpected: {other:?}"),
        }

        assert!(parse_line("   ", 4).is_none(), "blank");
        assert!(parse_line("not json", 4).is_none(), "malformed");
        assert!(
            parse_line(r#"{"type":"ryuukyoku_hack"}"#, 4).is_none(),
            "Other"
        );

        // 4p logs keep their four seats.
        let e = parse_line(
            &format!(
                r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                     "kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],
                     "tehais":[{h},{h},{h},{h}]}}"#
            ),
            4,
        )
        .expect("start_kyoku parses");
        match e {
            MjaiEvent::StartKyoku { scores, .. } => assert_eq!(scores.len(), 4),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
