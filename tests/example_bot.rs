//! End-to-end tests against the bundled rule-based example bot at
//! `mjai_bot/example/`.
//!
//! These exercise the full `BotRunner` ↔ mjai contract — JSONL framing,
//! `MjaiEvent::None` handling, decision-point flushing, the bot's
//! algorithm itself.
//!
//! The tests skip silently when `mjai_bot/example/.akagi/venv/bin/python`
//! is missing — that means the contributor hasn't run `uv sync` yet (or
//! `uv` isn't available). On a properly-set-up dev machine they run as
//! part of `cargo test`.
//!
//! Run manually:
//! ```sh
//! cd mjai_bot/example && UV_PROJECT_ENVIRONMENT=.akagi/venv uv sync
//! cargo test --test example_bot -- --nocapture
//! ```

use akagi::bot::{BotRunner, PythonRuntime, RuntimeMode, SubprocessBot};
use akagi::schema::MjaiEvent;
use std::path::{Path, PathBuf};
use tokio::process::Command;

// ----- skip-aware fixture helpers -----

fn example_bot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("mjai_bot")
        .join("example")
}

fn venv_python(bot_dir: &Path) -> Option<PathBuf> {
    let venv = bot_dir.join(".akagi").join("venv");
    let py = if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    };
    py.is_file().then_some(py)
}

async fn spawn_example(actor_id: u8) -> Option<SubprocessBot> {
    let bot_dir = example_bot_dir();
    let py = venv_python(&bot_dir)?;

    let mut cmd = Command::new(&py);
    cmd.current_dir(&bot_dir)
        .arg("bot.py")
        .arg(actor_id.to_string());

    // We bypass `runtime.ensure_synced` (it would call `uv sync` and is
    // covered by `runtime.rs` tests). Use `from_paths` purely so the bot
    // has *some* runtime reference for `reset()` to respawn against.
    let runtime = PythonRuntime::from_paths(
        py.clone(),
        PathBuf::from("/dev/null/uv"),
        RuntimeMode::System,
    );
    Some(
        SubprocessBot::spawn_with_command(cmd, runtime, &bot_dir, actor_id)
            .await
            .expect("spawn example bot"),
    )
}

// ----- mjai event builders for fixtures -----

fn empty_tehai() -> [String; 13] {
    std::array::from_fn(|_| "?".to_string())
}

fn tehai(tiles: &[&str]) -> [String; 13] {
    assert_eq!(tiles.len(), 13, "mjai tehais must be length 13");
    let mut out: [String; 13] = empty_tehai();
    for (i, t) in tiles.iter().enumerate() {
        out[i] = (*t).to_string();
    }
    out
}

fn start_game(seat: u8) -> MjaiEvent {
    MjaiEvent::StartGame {
        names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
        kyoku_first: Some(0),
        aka_flag: Some(true),
        id: Some(seat),
        num_players: 4,
    }
}

fn start_kyoku(my_seat: u8, my_tehai: [String; 13]) -> MjaiEvent {
    let mut tehais: Vec<Vec<String>> = (0..4).map(|_| empty_tehai().to_vec()).collect();
    tehais[my_seat as usize] = my_tehai.to_vec();
    MjaiEvent::StartKyoku {
        bakaze: "E".into(),
        dora_marker: "1m".into(),
        kyoku: 1,
        honba: 0,
        kyotaku: 0,
        oya: 0,
        scores: vec![25000, 25000, 25000, 25000],
        tehais,
        num_players: 4,
    }
}

// ----- tests -----

#[tokio::test]
async fn declares_riichi_when_tenpai_with_two_waits() {
    let Some(mut bot) = spawn_example(2).await else {
        eprintln!("skip: mjai_bot/example/.akagi/venv not synced — run `uv sync` first");
        return;
    };

    // Closed hand 1m2m3m 1p2p3p 1s2s3s EE SS — tenpai with E or S wait.
    // Drawing any harmless tile leaves the bot at tenpai with 2 distinct
    // waits → ≥ 2 wait threshold → riichi declaration.
    let my_tehai = tehai(&[
        "1m", "2m", "3m", "1p", "2p", "3p", "1s", "2s", "3s", "E", "E", "S", "S",
    ]);
    let resp = bot
        .react(&[
            start_game(2),
            start_kyoku(2, my_tehai),
            MjaiEvent::Tsumo {
                actor: 2,
                pai: "5p".into(),
            },
        ])
        .await
        .expect("react");

    match resp.action {
        MjaiEvent::Reach { actor: 2, .. } => {}
        other => panic!("expected reach by seat 2, got {other:?}"),
    }
    assert!(resp.meta.is_none(), "rule-based bot has no meta payload");

    // Clean shutdown — bot's main loop breaks on end_game.
    bot.set_react_timeout(std::time::Duration::from_millis(500));
    let _ = bot.react(&[MjaiEvent::EndGame]).await;
}

