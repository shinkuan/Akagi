//! Integration tests for the BotManager lifecycle status emission.
//!
//! These exercise the *happy path* of `spawn_runner`:
//! `Loading{SyncingDeps} → Loading{Spawning} → Ready` plus the matched
//! `Notification` pair (sticky info → success, both with the same `id`
//! so the frontend can swap toasts cleanly).
//!
//! The error paths are covered by the unit tests in
//! `src/bot/manager.rs::tests` (no real subprocess required). This file
//! drives a real `uv sync` + python spawn against a stdlib-only echo
//! bot so the full state machine fires.
//!
//! Skipped silently when `uv` or `python` is not on PATH — CI without
//! the toolchain just sees a passing no-op.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use akagi::bot::{BotManager, BotRegistry, PythonRuntime};
use akagi::config::AppConfig;
use akagi::event_bus::{bot_response_bus, bot_status_bus, notify_bus};
use akagi::inspector::InspectorWriter;
use akagi::schema::{BotStatus, LoadStage, MjaiEvent, NotifyLevel};
use tempfile::TempDir;
use tokio::sync::{broadcast, Mutex, RwLock};

fn fresh_syncs() -> Arc<Mutex<HashSet<String>>> {
    Arc::new(Mutex::new(HashSet::new()))
}

/// Shared config handle with a 4p active bot, as the supervisor hands the
/// manager. The active-bot name is read from this at each `start_game`.
fn cfg_with(active_4p: &str) -> Arc<RwLock<AppConfig>> {
    let mut c = AppConfig::default();
    c.bot.active_4p = active_4p.to_string();
    c.bot.active_3p = String::new();
    Arc::new(RwLock::new(c))
}

fn dummy_inspector() -> InspectorWriter {
    let keep = || {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.into_temp_path().keep().unwrap()
    };
    InspectorWriter::open(&keep(), &keep(), 8).unwrap().0
}

fn bin(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

fn write_echo_bot(dir: &Path) {
    fs::write(
        dir.join("bot.py"),
        r#"import sys, json
for line in sys.stdin:
    print('{"type":"none"}', flush=True)
    try:
        evs = json.loads(line)
    except Exception:
        continue
    if any(e.get("type") == "end_game" for e in evs):
        break
"#,
    )
    .unwrap();
    // `package = false` so uv treats this as a dep-only project and
    // skips trying to build the (empty) source tree. No deps → sync is
    // just venv creation, which takes ~1s.
    fs::write(
        dir.join("pyproject.toml"),
        r#"[project]
name = "echo-bot"
version = "0.1.0"
requires-python = ">=3.9"

[tool.uv]
package = false
"#,
    )
    .unwrap();
}

async fn next_status(rx: &mut broadcast::Receiver<BotStatus>) -> BotStatus {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("status event within 2s")
        .expect("recv ok")
}

#[tokio::test]
async fn loading_emits_syncing_then_spawning_then_ready() {
    let (Some(uv), Some(py)) = (bin("uv"), bin("python3").or_else(|| bin("python"))) else {
        eprintln!("skip: uv or python not on PATH");
        return;
    };

    let registry_root = TempDir::new().unwrap();
    let bot_dir = registry_root.path().join("echo");
    fs::create_dir_all(&bot_dir).unwrap();
    write_echo_bot(&bot_dir);

    let runtime = PythonRuntime::from_paths(py, uv, akagi::bot::RuntimeMode::System);
    // Sanity-check the layout once before handing off — manager itself
    // rescans on every spawn now (regression test for stale-snapshot
    // bug), but we still want a clear failure here if write_echo_bot
    // ever silently changes shape.
    let registry = BotRegistry::scan(registry_root.path()).unwrap();
    assert!(
        registry.find("echo").is_some(),
        "registry should pick up the test bot"
    );

    let response_bus = bot_response_bus();
    let status_bus = bot_status_bus();
    let notify = notify_bus();
    let mut status_rx = status_bus.subscribe();
    let mut notify_rx = notify.subscribe();

    let mut mgr = BotManager::new(
        Some(runtime),
        registry_root.path().to_path_buf(),
        cfg_with("echo"),
        response_bus,
        status_bus,
        notify,
        dummy_inspector(),
        fresh_syncs(),
    );

    // First-time spawn: ensure_synced creates the venv (no deps to
    // download, but the python interpreter is seeded), then the
    // subprocess is started. 60s ceiling covers cold I/O on CI.
    tokio::time::timeout(
        Duration::from_secs(60),
        mgr.handle(MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 4,
            game_meta: None,
        }),
    )
    .await
    .expect("spawn within 60s")
    .expect("handle ok");

    match next_status(&mut status_rx).await {
        BotStatus::Loading {
            stage: LoadStage::SyncingDeps,
            bot,
        } => assert_eq!(bot, "echo"),
        other => panic!("expected Loading{{SyncingDeps}}, got {other:?}"),
    }
    match next_status(&mut status_rx).await {
        BotStatus::Loading {
            stage: LoadStage::Spawning,
            bot,
        } => assert_eq!(bot, "echo"),
        other => panic!("expected Loading{{Spawning}}, got {other:?}"),
    }
    match next_status(&mut status_rx).await {
        BotStatus::Ready { bot, actor_id } => {
            assert_eq!(bot, "echo");
            assert_eq!(actor_id, 0);
        }
        other => panic!("expected Ready, got {other:?}"),
    }

    // Sticky info first (loading), success second (ready). Both share
    // the same id so the frontend swaps the same toast slot.
    let n1 = tokio::time::timeout(Duration::from_secs(1), notify_rx.recv())
        .await
        .expect("notify event")
        .expect("recv");
    assert_eq!(n1.level, NotifyLevel::Info);
    assert!(n1.sticky, "loading toast must be sticky");
    assert_eq!(n1.id.as_deref(), Some("bot-loading-echo"));

    let n2 = tokio::time::timeout(Duration::from_secs(1), notify_rx.recv())
        .await
        .expect("notify event")
        .expect("recv");
    assert_eq!(n2.level, NotifyLevel::Success);
    assert_eq!(n2.id.as_deref(), Some("bot-loading-echo"));
}

