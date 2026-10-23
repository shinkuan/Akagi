//! The always-on-top suggestion overlay ("PiP") window.
//!
//! A second webview, frameless and transparent, that floats over the game
//! client and renders the bot's top-N suggestions. It needs no backend
//! plumbing of its own: `bot-response` is already `app.emit()`-ed, which
//! broadcasts to *every* webview, so the overlay just listens for it (see
//! `frontend/src/routes/Overlay.tsx`).
//!
//! Both windows load the same `index.html`; the frontend branches on
//! `getCurrentWindow().label` to decide which root to render. That keeps the
//! window identity in one place (this module's [`LABEL`]) instead of encoding
//! it in a URL that the router would then have to parse back out.
//!
//! Lifecycle is driven entirely by `config.overlay.enabled`:
//!
//! - at startup, `lib::run` calls [`reconcile`] once;
//! - on every `update_config`, the command calls [`reconcile`] again;
//! - the overlay's own close button flips `enabled` to false, which routes
//!   back through the same path.

use crate::config::OverlayConfig;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
#[cfg(not(windows))]
use tauri_plugin_window_state::{StateFlags, WindowExt};
use tracing::{info, warn};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use windows_sys::core::BOOL;
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::Gdi::ClientToScreen,
    UI::HiDpi::GetDpiForWindow,
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
    UI::WindowsAndMessaging::{
        EnumChildWindows, EnumWindows, GetClassNameW, GetClientRect, GetCursorPos,
        GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    },
};

/// Window label. Also the label the `capabilities/overlay.json` capability is
/// scoped to — renaming this without renaming that leaves the overlay webview
/// with no permission to `listen()`, i.e. permanently blank.
pub const LABEL: &str = "overlay";

/// Event carrying a fresh [`OverlayConfig`] to every webview: the overlay reads
/// top-N / opacity off it, and the main window uses it to keep its own toggles
/// in sync when the overlay is closed from the overlay's own × button.
pub const CONFIG_EVENT: &str = "overlay-config";

/// A normalized pointer-down observed over the controlled Chromium renderer.
/// The tracker reads Windows' cursor/button state only; it never installs a
/// browser hook and never forwards or synthesizes input. The overlay uses this
/// to switch from the first-stage Chi button hint to Majsoul's second-stage
/// meld-combination chooser.
#[cfg(windows)]
pub const POINTER_EVENT: &str = "overlay-pointer-down";

/// Position and size are the only state worth restoring. The plugin's default
/// (`StateFlags::all()`) would also restore DECORATIONS — putting a title bar
/// back onto a window that is deliberately frameless — so `lib::run` excludes
/// this label from the automatic restore and we do it ourselves.
#[cfg(not(windows))]
const RESTORE_FLAGS: StateFlags = StateFlags::POSITION.union(StateFlags::SIZE);

const DEFAULT_WIDTH: f64 = 300.0;
#[cfg(not(windows))]
const MIN_WIDTH: f64 = 190.0;

// Windows "embedded-look" mode. Geometry and the 1287x724 frontend design
// space follow the reference application's display-only implementation.
// This is deliberately an ordinary transparent
// top-level window: it follows the controlled Chromium client rectangle but is
// never inserted into Chromium, never subclasses its HWND, and never executes
// script in the page.
#[cfg(windows)]
static DOCK_TRACKER_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
const DOCK_POLL: Duration = Duration::from_millis(60);
#[cfg(windows)]
const POINTER_POLL: Duration = Duration::from_millis(8);

#[cfg(windows)]
#[derive(Clone, Copy, serde::Serialize)]
struct OverlayPointer {
    /// Horizontal position inside the renderer, from 0 to 1.
    x: f64,
    /// Vertical position inside the renderer, from 0 to 1.
    y: f64,
}

