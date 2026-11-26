//! The single-window "browser + app" layout.
//!
//! In the shipped experience the game and Akagi's controls live in **one**
//! window: a slim toolbar across the top, the game on the left, and the
//! control panel down the right. Tauri supports this as a window with three
//! child webviews (`Window::add_child`, requiring the `unstable` feature):
//!
//! - `main`  — created declaratively from `tauri.conf.json`; becomes the
//!   right-hand panel (it already carries the full app UI and its
//!   capabilities).
//! - `top`   — the toolbar strip: mode switcher, status, window controls.
//! - `game`  — an **external** webview pointed at the platform's
//!   `start_url` (maj-soul.com / tenhou). No capabilities are scoped to it,
//!   so the game has no way to reach Tauri IPC.
//!
//! Capture attaches to the game webview over CDP (WebView2 honours
//! `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`; see `capture::embedded`), which
//! is what keeps the existing frame-bridge and autoplay code unchanged.
//!
//! Before the first-run wizard completes the layout is *not* applied: the
//! wizard needs the full declarative window with its native title bar.
//! [`reconcile`] runs at startup and on every config save, so finishing the
//! wizard reshapes the window in place.
//!
//! **Main-thread rule**: `add_child` takes the same route as
//! `WebviewWindowBuilder::build` — it posts the work to the event loop (and
//! runs it inline when already there). Calling it from a Tokio worker
//! deadlocks (see `ipc::overlay`), so everything here is scheduled with
//! `run_on_main_thread`.

use crate::config::AppConfig;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use tauri::{
    webview::WebviewBuilder, AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, WebviewUrl,
};
use tracing::{info, warn};

/// The latest config, kept in a `std` lock so the window-event (resize)
/// callback and the main-thread layout pass can read it without touching
/// tokio — those callbacks are not async tasks and must not block on one.
static SNAPSHOT: OnceLock<Arc<RwLock<AppConfig>>> = OnceLock::new();

/// WebView2 browser-process options every child webview must carry.
///
/// `additionalBrowserArgs` only takes effect when the webview is *created*,
/// and wry sets its own value — which is why the embedded CDP port has to
/// travel from `lib::run` into each builder here instead of an environment
/// variable.
#[derive(Debug, Clone)]
pub struct Webview2Options {
    pub profile: PathBuf,
    pub args: String,
}

static WEBVIEW2: OnceLock<Webview2Options> = OnceLock::new();

pub fn set_webview2_options(options: Webview2Options) {
    let _ = WEBVIEW2.set(options);
}

fn webview2() -> Option<&'static Webview2Options> {
    WEBVIEW2.get()
}

/// Apply the shared WebView2 profile + browser arguments to a **window**
/// builder (the overlay and the detached pages are windows of their own, not
/// children of `main`).
///
/// Every webview in the process must agree on these: a second WebView2
/// environment that differs only in its options sometimes fails outright
/// (`HRESULT 0x8007139F`, "class not registered"), and the failure mode is a
/// permanently blank window with a single line in the log.
pub fn apply_to_window<R: Runtime>(
    builder: tauri::WebviewWindowBuilder<'_, R, AppHandle<R>>,
) -> tauri::WebviewWindowBuilder<'_, R, AppHandle<R>> {
    #[cfg(windows)]
    if let Some(opts) = webview2() {
        return builder
            .data_directory(opts.profile.clone())
            .additional_browser_args(&opts.args);
    }
    builder
}

/// Apply the shared WebView2 profile + browser arguments to a child builder.
fn with_webview2<R: Runtime>(builder: WebviewBuilder<R>) -> WebviewBuilder<R> {
    #[cfg(windows)]
    if let Some(opts) = webview2() {
        return builder
            .data_directory(opts.profile.clone())
            .additional_browser_args(&opts.args);
    }
    builder
}

/// Toolbar strip height, logical px.
pub const TOP_HEIGHT: f64 = 42.0;
/// Right-hand panel width, logical px.
pub const PANEL_WIDTH: f64 = 320.0;
/// Floor for the whole window: below this the board stops being playable
/// next to the panel, so the minimum is enforced instead of squeezing
/// either side.
const MIN_WINDOW: (f64, f64) = (900.0, 600.0);

pub const TOP_LABEL: &str = "top";
pub const GAME_LABEL: &str = "game";

/// Lay the window out according to the live config. Idempotent; safe on
/// every save.
pub fn reconcile<R: Runtime>(app: &AppHandle<R>, cfg: &AppConfig) {
    // Keep the snapshot current for the resize path, then apply.
    let snapshot = SNAPSHOT.get_or_init(|| Arc::new(RwLock::new(cfg.clone())));
    if let Ok(mut guard) = snapshot.write() {
        *guard = cfg.clone();
    }
    relayout(app);
}

