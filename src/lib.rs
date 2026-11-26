pub mod analysis;
pub mod autoplay;
pub mod bot;
pub mod bridge;
pub mod capture;
pub mod cli;
pub mod config;
pub mod event_bus;
pub mod game_state;
pub mod github;
pub mod history;
pub mod inspector;
pub mod ipc;
pub mod logger;
pub mod platform;
pub mod proxy;
pub mod schema;
pub mod updater;
pub mod util;

use clap::Parser;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;
use tracing::{error, info, warn};

/// Everything the embedded game webview needs on the WebView2 side: where the
/// browser keeps its profile and which DevTools port to open.
///
/// This is *not* done through `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`:
/// wry always sets `additionalBrowserArgs` itself (to disable the built-in
/// PDF/UI overlays), and an explicit `additionalBrowserArgs` makes WebView2
/// ignore the environment variable entirely. The arguments have to be handed
/// to the builder instead — which is why the main window is created at
/// runtime rather than declaratively in `tauri.conf.json`.
#[derive(Debug, Clone)]
struct Webview2Setup {
    profile: std::path::PathBuf,
    /// `additionalBrowserArgs` for every `WebviewWindowBuilder` / `WebviewBuilder`.
    /// wry's defaults are included: passing our own replaces theirs. The
    /// embedded CDP port lives in here; `capture::embedded` got it separately
    /// via `set_endpoint`.
    args: String,
}

fn webview2_setup() -> Option<Webview2Setup> {
    if !cfg!(windows) {
        return None;
    }
    let profile = util::resolve_dir(Path::new("./webview-profile"));
    if let Err(e) = std::fs::create_dir_all(&profile) {
        warn!(
            "embedded: could not create webview profile dir {}: {e}",
            profile.display()
        );
    }
    // A stable port by preference: the WebView2 browser process outlives a
    // frontend hot-reload of the app window, and tools (or a second Akagi)
    // that remember the port keep working. Falls back to any free port when
    // 23411 is taken.
    const PREFERRED_PORT: u16 = 23411;
    let port = if std::net::TcpListener::bind(("127.0.0.1", PREFERRED_PORT)).is_ok() {
        PREFERRED_PORT
    } else {
        match capture::chromium::launch::pick_remote_debugging_port() {
            Ok(p) => p,
            Err(e) => {
                error!("embedded: could not reserve a DevTools port: {e:#}");
                return None;
            }
        }
    };
    // wry's defaults, which an explicit `additionalBrowserArgs` would
    // otherwise silently drop.
    let args = format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --remote-debugging-port={port}"
    );
    capture::embedded::set_endpoint(capture::embedded::Webview2Endpoint {
        port,
        user_data_dir: profile.clone(),
    });
    info!(
        "embedded: webview2 CDP on port {port}, profile {}",
        profile.display()
    );
    Some(Webview2Setup { profile, args })
}