// The rows split the window's height between them (see the `overlay-show` mahgen
// kind), so the window's height has to be a function of how many rows there are.
// A height that fits three rows comfortably squashes five into an unreadable
// smear, and `top_n` is user-settable — so both the starting height and the
// floor are derived from it rather than fixed.
/// Title bar, card border, and the padding around the list.
const CHROME_HEIGHT: f64 = 48.0;
/// Below this a row can no longer fit a legible tile next to its label.
#[cfg(any(not(windows), test))]
const MIN_ROW_HEIGHT: f64 = 34.0;
/// Roomy enough that the tile is worth glancing at without leaning in.
const DEFAULT_ROW_HEIGHT: f64 = 62.0;

fn default_height(top_n: usize) -> f64 {
    CHROME_HEIGHT + top_n as f64 * DEFAULT_ROW_HEIGHT
}

#[cfg(any(not(windows), test))]
fn min_height(top_n: usize) -> f64 {
    CHROME_HEIGHT + top_n as f64 * MIN_ROW_HEIGHT
}

pub fn get<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(LABEL)
}

/// Open the overlay, or re-apply the live settings to the one already open.
pub fn open<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig) -> tauri::Result<()> {
    let rows = cfg.clamped_top_n();

    if let Some(w) = get(app) {
        w.set_always_on_top(cfg.always_on_top)?;
        #[cfg(windows)]
        {
            // Docked mode must be allowed to follow a very small Chromium renderer.
            // The old card-HUD floor (150 logical px at top_n=3) otherwise clamps
            // the overlay while the game content has already shrunk below it.
            w.set_min_size(None::<LogicalSize<f64>>)?;
            // The tracker outlives individual overlay windows and resumes when
            // a fresh one appears. This idempotent call also covers a first-open
            // path raced with reconcile().
            start_dock_tracker(app);
        }
        #[cfg(not(windows))]
        // Raising `top_n` in Settings adds rows to a window that may already be
        // at its old floor, so the floor has to move with it.
        w.set_min_size(Some(LogicalSize::new(MIN_WIDTH, min_height(rows))))?;
        w.show()?;
        return Ok(());
    }

    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Akagi Overlay")
        .inner_size(DEFAULT_WIDTH, default_height(rows));
    // Do not register a Win32 minimum-track size at construction time. Winit
    // may retain that native constraint even after `set_min_size(None)`, which
    // is why the old 150 logical-pixel floor still appeared as a 160 physical-
    // pixel overlay over a 129-pixel renderer. Only the free-floating layouts
    // on other platforms need the card-HUD minimum.
    #[cfg(not(windows))]
    let builder = builder.min_inner_size(MIN_WIDTH, min_height(rows));

    let builder = builder.decorations(false);
    #[cfg(windows)]
    // The reference overlay is non-resizable. Besides preventing an accidental
    // edge drag, this removes Windows' resizable-frame minimum tracking
    // constraint, which otherwise clamps a requested 129 px renderer height to
    // roughly 160 px.
    let builder = builder.resizable(false);

    let w = builder
        .transparent(true)
        .always_on_top(cfg.always_on_top)
        .skip_taskbar(true)
        .maximizable(false)
        .minimizable(false)
        .shadow(false)
        // Never steal focus from the game client — the whole point of the
        // overlay is that you don't have to look away, let alone click away.
        .focused(false)
        .build()?;

    #[cfg(windows)]
    {
        // The browser renderer, not the legacy floating-card layout, owns the
        // geometry on Windows. Remove the builder's initial size floor before
        // the dock tracker applies its first measurement.
        w.set_min_size(None::<LogicalSize<f64>>)?;
        // Click-through is the important interaction guarantee: the hint layer
        // cannot consume a discard/call click or send any input to the game.
        w.set_ignore_cursor_events(true)?;
        start_dock_tracker(app);
    }

    // Undecorated windows are still resizable from their edges on Windows and
    // macOS, so no in-page resize grip is needed.
    #[cfg(not(windows))]
    {
        if let Err(e) = w.restore_state(RESTORE_FLAGS) {
            warn!("overlay: could not restore saved geometry: {e}");
        }
    }

    info!("overlay window opened");
    Ok(())
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClientBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[cfg(windows)]
struct WindowSearch {
    pid: u32,
    found: HWND,
    best_area: u64,
}

#[cfg(windows)]
unsafe extern "system" fn enum_controlled_chromium(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = unsafe { &mut *(lparam as *mut WindowSearch) };
    if unsafe { IsWindowVisible(hwnd) } == 0 || unsafe { IsIconic(hwnd) } != 0 {
        return 1;
    }

    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid != search.pid {
        return 1;
    }

    // Chromium top-level windows use this class. Filtering it out avoids
    // accidentally docking to an invisible utility HWND in the same process.
    let mut class_name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32) };
    let class = String::from_utf16_lossy(&class_name[..len.max(0) as usize]);
    if class != "Chrome_WidgetWin_1" {
        return 1;
    }

    // The browser process owns several Chrome_WidgetWin_1 top-level windows
    // (main frame, omnibox popup, devtools popups). Docking to the first one
    // enumeration happens to yield can anchor the hint layer to a tiny popup
    // instead of the game frame, so prefer the largest visible frame.
    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return 1;
    }
    let area = (rect.right - rect.left).max(0) as u64 * (rect.bottom - rect.top).max(0) as u64;
    if area > search.best_area {
        search.found = hwnd;
        search.best_area = area;
    }
    1
}