#[tokio::test]
async fn second_spawn_skips_uv_sync_via_stamp() {
    // Re-using a venv that's already in sync should run the SyncingDeps
    // emission (status is unconditional) but `ensure_synced` should
    // return immediately — i.e. the whole spawn finishes quickly.
    let (Some(uv), Some(py)) = (bin("uv"), bin("python3").or_else(|| bin("python"))) else {
        eprintln!("skip: uv or python not on PATH");
        return;
    };

    let registry_root = TempDir::new().unwrap();
    let bot_dir = registry_root.path().join("echo");
    fs::create_dir_all(&bot_dir).unwrap();
    write_echo_bot(&bot_dir);

    let runtime =
        PythonRuntime::from_paths(py.clone(), uv.clone(), akagi::bot::RuntimeMode::System);

    // Cold spawn — populates venv + stamp.
    {
        let response_bus = bot_response_bus();
        let status_bus = bot_status_bus();
        let notify = notify_bus();
        let mut mgr = BotManager::new(
            Some(runtime.clone()),
            registry_root.path().to_path_buf(),
            cfg_with("echo"),
            response_bus,
            status_bus,
            notify,
            dummy_inspector(),
            fresh_syncs(),
        );
        tokio::time::timeout(
            Duration::from_secs(60),
            mgr.handle(MjaiEvent::StartGame {
                names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
                kyoku_first: None,
                aka_flag: None,
                id: Some(0),
                num_players: 4,
                game_meta: None,
            }),
        )
        .await
        .expect("cold spawn within 60s")
        .expect("handle ok");
    }

    // Warm spawn — same dir, fresh manager. Stamp matches, ensure_synced
    // is a no-op, full transition reaches Ready in well under 5s.
    let response_bus = bot_response_bus();
    let status_bus = bot_status_bus();
    let notify = notify_bus();
    let mut status_rx = status_bus.subscribe();
    let mut mgr = BotManager::new(
        Some(runtime),
        registry_root.path().to_path_buf(),
        cfg_with("echo"),
        response_bus,
        status_bus,
        notify,
        dummy_inspector(),
        fresh_syncs(),
    );

    let started = std::time::Instant::now();
    tokio::time::timeout(
        Duration::from_secs(5),
        mgr.handle(MjaiEvent::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(2),
            num_players: 4,
            game_meta: None,
        }),
    )
    .await
    .expect("warm spawn within 5s")
    .expect("handle ok");
    let elapsed = started.elapsed();

    // Drain to Ready and confirm warm path is fast.
    let mut saw_ready = false;
    for _ in 0..3 {
        match next_status(&mut status_rx).await {
            BotStatus::Ready { actor_id, .. } => {
                assert_eq!(actor_id, 2);
                saw_ready = true;
                break;
            }
            BotStatus::Loading { .. } => continue,
            other => panic!("unexpected status: {other:?}"),
        }
    }
    assert!(saw_ready, "warm spawn should reach Ready");
    assert!(
        elapsed < Duration::from_secs(5),
        "warm spawn took {elapsed:?} — stamp should make it fast"
    );
}
