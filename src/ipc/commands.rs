//! `#[tauri::command]` handlers exposed to the frontend.
//!
//! Errors are returned as `String` because Tauri serializes command
//! errors via `Display` and most call sites just want a human message in
//! a toast. Keep the JSON shape conservative — clients lock onto field
//! names quickly and renames break dashboards.

use crate::analysis::result::AnalysisResult;
use crate::bot::install::{self, GithubInstallSpec};
use crate::bot::manifest::{self, BotSource};
use crate::bot::runtime;
use crate::bot::sync_guard::SyncGuard;
use crate::bot::{BotEntry, BotRegistry};
use crate::config::AppConfig;
use crate::game_state::mahgen_view::MahgenView;
use crate::game_state::snapshot::GameStateSnapshot;
use crate::ipc::capture_supervisor::{
    restart_capture as restart_capture_inner, spawn_capture_supervisor,
};
use crate::ipc::state::AppState;
use crate::schema::{
    BotInfo, BotSettings, GameRecord, HistoryEvent, HistoryEventLog, HistoryFilter, HoraScoreInfo,
    InspectorEntry, LogEntry, LogSessionInfo, Notification, ReadInspectorRequest,
    ReadInspectorResponse, ReadLogRequest, ReadLogResponse, Snapshot,
};
use crate::util::resolve_dir;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::State;

/// Returns `true` exactly once per process the first time `bot_enabled`
/// is observed as `true` here. Side-effect on success: flips `flag`
/// false→true via a CAS. Used by `update_config` to decide whether the
/// caller should spawn a fresh `BotManager`.
fn claim_bot_manager_spawn(bot_enabled: bool, flag: &std::sync::atomic::AtomicBool) -> bool {
    use std::sync::atomic::Ordering;
    bot_enabled
        && flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
}

/// Same once-per-process gating as the bot variant, but for the autoplay
/// manager. Toggling `autoplay.enabled` back to false leaves the manager
/// alive — it just becomes a no-op because every bot response is gated
/// on a fresh `cfg.autoplay.enabled` read.
fn claim_autoplay_manager_spawn(
    autoplay_enabled: bool,
    flag: &std::sync::atomic::AtomicBool,
) -> bool {
    use std::sync::atomic::Ordering;
    autoplay_enabled
        && flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
}

fn entry_to_info(e: &BotEntry) -> BotInfo {
    BotInfo {
        name: e.name.clone(),
        dir: e.dir.to_string_lossy().into_owned(),
        has_pyproject: e.pyproject.is_some(),
        manifest: e.manifest.clone(),
    }
}

type CmdResult<T> = Result<T, String>;

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> CmdResult<AppConfig> {
    Ok(state.config.read().await.clone())
}