#[cfg(windows)]
fn controlled_chromium_window(pid: u32) -> Option<HWND> {
    if pid == 0 {
        return None;
    }
    let mut search = WindowSearch {
        pid,
        found: std::ptr::null_mut(),
        best_area: 0,
    };
    unsafe {
        EnumWindows(
            Some(enum_controlled_chromium),
            &mut search as *mut WindowSearch as LPARAM,
        );
    }
    (!search.found.is_null()).then_some(search.found)
}

#[cfg(windows)]
fn client_bounds(hwnd: HWND) -> Option<ClientBounds> {
    let mut rect = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    let mut origin = POINT { x: 0, y: 0 };
    if unsafe { ClientToScreen(hwnd, &mut origin) } == 0 {
        return None;
    }
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    // MahjongMaster accepts renderer rectangles above 100x100. Keeping the
    // old 320x240 floor made our tracker stop updating exactly when the user
    // reduced Chrome to a short, wide window.
    (width > 100 && height > 100).then_some(ClientBounds {
        x: origin.x,
        y: origin.y,
        width,
        height,
    })
}

#[cfg(windows)]
struct ContentSearch {
    found: Option<ClientBounds>,
}

#[cfg(windows)]
unsafe extern "system" fn enum_chromium_content(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut class_name = [0u16; 96];
    let len = unsafe { GetClassNameW(hwnd, class_name.as_mut_ptr(), class_name.len() as i32) };
    let class = String::from_utf16_lossy(&class_name[..len.max(0) as usize]);
    if class != "Chrome_RenderWidgetHostHWND" {
        return 1;
    }

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return 1;
    }
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    // Match the reference implementation's small-window tolerance. Chromium's
    // occlusion tracker may report the renderer child itself as not visible
    // while its top-level window is visible (notably when another app covers
    // it, which is an intended use case here), so class + usable rectangle are
    // the reliable criteria. A valid renderer can be only ~130 physical pixels
    // high after the toolbar has consumed most of a compact browser window.
    if width <= 100 || height <= 100 {
        return 1;
    }

    let search = unsafe { &mut *(lparam as *mut ContentSearch) };
    let candidate = ClientBounds {
        x: rect.left,
        y: rect.top,
        width,
        height,
    };
    let candidate_area = width as u64 * height as u64;
    let current_area = search
        .found
        .map(|b| b.width as u64 * b.height as u64)
        .unwrap_or(0);
    if candidate_area > current_area {
        search.found = Some(candidate);
    }
    1
}