pub fn run() {
    platform::setup();

    let args = cli::Cli::parse();
    let (cfg, config_path) = config::load_config(args.config.as_deref());

    // Embedded capture runs the game in a WebView2 inside our own window,
    // and WebView2 only opens a DevTools port when the browser arguments are
    // present **before** the first webview is created. That happens while
    // the app is being built, so the variables have to be set here — before
    // `tauri::Builder` — and the endpoint handed to `capture::embedded`.
    //
    // Gated on the mode: there is no reason to expose a debugging port on
    // our own UI when the game runs in an external browser or over MITM.
    // Only embedded capture needs the WebView2 DevTools port: there is no
    // reason to expose one on our own UI when the game runs elsewhere.
    let webview2 = if matches!(cfg.capture.mode, config::CaptureMode::Embedded) {
        webview2_setup()
    } else {
        None
    };

    let log_dir = util::resolve_dir(&cfg.logging.dir);
    let targets = [
        logger::LogTarget::new("proxy", "akagi::proxy"),
        logger::LogTarget::new("bot", "akagi::bot"),
    ];
    let session = match logger::init(
        &log_dir,
        &cfg.logging.level,
        &cfg.logging.all_level,
        &targets,
    ) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            // Read-only fs fallback: retry under user data dir.
            if let Some(fallback) =
                util::user_config_root().map(|r| r.join(util::strip_leading_dot(&cfg.logging.dir)))
            {
                if fallback != log_dir {
                    eprintln!(
                        "Logger init at {} failed: {e:?}. Retrying at {}",
                        log_dir.display(),
                        fallback.display()
                    );
                    match logger::init(
                        &fallback,
                        &cfg.logging.level,
                        &cfg.logging.all_level,
                        &targets,
                    ) {
                        Ok(s) => Arc::new(s),
                        Err(e2) => {
                            eprintln!("Failed to initialise logger: {e2:?}");
                            return;
                        }
                    }
                } else {
                    eprintln!("Failed to initialise logger: {e:?}");
                    return;
                }
            } else {
                eprintln!("Failed to initialise logger: {e:?}");
                return;
            }
        }
    };

    info!("Config loaded: {cfg:?}");
    info!("Log session at {}", session.dir().display());

    // All buses constructed up front so AppState owns the canonical
    // sender clones — every subsystem (bot manager, proxy supervisor,
    // ipc forwarders, future HUD) shares these handles via state.
    let mjai_bus = event_bus::mjai_bus();
    let bot_response_bus = event_bus::bot_response_bus();
    let bot_status_bus = event_bus::bot_status_bus();
    let capture_status_bus = event_bus::capture_status_bus();
    let notify_bus = event_bus::notify_bus();
    let analysis_bus = event_bus::analysis_bus();
    let post_tracker_bus = event_bus::post_tracker_bus();
    let history_bus = event_bus::history_bus();

    // Persistent game-history store. Lives under `<config_root>/history/`
    // by default — independent of the volatile session log dir. Failure
    // to create the dir is fatal: the recorder task and IPC commands
    // both depend on it.
    let history_root = util::resolve_dir(Path::new("./history"));
    let history_store = match history::HistoryStore::new(history_root.clone()) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            error!(
                "Failed to initialise history store at {}: {e:#}",
                history_root.display()
            );
            return;
        }
    };
    info!("History store at {}", history_root.display());

    let bot_enabled = cfg.bot.enabled;
    let proxy_enabled = cfg.proxy.enabled;
    let autoplay_enabled = cfg.autoplay.enabled;
    let overlay_cfg = cfg.overlay.clone();
    // Clone taken here, before `cfg` is moved into the setup closure: the
    // first layout pass needs it synchronously, and the tokio lock around
    // the live config must not be blocked on from the main thread.
    let layout_cfg = cfg.clone();

    // Game-state tracker handle is built up front so AppState can carry
    // the Arc, but the consumer task is spawned inside `.setup()` once
    // the Tauri Tokio runtime is live (sync `lib::run` has no runtime).
    let game_tracker = game_state::tracker::new_handle();
    let analysis_cache = std::sync::Arc::new(tokio::sync::RwLock::new(None));

    let tracker_rx = mjai_bus.subscribe();
    let tracker_post = post_tracker_bus.clone();
    let analysis_rx = post_tracker_bus.subscribe();
    let analysis_tracker = game_tracker.clone();
    let analysis_bus_for_runner = analysis_bus.clone();
    let analysis_cache_for_runner = analysis_cache.clone();

    let history_rx = mjai_bus.subscribe();
    let history_store_for_recorder = history_store.clone();
    let history_bus_for_recorder = history_bus.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // The overlay's geometry is still saved and restored, but not
                // by the plugin's automatic pass: that restores `StateFlags::all()`,
                // which would put decorations back on a deliberately frameless
                // window. `ipc::overlay::open` restores position+size itself.
                .skip_initial_state(ipc::overlay::LABEL)
                .build(),
        )
        .invoke_handler(crate::ipc_handlers!())
        .setup({
            // AppState constructed *inside* setup() so the python+uv
            // runtime can be located using `resource_dir` — bundled
            // binaries live under `<resource_dir>/runtime/...` and that
            // path is only resolvable once the AppHandle exists.
            move |app| {
                // The main window is created here rather than declaratively:
                // the embedded CDP port has to ride on `additionalBrowserArgs`
                // of the first webview created, and that can only be attached
                // by a builder (see `webview2_setup`).
                if let Some(opts) = &webview2 {
                    ipc::embedded_window::set_webview2_options(
                        ipc::embedded_window::Webview2Options {
                            profile: opts.profile.clone(),
                            args: opts.args.clone(),
                        },
                    );
                }
                let mut window_builder = tauri::WebviewWindowBuilder::new(
                    app,
                    "main",
                    tauri::WebviewUrl::App("index.html".into()),
                )
                .title("Akagi")
                .inner_size(1560.0, 980.0)
                .min_inner_size(760.0, 560.0);
                #[cfg(windows)]
                if let Some(opts) = &webview2 {
                    window_builder = window_builder
                        .data_directory(opts.profile.clone())
                        .additional_browser_args(&opts.args);
                }
                let main_window = window_builder.build()?;
                {
                    use tauri_plugin_window_state::{StateFlags, WindowExt};
                    if let Err(e) = main_window.restore_state(
                        StateFlags::POSITION | StateFlags::SIZE,
                    ) {
                        tracing::debug!("main window: no saved geometry to restore: {e}");
                    }
                }

                let resource_dir = app.path().resource_dir().ok();
                let runtime = bot::PythonRuntime::locate(resource_dir.as_deref()).ok();
                match &runtime {
                    Some(rt) => info!(
                        "bot runtime: python={} uv={} mode={:?}",
                        rt.python().display(),
                        rt.uv().display(),
                        rt.mode()
                    ),
                    None => warn!(
                        "no python3+uv runtime found (neither bundled nor on PATH); bot install/sync will be unavailable"
                    ),
                }

                let history_platform =
                    history::recorder::shared_platform(schema::Platform::from(cfg.platform.kind));

                let state = ipc::AppState::new(
                    cfg,
                    config_path,
                    session.clone(),
                    mjai_bus.clone(),
                    post_tracker_bus.clone(),
                    bot_response_bus.clone(),
                    bot_status_bus.clone(),
                    capture_status_bus.clone(),
                    notify_bus.clone(),
                    analysis_bus.clone(),
                    history_bus.clone(),
                    game_tracker,
                    analysis_cache,
                    history_store.clone(),
                    history_platform.clone(),
                    runtime.clone(),
                );

                ipc::install(app.handle(), state.clone())?;

                // Shape the window: panel on the right, toolbar on top, and
                // (in embedded mode) the game on the left. Before the
                // wizard completes this is a no-op and the full declarative
                // window stands.
                ipc::embedded_window::reconcile(app.handle(), &layout_cfg);

                // Re-lay-out on resize. The window owns three child webviews
                // whose bounds are absolute, so every resize has to push new
                // rectangles; the layout pass is idempotent and cheap.
                if let Some(window) = app.get_window("main") {
                    let handle = app.handle().clone();
                    window.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::Resized(_)) {
                            ipc::embedded_window::relayout(&handle);
                        }
                    });
                }

                // Reopen the suggestion overlay if it was left enabled. Safe
                // before any game data exists — it renders its empty state and
                // fills in on the first `bot-response`.
                ipc::overlay::reconcile(app.handle(), &overlay_cfg);

                // The overlay is an accessory window, not a co-equal one. Tauri
                // exits when *all* windows close, and the overlay counts — so
                // without this, closing the main window leaves the overlay on
                // screen holding the process open. It is `skip_taskbar` and
                // undecorated, which is right while Akagi is running and hostile
                // the moment it isn't: no taskbar entry, no title bar, nothing
                // that leads back to the process still alive behind the card.
                //
                // Close it with the main window and let Tauri exit on its own
                // once no windows remain — no `exit(0)`, so the window-state save
                // and every other shutdown path still runs. This closes the
                // *window*; it deliberately does not touch `overlay.enabled`, so
                // the overlay comes back on the next launch. Only a deliberate ×
                // (or the toggle) turns the feature off.
                if let Some(main) = app.get_webview_window("main") {
                    let handle = app.handle().clone();
                    main.on_window_event(move |event| {
                        if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                            if let Err(e) = ipc::overlay::close(&handle) {
                                warn!("overlay: could not close with the main window: {e}");
                            }
                        }
                    });
                }

                // Spawn tracker + analysis loops inside the Tauri Tokio
                // runtime — `lib::run` itself is sync.
                tauri::async_runtime::spawn(game_state::tracker::drive_loop(
                    state.game_tracker.clone(),
                    tracker_rx,
                    Some(tracker_post),
                ));
                tauri::async_runtime::spawn(analysis::runner::drive_loop(
                    analysis_rx,
                    analysis_tracker,
                    analysis_bus_for_runner,
                    analysis_cache_for_runner,
                ));

                // History recorder. Subscribes to the shared MjaiBus and
                // finalises one GameRecord per `EndGame`. The platform tag
                // is taken from the active bridge selection at startup;
                // changing platform at runtime requires a relaunch for the
                // history records to pick up the new tag.
                tauri::async_runtime::spawn(history::recorder::drive_loop(
                    history_store_for_recorder,
                    history_bus_for_recorder,
                    history_platform.clone(),
                    history_rx,
                ));

                if bot_enabled {
                    let cfg_for_bot = state.config.clone();
                    let events_for_bot = post_tracker_bus.clone();
                    let resp = bot_response_bus.clone();
                    let bs = bot_status_bus.clone();
                    let nb = notify_bus.clone();
                    let inspector = state.log_session.inspector();
                    let rt = runtime.clone();
                    let syncs = state.syncs_in_flight.clone();
                    state
                        .bot_manager_started
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = bot::run_bot_manager(
                            cfg_for_bot,
                            events_for_bot,
                            resp,
                            bs,
                            nb,
                            inspector,
                            rt,
                            syncs,
                        )
                        .await
                        {
                            error!("Bot manager failed: {e:#}");
                        }
                    });
                }

                // An untouched delay.lua from an older version becomes the
                // current default now, not at the first paced decision of a
                // game played with autoplay on.
                if let Some(dir) = state.config_path.parent() {
                    autoplay::delay::refresh_outdated_default(&dir.join("delay.lua"));
                }

                if autoplay_enabled {
                    let cfg_for_ap = state.config.clone();
                    let ctx_for_ap = state.autoplay_context.clone();
                    let tracker_for_ap = state.game_tracker.clone();
                    let mjai_for_ap = mjai_bus.clone();
                    let resp_for_ap = bot_response_bus.clone();
                    let notify_for_ap = notify_bus.clone();
                    // Default delay-script location is resolved relative
                    // to the loaded config file's directory.
                    let config_dir_for_ap = state
                        .config_path
                        .parent()
                        .map(std::path::Path::to_path_buf)
                        .unwrap_or_default();
                    state
                        .autoplay_manager_started
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = autoplay::run_autoplay_manager(
                            cfg_for_ap,
                            ctx_for_ap,
                            tracker_for_ap,
                            mjai_for_ap,
                            resp_for_ap,
                            notify_for_ap,
                            config_dir_for_ap,
                        )
                        .await
                        {
                            error!("Autoplay manager failed: {e:#}");
                        }
                    });
                }

                // Inspector mjai-bus subscriber: every event the bridges
                // emit is mirrored into the inspector timeline so the
                // user sees frames → mjai events → bot reactions in one
                // place. Spawned here (vs. inside Session::init) because
                // we need the Tauri Tokio runtime — Session is built sync.
                {
                    let mut rx = mjai_bus.subscribe();
                    let inspector = state.log_session.inspector();
                    tauri::async_runtime::spawn(async move {
                        loop {
                            match rx.recv().await {
                                Ok(event) => {
                                    inspector.record(crate::schema::InspectorEntry::MjaiEvent {
                                        ts_ms: chrono::Local::now().timestamp_millis(),
                                        event,
                                    });
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    // Lossy — the inspector file already
                                    // tells the durable story; the
                                    // real-time view tolerates gaps.
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                            }
                        }
                    });
                }

                if proxy_enabled {
                    let state_for_capture = state.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) =
                            ipc::capture_supervisor::spawn_capture_supervisor(state_for_capture)
                                .await
                        {
                            error!("Capture supervisor failed: {e:#}");
                        }
                    });
                }

                // Best-effort terminal ctrl_c → graceful capture shutdown.
                // GUI close goes through Tauri's window event path, not
                // here, so this only matters when the user runs Akagi
                // headless from a terminal.
                let stop_state = state.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    let stop = {
                        let mut ctl = stop_state.capture_control.lock().await;
                        ctl.stop.take()
                    };
                    if let Some(tx) = stop {
                        let _ = tx.send(());
                    }
                });

                Ok(())
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