/// Replace the entire config and persist it to the same file the app
/// loaded from. Capture-related changes (mode, chromium settings, proxy
/// settings) trigger an automatic supervisor restart so the user doesn't
/// have to relaunch the app to switch capture modes. A `bot.enabled`
/// false→true flip (typically the first-run wizard finishing) hot-starts
/// the `BotManager` so the user doesn't have to relaunch either; once
/// started the manager runs for the lifetime of the process (toggling
/// `bot.enabled` back to false still requires a relaunch to actually
/// stop it).
#[tauri::command]
pub async fn update_config(new_config: AppConfig, state: State<'_, AppState>) -> CmdResult<()> {
    persist_config(&new_config, &state.config_path).map_err(|e| e.to_string())?;

    // Snapshot the *previous* capture-relevant fields before we overwrite,
    // so we can decide whether the supervisor needs a swap.
    let (prev_capture, prev_proxy, prev_platform) = {
        let cfg = state.config.read().await;
        (cfg.capture.clone(), cfg.proxy.clone(), cfg.platform.kind)
    };
    let capture_changed = prev_capture != new_config.capture;
    let proxy_changed = prev_proxy != new_config.proxy;
    let platform_changed = prev_platform != new_config.platform.kind;
    let new_platform = new_config.platform.kind;
    let bot_cfg = new_config.bot.clone();
    let bot_now_enabled = new_config.bot.enabled;
    let autoplay_now_enabled = new_config.autoplay.enabled;
    *state.config.write().await = new_config;

    // Sync the history recorder's platform tag immediately. Subsequent
    // finalised games are stamped with the new tag; the in-flight buffer
    // (if any) keeps the tag it had at start_game — acceptable, since
    // mid-game platform switches are not a real workflow.
    if platform_changed {
        *state
            .history_platform
            .write()
            .expect("history platform lock poisoned") =
            crate::schema::Platform::from(new_platform);
    }

    if capture_changed || proxy_changed || platform_changed {
        // Run the restart in the background — `update_config` returns
        // promptly so the UI doesn't hang on slow shutdowns.
        let st = (*state).clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = restart_capture_inner(st).await {
                let _ = ();
                tracing::error!("auto-restart capture failed: {e:#}");
            }
        });
        let body = if platform_changed {
            "Applied platform / capture / proxy config changes."
        } else {
            "Applied capture / proxy config changes."
        };
        let _ = state
            .notify_bus
            .send(Notification::info("Capture restarted").body(body));
    } else {
        let _ = state.notify_bus.send(
            Notification::success("Config saved")
                .body("Restart affected subsystems for changes to take effect."),
        );
    }

    // Hot-start the bot manager when bot.enabled flips false→true.
    // `bot_manager_started` is process-wide, so repeat false→true→false→true
    // toggles only spawn once. The manager runs forever; flipping back to
    // false still requires an Akagi relaunch to actually stop it.
    if claim_bot_manager_spawn(bot_now_enabled, &state.bot_manager_started) {
        let mjai = state.mjai_bus.clone();
        let resp = state.bot_response_bus.clone();
        let bs = state.bot_status_bus.clone();
        let nb = state.notify_bus.clone();
        let inspector = state.log_session.inspector();
        let rt = state.runtime.clone();
        let syncs = state.syncs_in_flight.clone();
        let started_flag = state.bot_manager_started.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) =
                crate::bot::run_bot_manager(bot_cfg, mjai, resp, bs, nb, inspector, rt, syncs).await
            {
                tracing::error!("Bot manager failed: {e:#}");
                // Setup failure: clear the flag so a follow-up
                // update_config (e.g. the user fixing the bot dir and
                // saving again) gets another shot at spawning.
                started_flag.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        });
    }

    if claim_autoplay_manager_spawn(autoplay_now_enabled, &state.autoplay_manager_started) {
        let cfg_for_ap = state.config.clone();
        let ctx_for_ap = state.autoplay_context.clone();
        let tracker_for_ap = state.game_tracker.clone();
        let mjai_for_ap = state.mjai_bus.clone();
        let resp_for_ap = state.bot_response_bus.clone();
        let started_flag = state.autoplay_manager_started.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::autoplay::run_autoplay_manager(
                cfg_for_ap,
                ctx_for_ap,
                tracker_for_ap,
                mjai_for_ap,
                resp_for_ap,
            )
            .await
            {
                tracing::error!("Autoplay manager failed: {e:#}");
                started_flag.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn list_bots(state: State<'_, AppState>) -> CmdResult<Vec<BotInfo>> {
    let dir = state.config.read().await.bot.dir.clone();
    let resolved = resolve_dir(Path::new(&dir));
    let registry = BotRegistry::scan(&resolved).map_err(|e| format!("scan bots: {e:#}"))?;
    Ok(registry.entries().iter().map(entry_to_info).collect())
}

/// Read the merged settings (manifest + on-disk values) for one bot.
/// Returns an error when the bot does not exist or has no manifest —
/// frontend should hide the settings panel for manifest-less bots and
/// avoid calling this command for them.
#[tauri::command]
pub async fn get_bot_settings(name: String, state: State<'_, AppState>) -> CmdResult<BotSettings> {
    let dir = state.config.read().await.bot.dir.clone();
    let resolved = resolve_dir(Path::new(&dir));
    let registry = BotRegistry::scan(&resolved).map_err(|e| format!("scan bots: {e:#}"))?;
    let entry = registry
        .find(&name)
        .ok_or_else(|| format!("bot {name:?} not found"))?;
    let manifest = entry
        .manifest
        .clone()
        .ok_or_else(|| format!("bot {name:?} has no manifest.toml"))?;
    let values = manifest::load_values(&entry.dir, &manifest)
        .map_err(|e| format!("load settings: {e:#}"))?;
    Ok(BotSettings { manifest, values })
}

/// Persist user-edited settings for one bot. Validates the values against
/// the manifest before writing — wrong type, out-of-range numeric, and
/// unknown enum choice all surface as command errors.
///
/// New values take effect on the next bot spawn (i.e. the next
/// `start_game` event). The currently-running subprocess keeps its old
/// values; document this caveat in the UI.
#[tauri::command]
pub async fn update_bot_settings(
    name: String,
    values: BTreeMap<String, serde_json::Value>,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let dir = state.config.read().await.bot.dir.clone();
    let resolved = resolve_dir(Path::new(&dir));
    let registry = BotRegistry::scan(&resolved).map_err(|e| format!("scan bots: {e:#}"))?;
    let entry = registry
        .find(&name)
        .ok_or_else(|| format!("bot {name:?} not found"))?;
    let manifest = entry
        .manifest
        .as_ref()
        .ok_or_else(|| format!("bot {name:?} has no manifest.toml"))?;
    manifest::save_values(&entry.dir, manifest, &values)
        .map_err(|e| format!("save settings: {e:#}"))?;
    let _ = state
        .notify_bus
        .send(Notification::success(format!("{name} settings saved")));
    Ok(())
}

/// Update the active bot for a given mode (`"4p"` or `"3p"`) in config +
/// persist. Doesn't restart the running `BotManager` — that respawns on the
/// next `start_game` event anyway, and picks the matching mode bot then.
///
/// Empty `name` clears that mode's active bot (analysis-only in that mode).
#[tauri::command]
pub async fn set_active_bot(
    mode: String,
    name: String,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    {
        let mut cfg = state.config.write().await;
        match mode.as_str() {
            "4p" => cfg.bot.active_4p = name.clone(),
            "3p" => cfg.bot.active_3p = name.clone(),
            other => return Err(format!("unknown mode {other:?}; expected \"4p\" or \"3p\"")),
        }
        persist_config(&cfg, &state.config_path).map_err(|e| e.to_string())?;
    }
    let label = if name.is_empty() {
        format!("{mode} bot cleared")
    } else {
        format!("Active {mode} bot set to {name}")
    };
    let _ = state.notify_bus.send(Notification::success(label));
    Ok(())
}

/// Install a bot by downloading the latest release zip from a GitHub
/// repository. Refuses to overwrite an existing `mjai_bot/<name>/` —
/// the user must remove it first via the file browser. The installer
/// reports progress through `NotifyBus` with sticky id
/// `bot-install-<name>`.
#[tauri::command]
pub async fn install_bot_from_github(
    repo: String,
    asset_glob: Option<String>,
    name: Option<String>,
    state: State<'_, AppState>,
) -> CmdResult<BotInfo> {
    let dir = state.config.read().await.bot.dir.clone();
    let resolved = resolve_dir(Path::new(&dir));
    std::fs::create_dir_all(&resolved)
        .map_err(|e| format!("create bot dir {}: {e}", resolved.display()))?;

    let spec = GithubInstallSpec {
        repo,
        asset_glob,
        name,
    };
    let entry = install::install_from_github_release(
        spec,
        &resolved,
        &state.notify_bus,
        state.runtime.as_ref(),
    )
    .await
    .map_err(|e| format!("install: {e:#}"))?;
    Ok(entry_to_info(&entry))
}

/// Reinstall a bot from the GitHub source declared in its existing
/// `manifest.toml`. Removes the current install first.
#[tauri::command]
pub async fn update_bot_from_manifest(
    name: String,
    state: State<'_, AppState>,
) -> CmdResult<BotInfo> {
    let dir = state.config.read().await.bot.dir.clone();
    let resolved = resolve_dir(Path::new(&dir));
    let registry = BotRegistry::scan(&resolved).map_err(|e| format!("scan bots: {e:#}"))?;
    let entry = registry
        .find(&name)
        .ok_or_else(|| format!("bot {name:?} not found"))?;
    let manifest = entry
        .manifest
        .as_ref()
        .ok_or_else(|| format!("bot {name:?} has no manifest.toml"))?;
    let source = manifest
        .source
        .as_ref()
        .ok_or_else(|| format!("bot {name:?} manifest has no [bot.source] block"))?;

    let (repo, asset_glob) = match source {
        BotSource::GithubRelease { repo, asset_glob } => (repo.clone(), asset_glob.clone()),
    };

    std::fs::remove_dir_all(&entry.dir)
        .map_err(|e| format!("remove old install {}: {e}", entry.dir.display()))?;

    let spec = GithubInstallSpec {
        repo,
        asset_glob,
        name: Some(name.clone()),
    };
    let new_entry = install::install_from_github_release(
        spec,
        &resolved,
        &state.notify_bus,
        state.runtime.as_ref(),
    )
    .await
    .map_err(|e| format!("install: {e:#}"))?;
    Ok(entry_to_info(&new_entry))
}

/// Re-run `uv sync` for an installed bot. Frontend wires this to the
/// "Reinstall environment" button under Configure. `force=true` wipes
/// `.akagi/synced.stamp` and `.akagi/venv` first so a corrupted venv is
/// rebuilt from scratch (incremental sync can otherwise mask the breakage).
/// Reports progress + outcome through `NotifyBus` with sticky id
/// `bot-sync-<name>`. Refuses to start a second concurrent sync for the
/// same bot.
#[tauri::command]
pub async fn sync_bot_deps(name: String, force: bool, state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.config.read().await.bot.dir.clone();
    let resolved = resolve_dir(Path::new(&dir));
    let registry = BotRegistry::scan(&resolved).map_err(|e| format!("scan bots: {e:#}"))?;
    let entry = registry
        .find(&name)
        .ok_or_else(|| format!("bot {name:?} not found"))?
        .clone();
    let runtime = state.runtime.as_ref().ok_or_else(|| {
        "Python runtime not available — install python3 and uv on PATH".to_string()
    })?;

    let _guard = SyncGuard::acquire(&state.syncs_in_flight, &name)
        .await
        .ok_or_else(|| format!("sync already in progress for {name}"))?;

    let notify_id = format!("bot-sync-{name}");
    let _ = state.notify_bus.send(
        Notification::info(format!("Syncing {name}"))
            .body("Rebuilding Python environment (uv sync)…")
            .sticky()
            .id(notify_id.clone()),
    );

    if force {
        runtime::reset_sync_state(&entry.dir).await;
    }

    match runtime.ensure_synced(&entry.dir).await {
        Ok(()) => {
            let _ = state
                .notify_bus
                .send(Notification::success(format!("{name} environment ready")).id(notify_id));
            Ok(())
        }
        Err(e) => {
            let msg = format!("uv sync failed: {e:#}");
            let _ = state.notify_bus.send(
                Notification::error(format!("Sync failed for {name}"))
                    .body(msg.clone())
                    .id(notify_id),
            );
            Err(msg)
        }
    }
}

/// Start the capture backend selected by `cfg.capture.mode`. No-op
/// (returns Err) when one is already running — call `restart_capture`
/// instead if you want to swap.
#[tauri::command]
pub async fn start_capture(state: State<'_, AppState>) -> CmdResult<()> {
    let already_running = {
        let ctl = state.capture_control.lock().await;
        ctl.stop.is_some()
    };
    if already_running {
        return Err("capture backend already running".into());
    }
    spawn_capture_supervisor((*state).clone())
        .await
        .map_err(|e| format!("start capture: {e:#}"))
}

/// Tear down the running backend and start a fresh one. Used by the
/// Settings "Restart capture" button and by `update_config` whenever a
/// capture-affecting field changed. Safe to call when nothing is
/// running (becomes a plain start).
#[tauri::command]
pub async fn restart_capture(state: State<'_, AppState>) -> CmdResult<()> {
    restart_capture_inner((*state).clone())
        .await
        .map_err(|e| format!("restart capture: {e:#}"))
}

/// Probe the system for installed Chromium-family browsers. Surface in the
/// Settings UI so the user can pick which executable to launch.
#[tauri::command]
pub async fn detect_system_chrome(
) -> CmdResult<Vec<crate::capture::chromium::detect::DetectedBrowser>> {
    Ok(crate::capture::chromium::detect::detect_system_browsers())
}

/// List Chrome-for-Testing versions currently installed under
/// `<user_config_root>/chrome-for-testing/`. Newest first. Empty when
/// nothing is installed or the platform isn't supported by CfT.
#[tauri::command]
pub async fn list_cft_installed() -> CmdResult<Vec<String>> {
    Ok(crate::capture::chromium::cft::list_installed())
}

/// Download + extract Chrome-for-Testing for the current platform.
/// `channel` is interpreted as: `"stable"` / `"beta"` / `"dev"` /
/// `"canary"` (channel pin) or any literal version string (e.g.
/// `"131.0.6778.85"`). Empty string ≡ `"stable"`. Returns the
/// installed version.
///
/// Progress is reported through `NotifyBus` with sticky id
/// `capture-cft-download` so the frontend can show a single live toast.
#[tauri::command]
pub async fn download_chrome_for_testing(
    channel: Option<String>,
    state: State<'_, AppState>,
) -> CmdResult<String> {
    let raw = channel.unwrap_or_default();
    let parsed = crate::capture::chromium::cft::Channel::parse(&raw);
    crate::capture::chromium::cft::install(&parsed, &state.notify_bus)
        .await
        .map_err(|e| format!("install Chrome for Testing: {e:#}"))
}

/// Remove an installed Chrome-for-Testing version. No-op when the
/// version isn't installed.
#[tauri::command]
pub async fn remove_chrome_for_testing(
    version: String,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    if version.is_empty()
        || version.contains('/')
        || version.contains('\\')
        || version.contains("..")
    {
        return Err(format!("invalid CfT version {version:?}"));
    }
    crate::capture::chromium::cft::remove(&version)
        .map_err(|e| format!("remove Chrome for Testing: {e:#}"))?;
    let _ = state.notify_bus.send(Notification::success(format!(
        "Removed Chrome for Testing {version}"
    )));
    Ok(())
}

/// Stop the running capture backend. Kicks in-flight WebSocket flows
/// (MITM mode) and signals the supervisor to tear down. Returns Err if
/// nothing is running.
#[tauri::command]
pub async fn stop_capture(state: State<'_, AppState>) -> CmdResult<()> {
    let (stop, force_close) = {
        let mut ctl = state.capture_control.lock().await;
        (ctl.stop.take(), ctl.force_close.clone())
    };
    // Kick in-flight WS flows first so the game client actually
    // disconnects. Without this, hudsucker's graceful shutdown only
    // blocks new connections; existing ones drain naturally and the
    // user sees comm "still working" even after stop. (Chromium backend
    // ignores this — its shutdown closes the browser process directly.)
    force_close.notify_waiters();
    match stop {
        Some(tx) => {
            // Receiver dropped means the task already exited — that's fine,
            // we still cleared `stop` and the status forwarder will catch
            // up via the next CaptureStatus emission.
            let _ = tx.send(());
            Ok(())
        }
        None => Err("capture backend is not running".into()),
    }
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> CmdResult<Snapshot> {
    let config = state.config.read().await.clone();
    let bot_status = state.bot_status.read().await.clone();
    let capture_status = state.capture_control.lock().await.status.clone();
    let log_dir = state.log_session.dir().to_path_buf();
    Ok(Snapshot {
        config,
        bot_status,
        capture_status,
        log_dir,
    })
}

/// One-shot read of the latest [`crate::schema::CaptureStatus`]. Cheaper
/// than `get_status` when the caller only needs the capture lifecycle.
#[tauri::command]
pub async fn get_capture_status(
    state: State<'_, AppState>,
) -> CmdResult<crate::schema::CaptureStatus> {
    Ok(state.capture_control.lock().await.status.clone())
}

#[tauri::command]
pub async fn get_log_dir(state: State<'_, AppState>) -> CmdResult<PathBuf> {
    Ok(state.log_session.dir().to_path_buf())
}

/// Best-effort "open this folder in the OS file manager". Spawns the
/// platform-native opener and returns immediately — we don't wait on the
/// child, so a missing tool surfaces only if `spawn` itself fails. The
/// frontend already wraps this in try/catch.
#[tauri::command]
pub async fn open_log_folder(
    session: Option<String>,
    state: State<'_, AppState>,
) -> CmdResult<()> {
    let target = match session {
        Some(name) if !name.is_empty() => {
            // Defense-in-depth: only allow the canonical session name
            // shape — no `..`, no separators — so a malicious frontend
            // can't redirect this to anywhere else on disk.
            if !is_session_name(&name) {
                return Err(format!("invalid session name {name:?}"));
            }
            state.log_session.root().join(name)
        }
        _ => state.log_session.dir().to_path_buf(),
    };
    open_path(&target)
}

fn open_path(path: &Path) -> CmdResult<()> {
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";

    std::process::Command::new(cmd)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open path {}: {e}", path.display()))
}

/// Opens an `http(s)://` URL in the user's default browser. Used by the
/// first-run wizard for the GitHub / Discord links — Tauri 2's webview
/// won't reliably honour `target="_blank"` without the opener plugin, so
/// we route the click through the OS's native handler ourselves.
/// Validates the scheme to keep this from being abused as a generic
/// process spawn.
#[tauri::command]
pub async fn open_external_url(url: String) -> CmdResult<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("refused non-http(s) url: {url}"));
    }
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "windows")]
    let cmd = "explorer";

    std::process::Command::new(cmd)
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open url {url}: {e}"))
}