#[cfg(windows)]
fn chromium_content_bounds(hwnd: HWND) -> Option<ClientBounds> {
    let mut search = ContentSearch { found: None };
    unsafe {
        EnumChildWindows(
            hwnd,
            Some(enum_chromium_content),
            &mut search as *mut ContentSearch as LPARAM,
        );
    }
    search.found
}

#[cfg(windows)]
fn dock_geometry(hwnd: HWND, client: ClientBounds) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    // Prefer Chromium's renderer child HWND: unlike a guessed toolbar offset,
    // this remains exact across display scaling, compact toolbar modes and
    // maximized/restored windows.
    if let Some(content) = chromium_content_bounds(hwnd) {
        return (
            PhysicalPosition::new(content.x, content.y),
            PhysicalSize::new(content.width, content.height),
        );
    }

    // Fallback for Chromium builds that hide the renderer child from
    // EnumChildWindows.
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let toolbar = ((74u64 * dpi as u64) / 96) as u32;
    let toolbar = toolbar.min(client.height.saturating_sub(240));
    (
        PhysicalPosition::new(client.x, client.y + toolbar as i32),
        PhysicalSize::new(client.width, client.height - toolbar),
    )
}

/// Move and resize the transparent overlay in one Win32 operation.
///
/// Calling Tauri's `set_size` followed by `set_position` creates an observable
/// intermediate frame and gives the webview two separate resize/layout turns.
/// The reference implementation uses one native SetWindowPos call, which keeps
/// the overlay rectangle coherent throughout a live browser resize.
#[cfg(windows)]
fn apply_overlay_geometry<R: Runtime>(
    overlay: &WebviewWindow<R>,
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> bool {
    let Ok(hwnd) = overlay.hwnd() else {
        return false;
    };
    unsafe {
        SetWindowPos(
            hwnd.0 as HWND,
            std::ptr::null_mut(),
            position.x,
            position.y,
            size.width as i32,
            size.height as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        ) != 0
    }
}

#[cfg(windows)]
fn start_dock_tracker<R: Runtime>(app: &AppHandle<R>) {
    if DOCK_TRACKER_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        let mut last_geometry: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> = None;
        let mut last_target: HWND = std::ptr::null_mut();
        let mut shown = false;
        let mut left_button_down = false;
        let mut last_dock_poll = Instant::now() - DOCK_POLL;

        loop {
            std::thread::sleep(POINTER_POLL);

            // Observe the user's real click without consuming, replaying or
            // modifying it. Normalized coordinates keep the frontend aligned
            // across DPI settings and live browser resizing.
            let down = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) } < 0;
            if down
                && !left_button_down
                && !last_target.is_null()
                && unsafe { GetForegroundWindow() } == last_target
            {
                if let Some((position, size)) = last_geometry {
                    let mut cursor = POINT { x: 0, y: 0 };
                    if unsafe { GetCursorPos(&mut cursor) } != 0 {
                        let relative_x = cursor.x - position.x;
                        let relative_y = cursor.y - position.y;
                        if relative_x >= 0
                            && relative_y >= 0
                            && relative_x < size.width as i32
                            && relative_y < size.height as i32
                        {
                            let _ = app.emit(
                                POINTER_EVENT,
                                OverlayPointer {
                                    x: relative_x as f64 / size.width as f64,
                                    y: relative_y as f64 / size.height as f64,
                                },
                            );
                        }
                    }
                }
            }
            left_button_down = down;

            if last_dock_poll.elapsed() < DOCK_POLL {
                continue;
            }
            last_dock_poll = Instant::now();

            let Some(overlay) = get(&app) else {
                shown = false;
                last_geometry = None;
                last_target = std::ptr::null_mut();
                continue;
            };

            let pid = crate::capture::chromium::launch::controlled_browser_pid();
            let Some(target) = controlled_chromium_window(pid) else {
                last_target = std::ptr::null_mut();
                last_geometry = None;
                if shown {
                    let _ = overlay.hide();
                    shown = false;
                }
                continue;
            };
            last_target = target;

            let Some(bounds) = client_bounds(target) else {
                continue;
            };
            let geometry = dock_geometry(target, bounds);
            if last_geometry != Some(geometry) {
                if apply_overlay_geometry(&overlay, geometry.0, geometry.1) {
                    last_geometry = Some(geometry);
                } else {
                    // Retain a best-effort cross-version fallback if Tauri's
                    // native handle is temporarily unavailable during startup.
                    let sized = overlay.set_size(geometry.1).is_ok();
                    let positioned = overlay.set_position(geometry.0).is_ok();
                    if sized && positioned {
                        last_geometry = Some(geometry);
                    }
                }
            }
            if !shown {
                let _ = overlay.show();
                shown = true;
            }
        }
    });
}

