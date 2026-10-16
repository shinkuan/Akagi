// Edits to `AppConfig.capture` that the Setup wizard and Settings share.
//
// Every helper spreads the existing section, so a field one screen does not
// edit survives the save. `capture.http` used to be dropped here: Settings
// rebuilt `capture` from just `{mode, chromium}`, and the backend's serde
// defaults then silently reset `[capture.http]` on the next save.

import type {
  CaptureConfig,
  CaptureKind,
  CaptureMode,
  ChromiumConfig,
  IngestConfig,
} from '@/types'

export function withCaptureMode(capture: CaptureConfig, mode: CaptureMode): CaptureConfig {
  return { ...capture, mode }
}

export function withChromiumPatch(
  capture: CaptureConfig,
  patch: Partial<ChromiumConfig>,
): CaptureConfig {
  return { ...capture, chromium: { ...capture.chromium, ...patch } }
}

export function withIngestPatch(
  capture: CaptureConfig,
  patch: Partial<IngestConfig>,
): CaptureConfig {
  return { ...capture, ingest: { ...capture.ingest, ...patch } }
}

/**
 * The mode actually in effect. A platform without a web client can only be
 * captured through MITM, whatever a stale config says.
 */
export function effectiveCaptureMode(mode: CaptureMode, hasWebClient: boolean): CaptureMode {
  return hasWebClient ? mode : 'mitm'
}

/** i18n key naming the running capture backend (Overview, capture tile). */
export function captureTitleKey(kind: CaptureKind | null): string {
  switch (kind) {
    case 'chromium':
      return 'overview.capture_chromium'
    case 'ingest':
      return 'ingest.overview_title'
    default:
      return 'overview.capture_mitm'
  }
}

/** Where the running ingest backend serves the userscript. */
export function userscriptUrl(addr: string): string {
  return `http://${addr.trim()}/akagi.user.js`
}

/**
 * The install page the Install button opens, in the UI's language. It links
 * the script and explains what to do when the browser has no userscript
 * manager — opening the script directly would just show a page of code.
 */
export function installPageUrl(addr: string, lang: string): string {
  return `http://${addr.trim()}/?lang=${encodeURIComponent(lang)}`
}