/// Strict matcher for session directory names — `YYYYMMDD-HHMMSS`. Used
/// both to filter `list_log_sessions` output and to validate user-
/// supplied session names before they touch the filesystem.
fn is_session_name(name: &str) -> bool {
    if name.len() != 15 {
        return false;
    }
    let bytes = name.as_bytes();
    bytes
        .iter()
        .enumerate()
        .all(|(i, &b)| if i == 8 { b == b'-' } else { b.is_ascii_digit() })
}

/// List every session directory under the active log root. Newest first.
/// `is_active` marks the session this process is currently writing — the
/// UI uses it to enable live tail.
#[tauri::command]
pub async fn list_log_sessions(state: State<'_, AppState>) -> CmdResult<Vec<LogSessionInfo>> {
    let active_name = state
        .log_session
        .dir()
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let root = state.log_session.root().to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<Vec<LogSessionInfo>, String> {
        let mut out = Vec::new();
        let rd = match std::fs::read_dir(&root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(format!("read_dir {}: {e}", root.display())),
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_session_name(&name) {
                continue;
            }
            let path = entry.path();
            // Cheap directory size: sum file sizes in the immediate dir.
            // Sub-directories (per-platform flow logs) are NOT walked —
            // for the UI it's enough to be a rough indicator, and a
            // recursive walk on a session with thousands of game files
            // would block the picker.
            let mut size_bytes: u64 = 0;
            if let Ok(rd2) = std::fs::read_dir(&path) {
                for f in rd2.flatten() {
                    if let Ok(m) = f.metadata() {
                        if m.is_file() {
                            size_bytes += m.len();
                        }
                    }
                }
            }
            let mtime_ms = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let is_active = name == active_name;
            out.push(LogSessionInfo {
                name,
                path,
                size_bytes,
                mtime_ms,
                is_active,
            });
        }
        out.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(out)
    })
    .await
    .map_err(|e| format!("list_log_sessions join: {e}"))?
}

