//! Shared state held by Tauri as `tauri::State<AppState>`.
//!
//! `AppState` owns the long-lived handles every IPC command and forwarder
//! needs: a clone of each `event_bus` `Sender`, the live `AppConfig`, the
//! log session directory, and the runtime handle that lets commands
//! start / stop the capture backend on demand.
//!
//! Snapshots vs streams: the `*_bus` channels are the canonical event
//! stream, but a frontend that opens a window mid-game needs a one-shot
//! "what's the current state?" answer. To serve that, the IPC forwarder
//! task mirrors every status event into the `bot_status` /
//! `capture_control.status` slots, and the `get_status` command reads
//! them back. Commands that *change* state (set_active_bot,
//! start/stop_capture) also write the snapshot synchronously so a
//! follow-up `get_status` is always consistent with the action that just
//! succeeded.

use crate::analysis::runner::AnalysisCache;
use crate::autoplay::AutoplayContext;
use crate::bot::PythonRuntime;
use crate::config::AppConfig;
use crate::event_bus::{
    AnalysisBus, BotResponseBus, BotStatusBus, CaptureStatusBus, HistoryBus, MjaiBus, NotifyBus,
};
use crate::game_state::GameTracker;
use crate::history::HistoryStore;
use crate::history::recorder::SharedPlatform;
use crate::logger::Session;
use crate::schema::{BotStatus, CaptureStatus};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, Notify, RwLock};

/// Per-running-capture-backend control handle.
///
/// `stop` is `Some` while the backend task is alive; sending `()`
/// triggers graceful shutdown and the task exits. After exit the
/// supervisor sets `stop = None` and updates `status`.
///
/// `force_close` is shared with the hudsucker handler when MITM is the
/// active backend; calling `notify_waiters()` kicks every in-flight
/// WebSocket flow so the game client actually disconnects (graceful
/// shutdown alone only blocks new connections — existing flows would
/// otherwise drain naturally). The Chromium backend ignores it.
pub struct CaptureControl {
    pub status: CaptureStatus,
    pub stop: Option<oneshot::Sender<()>>,
    pub force_close: Arc<Notify>,
}

impl Default for CaptureControl {
    fn default() -> Self {
        Self {
            status: CaptureStatus::Stopped,
            stop: None,
            force_close: Arc::new(Notify::new()),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub config_path: Arc<PathBuf>,
    pub log_session: Arc<Session>,

    pub mjai_bus: MjaiBus,
    pub bot_response_bus: BotResponseBus,
    pub bot_status_bus: BotStatusBus,
    pub capture_status_bus: CaptureStatusBus,
    pub notify_bus: NotifyBus,
    pub analysis_bus: AnalysisBus,
    pub history_bus: HistoryBus,

    /// Latest BotStatus seen on the bus. Forwarder writes; commands read.
    pub bot_status: Arc<RwLock<BotStatus>>,
    pub capture_control: Arc<Mutex<CaptureControl>>,
    /// Live game-state mirror. Future IPC commands lock this and call
    /// `snapshot()` to expose hands/scores/dora to the frontend.
    pub game_tracker: Arc<Mutex<GameTracker>>,
    /// Latest analysis result, populated by the analysis runner. Read by
    /// the `get_analysis` Tauri command for one-shot queries.
    pub analysis_cache: AnalysisCache,
    /// Persistent game-history store. Written by the recorder task,
    /// read by `list_game_history` / `get_game_history_*` IPC commands.
    pub history_store: Arc<HistoryStore>,
    /// Shared cell holding the platform tag the history recorder stamps
    /// onto each finalised game. `update_config` updates this when the
    /// user switches bridges so subsequent records pick up the new tag
    /// without a relaunch.
    pub history_platform: SharedPlatform,

    /// Bundled-or-system Python + uv. `None` on dev boxes lacking both —
    /// install/sync commands surface a friendly error instead of panicking;
    /// the bot manager refuses to start when bot mode is enabled.
    pub runtime: Option<PythonRuntime>,
    /// Names of bots whose `uv sync` is currently in flight. Both the
    /// `sync_bot_deps` command and `BotManager::spawn_runner` acquire-or-bail
    /// so two parallel syncs against the same venv can't trample each other.
    pub syncs_in_flight: Arc<Mutex<HashSet<String>>>,
    /// Set to `true` once a `BotManager` has been spawned for this process.
    /// Used by `update_config` to start the manager on a runtime
    /// false→true flip of `bot.enabled` (e.g. when the first-run wizard
    /// finishes) without ever spawning twice. There is no off-switch:
    /// once started, the manager runs for the lifetime of the process.
    pub bot_manager_started: Arc<AtomicBool>,
    /// Shared with the chromium capture backend (which writes the page
    /// handle on Majsoul WS open) and the `AutoplayManager` (which reads
    /// it to dispatch input). Always present; `enabled = false` in
    /// config simply suppresses the manager spawn — the context still
    /// exists so a runtime toggle works without re-plumbing.
    pub autoplay_context: Arc<AutoplayContext>,
    /// Set to `true` once an `AutoplayManager` has been spawned for this
    /// process. Used by `update_config` to start the manager on a
    /// runtime false→true flip of `autoplay.enabled` without re-spawning.
    pub autoplay_manager_started: Arc<AtomicBool>,
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: AppConfig,
        config_path: PathBuf,
        log_session: Arc<Session>,
        mjai_bus: MjaiBus,
        bot_response_bus: BotResponseBus,
        bot_status_bus: BotStatusBus,
        capture_status_bus: CaptureStatusBus,
        notify_bus: NotifyBus,
        analysis_bus: AnalysisBus,
        history_bus: HistoryBus,
        game_tracker: Arc<Mutex<GameTracker>>,
        analysis_cache: AnalysisCache,
        history_store: Arc<HistoryStore>,
        history_platform: SharedPlatform,
        runtime: Option<PythonRuntime>,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            config_path: Arc::new(config_path),
            log_session,
            mjai_bus,
            bot_response_bus,
            bot_status_bus,
            capture_status_bus,
            notify_bus,
            analysis_bus,
            history_bus,
            bot_status: Arc::new(RwLock::new(BotStatus::Idle)),
            capture_control: Arc::new(Mutex::new(CaptureControl::default())),
            game_tracker,
            analysis_cache,
            history_store,
            history_platform,
            runtime,
            syncs_in_flight: Arc::new(Mutex::new(HashSet::new())),
            bot_manager_started: Arc::new(AtomicBool::new(false)),
            autoplay_context: Arc::new(AutoplayContext::new()),
            autoplay_manager_started: Arc::new(AtomicBool::new(false)),
        }
    }
}
