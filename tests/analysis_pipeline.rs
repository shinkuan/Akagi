//! End-to-end test for the Phase 4 analysis pipeline.
//!
//! Wires the same buses as production:
//!   MjaiBus → GameTracker.run → PostTrackerBus → analysis::runner → AnalysisBus
//!
//! Sends a synthetic mjai event sequence and asserts that an
//! `AnalysisResult` arrives on the bus *and* that the cache slot the IPC
//! `get_analysis` command reads is populated.

use std::sync::Arc;
use std::time::Duration;

use akagi::analysis::runner::AnalysisCache;
use akagi::event_bus::{analysis_bus, mjai_bus, post_tracker_bus};
use akagi::game_state::mahgen_view::MahgenView;
use akagi::game_state::spawn_with_post;
use akagi::schema::MjaiEvent;
use tokio::sync::RwLock;

fn start_game(seat: u8) -> MjaiEvent {
    MjaiEvent::StartGame {
        names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        kyoku_first: None,
        aka_flag: None,
        id: Some(seat),
        num_players: 4,
        game_meta: None,
    }
}

fn start_kyoku() -> MjaiEvent {
    // Tenpai-shape seat-0 hand: 234m 234p 234s 67p + EE → waits 5p/8p ryanmen.
    let seat0: Vec<String> = vec![
        "2m".into(),
        "3m".into(),
        "4m".into(),
        "2p".into(),
        "3p".into(),
        "4p".into(),
        "6p".into(),
        "7p".into(),
        "2s".into(),
        "3s".into(),
        "4s".into(),
        "E".into(),
        "E".into(),
    ];
    let filler: Vec<String> = (0..13).map(|_| "1m".into()).collect();
    MjaiEvent::StartKyoku {
        bakaze: "E".into(),
        dora_marker: "9p".into(),
        kyoku: 1,
        honba: 0,
        kyotaku: 0,
        oya: 0,
        scores: vec![25_000, 25_000, 25_000, 25_000],
        tehais: vec![seat0, filler.clone(), filler.clone(), filler],
        num_players: 4,
        catch_up: 0,
    }
}

#[tokio::test]
async fn end_to_end_pipeline_emits_analysis_for_seat_0() {
    let mjai = mjai_bus();
    let post = post_tracker_bus();
    let bus = analysis_bus();
    let cache: AnalysisCache = Arc::new(RwLock::new(None));

    let _tracker = spawn_with_post(mjai.subscribe(), Some(post.clone()));
    akagi::analysis::runner::spawn(
        post.subscribe(),
        _tracker.clone(),
        bus.clone(),
        cache.clone(),
    );

    let mut rx = bus.subscribe();
    mjai.send(start_game(0)).unwrap();
    mjai.send(start_kyoku()).unwrap();

    // Drain at least one analysis result. Allow up to 1s to absorb broadcast +
    // channel scheduling jitter on slower CI hosts.
    let result = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match rx.recv().await {
                Ok(r) => break r,
                Err(_) => continue,
            }
        }
    })
    .await
    .expect("timed out waiting for analysis-result");

    assert_eq!(result.seat, 0);
    assert_eq!(result.shanten, 0);
    // Cached for IPC `get_analysis`.
    let cached = cache.read().await.clone().expect("cache populated");
    assert_eq!(cached.seat, 0);
    assert_eq!(cached.shanten, 0);

    // Sanity: at least one wait info entry on a tenpai shape.
    let h13 = result.hand13.as_ref().expect("hand13 in 13-tile state");
    assert!(!h13.waits.is_empty(), "expected waits at tenpai");
}

#[tokio::test]
async fn tracker_snapshot_emits_mahgen_view() {
    // Same setup as the e2e test, but pull a MahgenView directly from the
    // tracker after start_kyoku has settled.
    let mjai = mjai_bus();
    let post = post_tracker_bus();
    let _bus = analysis_bus();
    let cache: AnalysisCache = Arc::new(RwLock::new(None));

    let tracker = spawn_with_post(mjai.subscribe(), Some(post.clone()));
    akagi::analysis::runner::spawn(
        post.subscribe(),
        tracker.clone(),
        _bus.clone(),
        cache.clone(),
    );

    mjai.send(start_game(0)).unwrap();
    mjai.send(start_kyoku()).unwrap();

    // Allow the tracker task to drain.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let snap = tracker.lock().await.snapshot().expect("snapshot");
    assert_eq!(snap.our_seat, Some(0));
    let view = MahgenView::from_snapshot(&snap);

    // Self (seat 0) sees real tiles; others see backs.
    assert!(view.players[0].hand.contains('m') || view.players[0].hand.contains('p'));
    for op in 1..4 {
        assert!(
            view.players[op].hand.ends_with('z'),
            "op{op} hand should be backs"
        );
        // Hand of N tile-backs renders as N zeros + 'z'.
        assert!(
            view.players[op].hand.starts_with('0'),
            "op{op} hand should start with 0"
        );
    }
    // Dora indicator string non-empty.
    assert!(!view.dora_indicators.is_empty());
}

#[tokio::test]
async fn no_start_game_no_analysis() {
    let mjai = mjai_bus();
    let post = post_tracker_bus();
    let bus = analysis_bus();
    let cache: AnalysisCache = Arc::new(RwLock::new(None));

    let _tracker = spawn_with_post(mjai.subscribe(), Some(post.clone()));
    akagi::analysis::runner::spawn(
        post.subscribe(),
        _tracker.clone(),
        bus.clone(),
        cache.clone(),
    );

    let mut rx = bus.subscribe();
    // Send start_kyoku WITHOUT a preceding start_game → no own_seat captured →
    // analysis runner skips silently.
    mjai.send(start_kyoku()).unwrap();

    let res = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(
        res.is_err(),
        "should time out — no analysis without start_game"
    );
    assert!(cache.read().await.is_none());
}