/// Read filtered, paginated entries from one session's `all.jsonl`.
///
/// Filtering is server-side so a frontend opening a 100k-line session can
/// still ask "just the errors" without dragging the whole file across the
/// wire. The reader is line-oriented — one bad line is counted under
/// `skipped_malformed` and skipped, so a partial trailing line in the
/// active session (still being written) doesn't poison the response.
/// `limit` is capped at 2000 to bound payload size.
#[tauri::command]
pub async fn read_log_session(
    req: ReadLogRequest,
    state: State<'_, AppState>,
) -> CmdResult<ReadLogResponse> {
    if !is_session_name(&req.session) {
        return Err(format!("invalid session name {:?}", req.session));
    }
    let root = state.log_session.root().to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<ReadLogResponse, String> {
        let path = root.join(&req.session).join("all.jsonl");
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadLogResponse {
                    entries: Vec::new(),
                    has_more: false,
                    skipped_malformed: 0,
                });
            }
            Err(e) => return Err(format!("open {}: {e}", path.display())),
        };
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead;

        let level_set: Option<std::collections::HashSet<String>> =
            req.levels.as_ref().map(|v| v.iter().cloned().collect());
        let target_filters: Vec<String> = req.targets.unwrap_or_default();
        let search_lc = req.search.as_deref().map(|s| s.to_lowercase());
        // 0 → use a sane default (1000); otherwise hard-cap at 2000.
        let limit = if req.limit == 0 { 1000 } else { req.limit.min(2000) };

        let mut skipped_malformed: u32 = 0;
        let mut total_match: usize = 0;
        let mut entries: Vec<LogEntry> = Vec::with_capacity(limit);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: LogEntry = match serde_json::from_str(trimmed) {
                Ok(e) => e,
                Err(_) => {
                    skipped_malformed += 1;
                    continue;
                }
            };
            if let Some(ref ls) = level_set {
                if !ls.contains(&entry.level) {
                    continue;
                }
            }
            if !target_filters.is_empty()
                && !target_filters.iter().any(|t| entry.target.starts_with(t))
            {
                continue;
            }
            if let Some(ref s) = search_lc {
                if !entry.message.to_lowercase().contains(s) {
                    continue;
                }
            }
            total_match += 1;
            if total_match > req.offset && entries.len() < limit {
                entries.push(entry);
            }
        }

        let has_more = total_match > req.offset + entries.len();
        Ok(ReadLogResponse {
            entries,
            has_more,
            skipped_malformed,
        })
    })
    .await
    .map_err(|e| format!("read_log_session join: {e}"))?
}

