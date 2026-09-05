//! Regression for the Tenhou reconnect corruption reported in issue #280.
//!
//! When the Tenhou client drops and rejoins a game, the capture backend
//! opens a fresh WebSocket flow and therefore a fresh `TenhouBridge`. The
//! rejoin sequence carries the hand in progress as `<REINIT/>` and need not
//! repeat `<TAIKYOKU/>`, so the new bridge used to sit at the default seat 0
//! for the rest of the game while the (per-session) tracker kept the real
//! seat. Every opponent draw that mapped onto our seat arrived as a `?`,
//! which the riichi engine stores as tile 0 — a 1m — and the next `<INIT/>`
//! only repeated the mistake. This drives the bridge → tracker pipeline
//! through that exact sequence from a non-East seat.

use akagi::bridge::tenhou::TenhouBridge;
use akagi::bridge::{Bridge, Direction};
use akagi::game_state::tracker::GameTracker;
use akagi::schema::MjaiEvent;

/// Opening hand with no 1m, so a stray `?`-turned-1m stands out.
const HAND_E1: &str = "4,8,12,36,40,44,72,76,80,108,112,116,120";
/// Same for the second hand (tile 16 is the red 5m).
const HAND_E2: &str = "8,12,16,40,44,48,76,80,84,112,116,120,124";

fn feed(bridge: &mut TenhouBridge, tracker: &mut GameTracker, json: &str) -> Vec<MjaiEvent> {
    let events = bridge.parse(Direction::Down, json.as_bytes()).events;
    for ev in &events {
        tracker.handle(ev).expect("tracker accepts bridge output");
    }
    events
}

fn our_tehai(tracker: &GameTracker) -> Vec<String> {
    let snap = tracker.snapshot().expect("game in progress");
    let seat = snap.our_seat.expect("seat known") as usize;
    let mut tehai = snap.players[seat].tehai.clone();
    tehai.sort();
    tehai
}

fn sorted_mjai(indices: &str) -> Vec<String> {
    let mut v: Vec<String> = indices
        .split(',')
        .map(|t| akagi::bridge::tenhou::tile::tenhou_to_mjai_one(t.parse().unwrap()))
        .collect();
    v.sort();
    v
}

#[test]
fn tenhou_rejoin_without_taikyoku_keeps_our_seat_and_hand_intact() {
    let mut tracker = GameTracker::new();

    // ---- Flow 1: the game starts normally. We sit West (wire-abs 2). ----
    let mut flow1 = TenhouBridge::new(None, None);
    feed(
        &mut flow1,
        &mut tracker,
        r#"{"tag":"GO","type":"9","lobby":"0"}"#,
    );
    feed(
        &mut flow1,
        &mut tracker,
        r#"{"tag":"UN","n0":"us","n1":"shimocha","n2":"toimen","n3":"kamicha"}"#,
    );
    feed(&mut flow1, &mut tracker, r#"{"tag":"TAIKYOKU","oya":"2"}"#);
    let init = format!(
        r#"{{"tag":"INIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"2","hai":"{HAND_E1}"}}"#
    );
    feed(&mut flow1, &mut tracker, &init);
    assert_eq!(tracker.our_seat(), Some(2));
    assert_eq!(our_tehai(&tracker), sorted_mjai(HAND_E1));

    // Dealer (rel 2 = abs 0) draws and discards, then abs 1, then us, then abs 3.
    for frame in [
        r#"{"tag":"V"}"#,
        r#"{"tag":"F5"}"#,
        r#"{"tag":"W"}"#,
        r#"{"tag":"G9"}"#,
        r#"{"tag":"T20"}"#,
        r#"{"tag":"D20"}"#,
        r#"{"tag":"U"}"#,
        r#"{"tag":"E33"}"#,
    ] {
        feed(&mut flow1, &mut tracker, frame);
    }
    assert_eq!(our_tehai(&tracker), sorted_mjai(HAND_E1));
    let seen_before_drop = tracker.events_seen;
    drop(flow1);

    // ---- Flow 2: the client rejoins. No TAIKYOKU, a REINIT snapshot. ----
    let mut flow2 = TenhouBridge::new(None, None);
    let reinit = format!(
        r#"{{"tag":"REINIT","seed":"0,0,0,1,2,4","ten":"250,250,250,250","oya":"2","hai":"{HAND_E1}","kawa0":"20","kawa1":"33","kawa2":"5","kawa3":"9"}}"#
    );
    assert!(feed(&mut flow2, &mut tracker, &reinit).is_empty());

    // The rest of this hand is withheld from the tracker entirely.
    for frame in [
        r#"{"tag":"V"}"#,
        r#"{"tag":"F40"}"#,
        r#"{"tag":"W"}"#,
        r#"{"tag":"G44"}"#,
        r#"{"tag":"T24"}"#,
        r#"{"tag":"D24"}"#,
        r#"{"tag":"U"}"#,
        r#"{"tag":"E48"}"#,
        r#"{"tag":"AGARI","who":"2","fromWho":"2","sc":"250,-10,250,-10,250,30,250,-10","ba":"0,0"}"#,
    ] {
        assert!(
            feed(&mut flow2, &mut tracker, frame).is_empty(),
            "{frame} must not reach the tracker"
        );
    }
    assert_eq!(tracker.events_seen, seen_before_drop);
    assert_eq!(our_tehai(&tracker), sorted_mjai(HAND_E1));

    // ---- Next kyoku: E2, dealer wire-abs 1 (rel 3 from seat 2). ----
    let init = format!(
        r#"{{"tag":"INIT","seed":"1,0,0,1,2,8","ten":"240,240,280,240","oya":"3","hai":"{HAND_E2}"}}"#
    );
    let events = feed(&mut flow2, &mut tracker, &init);
    assert!(
        matches!(
            &events[0],
            MjaiEvent::StartGame {
                id: Some(2),
                num_players: 4,
                ..
            }
        ),
        "the rejoined flow reopens the game on our real seat: {events:?}"
    );
    let snap = tracker.snapshot().unwrap();
    assert_eq!(snap.our_seat, Some(2));
    assert_eq!(snap.kyoku, 2);
    assert_eq!(snap.oya, 1);
    assert_eq!(our_tehai(&tracker), sorted_mjai(HAND_E2));

    // Live play resumes on the right seats: opponent draws stay hidden and
    // never leak into our hand as 1m; our own draw lands in it.
    feed(&mut flow2, &mut tracker, r#"{"tag":"W"}"#);
    assert_eq!(our_tehai(&tracker), sorted_mjai(HAND_E2));
    feed(&mut flow2, &mut tracker, r#"{"tag":"G12"}"#);
    let events = feed(&mut flow2, &mut tracker, r#"{"tag":"T24"}"#);
    assert!(matches!(&events[0], MjaiEvent::Tsumo { actor: 2, pai } if pai == "7m"));
    let snap = tracker.snapshot().unwrap();
    let mut expected = sorted_mjai(HAND_E2);
    expected.push("7m".into());
    expected.sort();
    assert_eq!(our_tehai(&tracker), expected);
    assert_eq!(snap.players[2].drawn_tile.as_deref(), Some("7m"));
    assert!(
        !snap.players[2].tehai.iter().any(|t| t == "1m"),
        "no phantom 1m: {:?}",
        snap.players[2].tehai
    );
}