/// Re-apply the last known config — what the window-resize callback calls.
pub fn relayout<R: Runtime>(app: &AppHandle<R>) {
    let Some(snapshot) = SNAPSHOT.get() else {
        return;
    };
    let cfg = match snapshot.read() {
        Ok(g) => g.clone(),
        Err(_) => return,
    };
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply(&handle, &cfg)) {
        warn!("embedded-window: could not schedule layout on the main thread: {e}");
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, cfg: &AppConfig) {
    let Some(window) = app.get_window("main") else {
        warn!("embedded-window: no main window to lay out");
        return;
    };

    // The wizard owns the whole window until it completes.
    if !cfg.general.first_run_completed {
        return;
    }

    // Frameless, as in the design: the toolbar strip carries the title and
    // the window controls. Idempotent, so no "already applied" bookkeeping.
    if let Err(e) = window.set_decorations(false) {
        warn!("embedded-window: could not drop the title bar: {e}");
    }
    // The layout has three fixed pieces; rather than degrade it below the
    // point where the board is playable, hold the window at its floor.
    if let Err(e) = window.set_min_size(Some(LogicalSize::new(MIN_WINDOW.0, MIN_WINDOW.1))) {
        warn!("embedded-window: could not set the minimum window size: {e}");
    }

    let Ok(inner) = window.inner_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let (w, h) = (inner.width as f64 / scale, inner.height as f64 / scale);

    // Panel = the declarative `main` webview, narrowed to the right column.
    if let Some(main) = app.get_webview("main") {
        let rectangle = rect(w - PANEL_WIDTH, TOP_HEIGHT, PANEL_WIDTH, h - TOP_HEIGHT);
        if let Err(e) = main.set_bounds(rectangle) {
            warn!("embedded-window: could not place the panel webview: {e}");
        }
    }

    // Toolbar strip across the top.
    if app.get_webview(TOP_LABEL).is_none() {
        // No `auto_resize`: it makes the child fill the whole window, which is
        // the opposite of what a tiled child wants. Bounds are set explicitly
        // here and re-applied from the resize handler.
        let builder = with_webview2(WebviewBuilder::new(
            TOP_LABEL,
            WebviewUrl::App("index.html".into()),
        ));
        match window.add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(w, TOP_HEIGHT),
        ) {
            Ok(_) => info!("embedded-window: toolbar webview created"),
            Err(e) => warn!("embedded-window: toolbar webview failed: {e}"),
        }
    } else if let Some(top) = app.get_webview(TOP_LABEL) {
        if let Err(e) = top.set_bounds(rect(0.0, 0.0, w, TOP_HEIGHT)) {
            warn!("embedded-window: could not place the toolbar: {e}");
        }
    }

    // Game webview, only while embedded capture is the active mode.
    let wants_game = matches!(cfg.capture.mode, crate::config::CaptureMode::Embedded);
    match (wants_game, app.get_webview(GAME_LABEL)) {
        (true, None) => {
            let (gw, gh) = game_size(w, h);
            let builder = with_webview2(WebviewBuilder::new(GAME_LABEL, game_url(cfg)));
            match window.add_child(
                builder,
                LogicalPosition::new(0.0, TOP_HEIGHT),
                LogicalSize::new(gw, gh),
            ) {
                Ok(_) => info!("embedded-window: game webview created"),
                Err(e) => warn!("embedded-window: game webview failed: {e}"),
            }
        }
        (true, Some(view)) => {
            let (gw, gh) = game_size(w, h);
            if let Err(e) = view.set_bounds(rect(0.0, TOP_HEIGHT, gw, gh)) {
                warn!("embedded-window: could not place the game webview: {e}");
            }
        }
        (false, Some(view)) => {
            // Leaving embedded mode: drop the game webview so it cannot keep
            // a second game session running behind the scenes.
            if let Err(e) = view.close() {
                warn!("embedded-window: could not close the game webview: {e}");
            }
            info!("embedded-window: game webview closed (capture mode is not embedded)");
        }
        (false, None) => {}
    }

    // One line per layout pass, listing every webview the window holds. The
    // three-tile arrangement is only correct if all three exist, and a
    // silently missing child is invisible on screen (its area is just the
    // window background).
    if std::env::var_os("AKAGI_DEBUG_LAYOUT").is_some() {
        let mut labels: Vec<String> = app
            .webviews()
            .into_iter()
            .map(|(label, view)| {
                let size = view
                    .size()
                    .map(|s| format!("{:.0}x{:.0}", s.width, s.height))
                    .unwrap_or_else(|_| "?".into());
                format!("{label}={size}")
            })
            .collect();
        labels.sort();
        info!(
            "embedded-window: layout {w:.0}x{h:.0} panel={PANEL_WIDTH} top={TOP_HEIGHT} -> [{}]",
            labels.join(", ")
        );
    }
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> tauri::Rect {
    tauri::Rect {
        position: LogicalPosition::new(x, y).into(),
        size: LogicalSize::new(w.max(1.0), h.max(1.0)).into(),
    }
}

/// Game area = everything left of the panel, below the toolbar.
fn game_size(window_width: f64, window_height: f64) -> (f64, f64) {
    (
        (window_width - PANEL_WIDTH).max(1.0),
        (window_height - TOP_HEIGHT).max(1.0),
    )
}

/// Webview URL for the game: the platform's configured start URL. An empty
/// `start_url` (native-only platforms) has no web client to embed, so the
/// webview shows the app's own notice page instead of a blank white pane.
fn game_url(cfg: &AppConfig) -> WebviewUrl {
    let start = cfg.capture.chromium.start_url.trim();
    if start.is_empty() {
        WebviewUrl::App("index.html#/no-web-client".into())
    } else {
        WebviewUrl::External(
            start
                .parse()
                .unwrap_or_else(|_| "https://game.maj-soul.com/1/".parse().expect("static URL")),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_area_fills_everything_left_of_the_panel() {
        let (gw, gh) = game_size(1560.0, 980.0);
        assert_eq!(gw, 1560.0 - PANEL_WIDTH);
        assert_eq!(gh, 980.0 - TOP_HEIGHT);
        // The floor window still leaves a playable board next to the panel.
        let (gw, _) = game_size(MIN_WINDOW.0, MIN_WINDOW.1);
        assert!(gw >= 560.0, "board gets {gw}px at the window floor");
    }
}
