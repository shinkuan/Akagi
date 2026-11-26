import { getCurrentWindow } from '@tauri-apps/api/window'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { HAS_TAURI } from '@/lib/tauri'

/** Label of the always-on-top suggestion overlay. Mirrors `ipc::overlay::LABEL`. */
export const OVERLAY_LABEL = 'overlay'

/** Toolbar strip of the single-window layout. Mirrors `ipc::embedded_window::TOP_LABEL`. */
export const TOP_LABEL = 'top'

/**
 * Labels of the **webview** this document runs in.
 *
 * Tauri 2 keeps windows and webviews in two layers: the single-window layout
 * puts three webviews (`main`, `top`, `game`) inside one window (`main`), so
 * `getCurrentWindow().label` says `main` for all of them. The webview label is
 * the one that identifies which *surface* this is — use [`currentWebviewLabel`]
 * for that, and [`currentWindowLabel`] only when the window itself matters
 * (the overlay is its own window).
 */
export function currentWebviewLabel(): string {
  if (!HAS_TAURI) return 'main'
  try {
    return getCurrentWebview().label
  } catch {
    return 'main'
  }
}

export function currentWindowLabel(): string {
  if (!HAS_TAURI) return 'main'
  try {
    return getCurrentWindow().label
  } catch {
    return 'main'
  }
}

export function isOverlayWindow(): boolean {
  // The overlay is a window of its own, so both labels agree there.
  return currentWindowLabel() === OVERLAY_LABEL || currentWebviewLabel() === OVERLAY_LABEL
}

/** The single-window layout's toolbar strip (not the main panel). */
export function isTopWindow(): boolean {
  return currentWebviewLabel() === TOP_LABEL
}

/** Is this the panel / wizard webview (as opposed to a detached page)? */
export function isMainWebview(): boolean {
  return currentWebviewLabel() === 'main'
}