/// Read filtered, paginated entries from a session's `inspector.jsonl`.
/// Same line-oriented, fault-tolerant reader as `read_log_session` —
/// malformed lines bumped under `skipped_malformed`, `limit` capped at
/// 2000. Filtering is server-side so a session with hundreds of
/// thousands of events doesn't have to cross the wire to be narrowed.
#[tauri::command]
pub async fn read_inspector(
    req: ReadInspectorRequest,
    state: State<'_, AppState>,
) -> CmdResult<ReadInspectorResponse> {
    if !is_session_name(&req.session) {
        return Err(format!("invalid session name {:?}", req.session));
    }
    let root = state.log_session.root().to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<ReadInspectorResponse, String> {
        let path = root.join(&req.session).join("inspector.jsonl");
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadInspectorResponse {
                    entries: Vec::new(),
                    has_more: false,
                    skipped_malformed: 0,
                });
            }
            Err(e) => return Err(format!("open {}: {e}", path.display())),
        };
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead;

        let kind_set: Option<std::collections::HashSet<String>> =
            req.kinds.as_ref().map(|v| v.iter().cloned().collect());
        let actor = req.actor;
        let search_lc = req.search.as_deref().map(|s| s.to_lowercase());
        let limit = if req.limit == 0 { 1000 } else { req.limit.min(2000) };

        let mut skipped_malformed: u32 = 0;
        let mut total_match: usize = 0;
        let mut entries: Vec<InspectorEntry> = Vec::with_capacity(limit);

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: InspectorEntry = match serde_json::from_str(trimmed) {
                Ok(e) => e,
                Err(_) => {
                    skipped_malformed += 1;
                    continue;
                }
            };
            if !inspector_matches(&entry, kind_set.as_ref(), actor, search_lc.as_deref()) {
                continue;
            }
            total_match += 1;
            if total_match > req.offset && entries.len() < limit {
                entries.push(entry);
            }
        }

        let has_more = total_match > req.offset + entries.len();
        Ok(ReadInspectorResponse {
            entries,
            has_more,
            skipped_malformed,
        })
    })
    .await
    .map_err(|e| format!("read_inspector join: {e}"))?
}