pub fn close<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(w) = get(app) {
        w.close()?;
        info!("overlay window closed");
    }
    Ok(())
}

/// Bring the live window in line with `cfg`: open it, close it, or leave it
/// alone. Idempotent — safe to call on every config save.
///
/// **This must run on the main thread, and it does not block.** On Windows,
/// `WebviewWindowBuilder::build()` called from a Tokio worker — which is where
/// every `#[tauri::command] async fn` runs — deadlocks: it asks the event loop
/// to create the window and then blocks the caller waiting for a reply the main
/// thread cannot deliver. The whole GUI freezes, with no error and no log line,
/// while background tasks carry on as if nothing happened. So the work is
/// posted to the event loop and `reconcile` returns immediately.
///
/// Startup goes through here too even though `lib::run`'s `setup` closure is
/// already on the main thread: one path, one set of rules.
pub fn reconcile<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig) {
    let handle = app.clone();
    let cfg = cfg.clone();
    if let Err(e) = app.run_on_main_thread(move || apply(&handle, &cfg)) {
        warn!("overlay: could not schedule reconcile on the main thread: {e}");
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, cfg: &OverlayConfig) {
    let result = if cfg.enabled {
        open(app, cfg)
    } else {
        close(app)
    };
    if let Err(e) = result {
        warn!("overlay: reconcile failed: {e}");
        return;
    }
    // Broadcast, not `emit_to(LABEL, …)`: the overlay needs the new top-N /
    // opacity, and the main window needs it to keep its toggles in sync with an
    // overlay that was closed from its own × button.
    if let Err(e) = app.emit(CONFIG_EVENT, cfg) {
        warn!("overlay: could not emit {CONFIG_EVENT}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TOP_N_MAX, TOP_N_MIN};

    /// The rows split the window's height, so a window sized for three rows
    /// squashes five into an unreadable smear. Both the starting height and the
    /// floor have to grow with `top_n` — a fixed height is the bug this replaced.
    #[test]
    fn window_height_grows_with_the_row_count() {
        for n in TOP_N_MIN..TOP_N_MAX {
            assert!(
                min_height(n + 1) > min_height(n),
                "floor must rise from {n} to {} rows",
                n + 1
            );
            assert!(
                default_height(n + 1) > default_height(n),
                "starting height must rise from {n} to {} rows",
                n + 1
            );
        }
    }

    /// Every row must clear `MIN_ROW_HEIGHT` at the floor, at any `top_n` —
    /// that is what keeps a legible tile next to its label.
    #[test]
    fn floor_leaves_every_row_its_minimum() {
        for n in TOP_N_MIN..=TOP_N_MAX {
            let per_row = (min_height(n) - CHROME_HEIGHT) / n as f64;
            assert!(
                per_row >= MIN_ROW_HEIGHT,
                "{n} rows get {per_row}px each, below the {MIN_ROW_HEIGHT}px minimum"
            );
        }
    }

    #[test]
    fn the_starting_height_is_roomier_than_the_floor() {
        for n in TOP_N_MIN..=TOP_N_MAX {
            assert!(default_height(n) > min_height(n));
        }
    }
}