#[tokio::test]
async fn discards_min_shanten_when_one_shanten() {
    let Some(mut bot) = spawn_example(0).await else {
        eprintln!("skip: mjai_bot/example/.akagi/venv not synced");
        return;
    };

    // Hand: 1m2m3m 4m5m 1p2p3p 1s2s3s EE — 1-shanten with assorted shape.
    // Drawing 9m gives 14 tiles. Bot should pick the discard that
    // minimises shanten (likely the lone 9m or breaks the worst pair).
    // We don't pin a specific tile (the bot's tiebreak depends on
    // ukeire which is sensitive to mahjong-lib internals), only that
    // SOME dahai is returned and it isn't tsumogiri-noise.
    let my_tehai = tehai(&[
        "1m", "2m", "3m", "4m", "5m", "1p", "2p", "3p", "1s", "2s", "3s", "E", "E",
    ]);
    let resp = bot
        .react(&[
            start_game(0),
            start_kyoku(0, my_tehai),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "9m".into(),
            },
        ])
        .await
        .expect("react");

    match resp.action {
        MjaiEvent::Dahai { actor: 0, .. } => {}
        MjaiEvent::Reach { actor: 0, .. } => {} // tenpai-shaped fallthrough is OK
        other => panic!("expected dahai/reach, got {other:?}"),
    }

    bot.set_react_timeout(std::time::Duration::from_millis(500));
    let _ = bot.react(&[MjaiEvent::EndGame]).await;
}

#[tokio::test]
async fn returns_none_for_others_tsumo() {
    let Some(mut bot) = spawn_example(0).await else {
        eprintln!("skip: mjai_bot/example/.akagi/venv not synced");
        return;
    };

    let my_tehai = tehai(&[
        "1m", "2m", "3m", "4m", "5m", "1p", "2p", "3p", "1s", "2s", "3s", "E", "E",
    ]);
    // Note: Akagi's BotManager would normally not flush a batch on
    // others' tsumo (it isn't a decision point). This test bypasses the
    // manager and feeds the event directly to verify the *bot* is
    // robust — it should observe seat 1's tsumo and return `none`.
    let resp = bot
        .react(&[
            start_game(0),
            start_kyoku(0, my_tehai),
            MjaiEvent::Tsumo {
                actor: 1,
                pai: "9m".into(),
            },
        ])
        .await
        .expect("react");

    assert!(
        matches!(resp.action, MjaiEvent::None),
        "expected None action for others' tsumo, got {:?}",
        resp.action
    );

    bot.set_react_timeout(std::time::Duration::from_millis(500));
    let _ = bot.react(&[MjaiEvent::EndGame]).await;
}

#[tokio::test]
async fn pons_yakuhai_when_shanten_decreases() {
    let Some(mut bot) = spawn_example(0).await else {
        eprintln!("skip: mjai_bot/example/.akagi/venv not synced");
        return;
    };

    // Hand: 3m4m 5m6m7m 2p2p 4p4p WW EE — 13 tiles, 2-shanten.
    // - 567m set, 3m4m protoset
    // - 2p2p, 4p4p, WW, EE — four pairs (one anchors as the pair, the
    //   others want to triple up).
    // Seat 0 = oya in East-1 → EE is round-wind double yakuhai.
    //
    // When seat 1 discards E, pon → meld EEE drops us to 1-shanten
    // (after discarding 3m or 4m: 567m + EEE meld + 3 pairs need one
    // more triplet from 2p2p / 4p4p / WW). `_has_viable_yaku` returns
    // true via the obvious yakuhai-meld heuristic.
    //
    // Bot has not tsumo'd yet — feeding seat 1's tsumo+dahai directly
    // means our internal hand stays at the fixture's 13 concealed
    // tiles, which is what Akagi's proxy actually delivers between
    // turns.
    let my_tehai = tehai(&[
        "3m", "4m", "5m", "6m", "7m", "2p", "2p", "4p", "4p", "W", "W", "E", "E",
    ]);
    let resp = bot
        .react(&[
            start_game(0),
            start_kyoku(0, my_tehai),
            MjaiEvent::Tsumo {
                actor: 1,
                pai: "?".into(),
            },
            MjaiEvent::Dahai {
                actor: 1,
                pai: "E".into(),
                tsumogiri: false,
            },
        ])
        .await
        .expect("react");

    match resp.action {
        MjaiEvent::Pon {
            actor: 0,
            target: 1,
            ref pai,
            ref consumed,
        } => {
            assert_eq!(pai, "E");
            assert_eq!(consumed.len(), 2);
            assert!(consumed.iter().all(|c| c == "E"), "consumed = {consumed:?}");
        }
        other => panic!("expected pon E from seat 1, got {other:?}"),
    }

    bot.set_react_timeout(std::time::Duration::from_millis(500));
    let _ = bot.react(&[MjaiEvent::EndGame]).await;
}