/// Predicate factored out of `read_inspector` so the filtering rules are
/// in one place (and easy to test, when we add tests for the inspector
/// pipeline). `kind_set` matches against the `kind` discriminant; `actor`
/// matches mjai events with an `actor` field and bot reactions by
/// `actor_id`; ws frames are unaffected by the actor filter (they're
/// pre-bridge, no actor concept yet) — they only drop out via `kinds`.
fn inspector_matches(
    entry: &InspectorEntry,
    kind_set: Option<&std::collections::HashSet<String>>,
    actor: Option<u8>,
    search_lc: Option<&str>,
) -> bool {
    let kind = match entry {
        InspectorEntry::WsFrame { .. } => "ws_frame",
        InspectorEntry::MjaiEvent { .. } => "mjai_event",
        InspectorEntry::BotReaction { .. } => "bot_reaction",
    };
    if let Some(ks) = kind_set {
        if !ks.contains(kind) {
            return false;
        }
    }
    if let Some(a) = actor {
        match entry {
            InspectorEntry::WsFrame { .. } => {} // pre-bridge, no actor
            InspectorEntry::MjaiEvent { event, .. } => {
                if let Some(ev_actor) = mjai_event_actor(event) {
                    if ev_actor != a {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            InspectorEntry::BotReaction { reaction, .. } => {
                if reaction.actor_id != a {
                    return false;
                }
            }
        }
    }
    if let Some(s) = search_lc {
        let hay = match entry {
            InspectorEntry::WsFrame { raw, parsed, .. } => {
                let mut buf = match raw {
                    crate::schema::FrameRaw::Text(t) => t.to_lowercase(),
                    crate::schema::FrameRaw::Binary(b) => b.to_lowercase(),
                };
                if let Some(p) = parsed {
                    buf.push(' ');
                    buf.push_str(&p.method.to_lowercase());
                    buf.push(' ');
                    buf.push_str(&p.args.to_string().to_lowercase());
                }
                buf
            }
            InspectorEntry::MjaiEvent { event, .. } => serde_json::to_string(event)
                .unwrap_or_default()
                .to_lowercase(),
            InspectorEntry::BotReaction { reaction, .. } => serde_json::to_string(reaction)
                .unwrap_or_default()
                .to_lowercase(),
        };
        if !hay.contains(s) {
            return false;
        }
    }
    true
}

fn mjai_event_actor(event: &crate::schema::MjaiEvent) -> Option<u8> {
    use crate::schema::MjaiEvent as E;
    match event {
        E::Tsumo { actor, .. }
        | E::Dahai { actor, .. }
        | E::Chi { actor, .. }
        | E::Pon { actor, .. }
        | E::Daiminkan { actor, .. }
        | E::Kakan { actor, .. }
        | E::Ankan { actor, .. }
        | E::Reach { actor, .. }
        | E::ReachAccepted { actor }
        | E::Hora { actor, .. }
        | E::Kita { actor, .. } => Some(*actor),
        _ => None,
    }
}

/// Subscribe to live inspector events. Same shape and lossy semantics as
/// `subscribe_log_events`. The Logs → Inspector tab uses this for the
/// active session's live tail.
#[tauri::command]
pub async fn subscribe_inspector(
    state: State<'_, AppState>,
    on_event: tauri::ipc::Channel<InspectorEntry>,
) -> CmdResult<()> {
    let mut rx = state.log_session.subscribe_inspector();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(entry) => {
                    let _ = on_event.send(entry);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // Inject a synthetic mjai-event-shaped warn marker so
                    // the UI sees the gap. We piggyback on MjaiEvent::None
                    // — the inspector tab knows to render dropped-event
                    // markers when it sees `none` interleaved.
                    let warn = InspectorEntry::MjaiEvent {
                        ts_ms: chrono::Local::now().timestamp_millis(),
                        event: crate::schema::MjaiEvent::None,
                    };
                    let _ = on_event.send(warn);
                    tracing::warn!(
                        target: "akagi.inspector",
                        "inspector forwarder dropped {n} entries"
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    Ok(())
}

/// Subscribe to live log events. Each event the active session emits is
/// forwarded over the supplied `tauri::ipc::Channel`. Slow consumers
/// surface a synthetic `WARN akagi.logger "dropped N events…"` so the
/// UI can show the gap explicitly. The forwarder task lives until the
/// broadcast is closed (process shutdown).
#[tauri::command]
pub async fn subscribe_log_events(
    state: State<'_, AppState>,
    on_event: tauri::ipc::Channel<LogEntry>,
) -> CmdResult<()> {
    let mut rx = state.log_session.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(entry) => {
                    let _ = on_event.send(entry);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    let warn = LogEntry {
                        ts_ms: chrono::Local::now().timestamp_millis(),
                        level: "WARN".into(),
                        target: "akagi.logger".into(),
                        file: None,
                        line: None,
                        message: format!("dropped {n} log events (consumer too slow)"),
                        fields: std::collections::HashMap::new(),
                    };
                    let _ = on_event.send(warn);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    Ok(())
}

/// Latest analysis output. `None` until the analysis runner has produced
/// at least one result for the current game.
#[tauri::command]
pub async fn get_analysis(state: State<'_, AppState>) -> CmdResult<Option<AnalysisResult>> {
    Ok(state.analysis_cache.read().await.clone())
}

/// Live game-state snapshot from the tracker. `None` before any
/// `start_game` event has been observed.
#[tauri::command]
pub async fn get_game_snapshot(state: State<'_, AppState>) -> CmdResult<Option<GameStateSnapshot>> {
    Ok(state.game_tracker.lock().await.snapshot())
}

/// Score a hypothetical bot hora (ron or tsumo) against the live engine
/// state. The bot's own `hora` mjai response carries no score data
/// (`deltas` is only populated on the inbound mjai event from the platform
/// after the win is confirmed). The frontend's `BotActionTile` calls this
/// to display "+8000點" beside the action label.
///
/// Returns `None` when no game is in progress, when the actor's hand isn't
/// a valid agari shape, or when the winning tile can't be inferred from
/// the live state (no recent discard for ron / no recent draw for tsumo).
#[tauri::command]
pub async fn compute_bot_hora_score(
    actor: u8,
    is_tsumo: bool,
    state: State<'_, AppState>,
) -> CmdResult<Option<HoraScoreInfo>> {
    Ok(state
        .game_tracker
        .lock()
        .await
        .evaluate_hora(actor, is_tsumo))
}

/// Pre-encoded mahgen DSL strings ready for the frontend `<mah-gen>`
/// element. Built from the same snapshot as `get_game_snapshot` — call
/// whichever the UI surface prefers; both are O(34 tiles) to generate.
#[tauri::command]
pub async fn get_mahgen_view(state: State<'_, AppState>) -> CmdResult<Option<MahgenView>> {
    Ok(state
        .game_tracker
        .lock()
        .await
        .snapshot()
        .map(|s| MahgenView::from_snapshot(&s)))
}

/// Remove a bot's directory under `bot.dir/<name>/`. Refuses to delete
/// the currently-active bot — user must `set_active_bot` to a different
/// one first. Refuses target paths that escape `bot.dir` (defense in
/// depth even though `name` came from the bot list, not raw user input).
#[tauri::command]
pub async fn delete_bot(name: String, state: State<'_, AppState>) -> CmdResult<()> {
    let (active_4p, active_3p, dir) = {
        let cfg = state.config.read().await;
        (
            cfg.bot.active_4p.clone(),
            cfg.bot.active_3p.clone(),
            cfg.bot.dir.clone(),
        )
    };
    if active_4p == name || active_3p == name {
        return Err(format!(
            "{name:?} is an active bot (4p or 3p) — switch to a different bot first"
        ));
    }
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("invalid bot name {name:?}"));
    }
    let resolved_root = resolve_dir(Path::new(&dir));
    let target = resolved_root.join(&name);
    if !target.is_dir() {
        return Err(format!("bot {name:?} not found at {}", target.display()));
    }
    let canon_root = std::fs::canonicalize(&resolved_root)
        .map_err(|e| format!("canonicalize {}: {e}", resolved_root.display()))?;
    let canon_target = std::fs::canonicalize(&target)
        .map_err(|e| format!("canonicalize {}: {e}", target.display()))?;
    if !canon_target.starts_with(&canon_root) {
        return Err(format!("bot {name:?} resolves outside the bot directory"));
    }
    std::fs::remove_dir_all(&canon_target)
        .map_err(|e| format!("remove {}: {e}", canon_target.display()))?;
    let _ = state
        .notify_bus
        .send(Notification::success(format!("Deleted bot {name}")));
    Ok(())
}

// ---------- Game history ----------
//
// Reads/writes are delegated to `state.history_store`. All errors bubble
// up as user-readable strings (the store error chain via anyhow has the
// detail we need; `:#` prints the full chain).

/// Filtered, paginated listing of finalised games. Newest-first by
/// `started_at`. `limit == 0` means use the store's default cap.
#[tauri::command]
pub async fn list_game_history(
    filter: Option<HistoryFilter>,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> CmdResult<Vec<GameRecord>> {
    let store = state.history_store.clone();
    let filter = filter.unwrap_or_default();
    let limit = limit.unwrap_or(0);
    let offset = offset.unwrap_or(0);
    tokio::task::spawn_blocking(move || store.list(&filter, limit, offset))
        .await
        .map_err(|e| format!("history list join error: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

/// Single record by id.
#[tauri::command]
pub async fn get_game_history_record(
    id: String,
    state: State<'_, AppState>,
) -> CmdResult<Option<GameRecord>> {
    let store = state.history_store.clone();
    tokio::task::spawn_blocking(move || store.get(&id))
        .await
        .map_err(|e| format!("history get join error: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

/// Full mjai event stream for a recorded game. `None` if the id is unknown.
#[tauri::command]
pub async fn get_game_history_events(
    id: String,
    state: State<'_, AppState>,
) -> CmdResult<Option<HistoryEventLog>> {
    let store = state.history_store.clone();
    tokio::task::spawn_blocking(move || store.get_events(&id))
        .await
        .map_err(|e| format!("history get_events join error: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

/// Delete a recorded game (its index entry + games/<id>.mjai.jsonl).
/// Returns true if a record was actually removed. Emits a
/// `HistoryEvent::Deleted` on the history bus so the frontend can drop
/// the row from its cache without a refetch.
#[tauri::command]
pub async fn delete_game_history_entry(
    id: String,
    state: State<'_, AppState>,
) -> CmdResult<bool> {
    let store = state.history_store.clone();
    let id_for_blocking = id.clone();
    let removed = tokio::task::spawn_blocking(move || store.delete(&id_for_blocking))
        .await
        .map_err(|e| format!("history delete join error: {e}"))?
        .map_err(|e| format!("{e:#}"))?;
    if removed {
        let _ = state.history_bus.send(HistoryEvent::Deleted { id });
    }
    Ok(removed)
}

fn persist_config(config: &AppConfig, path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let body = toml::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(path, body)
}

/// Pre-builds the handler list for `tauri::generate_handler!`. Keep in
/// sync when adding commands.
#[macro_export]
macro_rules! ipc_handlers {
    () => {
        ::tauri::generate_handler![
            $crate::ipc::commands::get_config,
            $crate::ipc::commands::update_config,
            $crate::ipc::commands::list_bots,
            $crate::ipc::commands::set_active_bot,
            $crate::ipc::commands::get_bot_settings,
            $crate::ipc::commands::update_bot_settings,
            $crate::ipc::commands::install_bot_from_github,
            $crate::ipc::commands::update_bot_from_manifest,
            $crate::ipc::commands::sync_bot_deps,
            $crate::ipc::commands::delete_bot,
            $crate::ipc::commands::start_capture,
            $crate::ipc::commands::stop_capture,
            $crate::ipc::commands::restart_capture,
            $crate::ipc::commands::get_capture_status,
            $crate::ipc::commands::detect_system_chrome,
            $crate::ipc::commands::list_cft_installed,
            $crate::ipc::commands::download_chrome_for_testing,
            $crate::ipc::commands::remove_chrome_for_testing,
            $crate::ipc::commands::get_status,
            $crate::ipc::commands::get_log_dir,
            $crate::ipc::commands::open_log_folder,
            $crate::ipc::commands::open_external_url,
            $crate::ipc::commands::list_log_sessions,
            $crate::ipc::commands::read_log_session,
            $crate::ipc::commands::subscribe_log_events,
            $crate::ipc::commands::read_inspector,
            $crate::ipc::commands::subscribe_inspector,
            $crate::ipc::commands::get_analysis,
            $crate::ipc::commands::get_game_snapshot,
            $crate::ipc::commands::get_mahgen_view,
            $crate::ipc::commands::compute_bot_hora_score,
            $crate::ipc::commands::list_game_history,
            $crate::ipc::commands::get_game_history_record,
            $crate::ipc::commands::get_game_history_events,
            $crate::ipc::commands::delete_game_history_entry,
        ]
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn persist_config_round_trips() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("nested").join("config.toml");
        let mut cfg = AppConfig::default();
        cfg.bot.active_4p = "mortal".into();
        cfg.bot.active_3p = "mortal_3p".into();
        cfg.proxy.addr = "127.0.0.1:9999".into();

        persist_config(&cfg, &path).unwrap();

        let body = std::fs::read_to_string(&path).unwrap();
        let back: AppConfig = toml::from_str(&body).unwrap();
        assert_eq!(back.bot.active_4p, "mortal");
        assert_eq!(back.bot.active_3p, "mortal_3p");
        assert_eq!(back.proxy.addr, "127.0.0.1:9999");
    }

    /// Regression: the first-run wizard ships a fresh-install Akagi with
    /// `bot.enabled = false` (defaults), then calls `update_config` to
    /// flip it to `true`. Before the fix the bot manager was never
    /// spawned in this process — the user had to relaunch the app.
    /// `claim_bot_manager_spawn` is the gate that makes
    /// `update_config` spawn the manager exactly once on that flip.
    #[test]
    fn claim_bot_manager_spawn_fires_once_on_false_to_true_flip() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let flag = AtomicBool::new(false);

        // bot.enabled still false (e.g. user saved an unrelated setting
        // before completing the wizard) — must not claim.
        assert!(!claim_bot_manager_spawn(false, &flag));
        assert!(!flag.load(Ordering::SeqCst));

        // Wizard finishes with bot.enabled = true — claim succeeds.
        assert!(claim_bot_manager_spawn(true, &flag));
        assert!(flag.load(Ordering::SeqCst));

        // Subsequent saves with bot.enabled still true — manager is
        // already running, must not double-spawn.
        assert!(!claim_bot_manager_spawn(true, &flag));
        assert!(flag.load(Ordering::SeqCst));

        // Toggling bot.enabled back to false then forward to true: the
        // running manager survives (no off-switch yet), so we still must
        // not claim a second time.
        assert!(!claim_bot_manager_spawn(false, &flag));
        assert!(!claim_bot_manager_spawn(true, &flag));
        assert!(flag.load(Ordering::SeqCst));
    }

    /// Regression: when startup spawns the manager (because `bot.enabled`
    /// was already true on first config load), the flag is set true
    /// up front. A subsequent `update_config` must observe that and skip
    /// spawning a duplicate manager.
    #[test]
    fn claim_bot_manager_spawn_respects_preset_flag() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let flag = AtomicBool::new(true);
        assert!(!claim_bot_manager_spawn(true, &flag));
        assert!(flag.load(Ordering::SeqCst));
    }
}
