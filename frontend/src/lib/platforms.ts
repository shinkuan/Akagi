// Platform metadata used by the Setup wizard, Settings page, and any
// UI that needs to show platform-specific defaults / hints.
//
// `kind` mirrors `src/config/platform.rs::Platform` (PascalCase JSON), so
// the values flow into `AppConfig.platform.kind` unchanged.

import type { PlatformKind } from '@/types'

export type PlatformInfo = {
  kind: PlatformKind
  /** i18n key resolving to the localised platform name. */
  labelKey: string
  /** i18n key resolving to the one-line localised summary shown beside the picker. */
  descriptionKey: string
  /**
   * Default URL for the Chromium capture backend's `start_url` when this
   * platform is active. Picked so the launched browser lands directly on
   * the game's lobby/match-find page. Empty for native-only platforms that
   * have no web client (see `supportsChromium`).
   */
  defaultStartUrl: string
  /**
   * Whether the Chromium (CDP) capture backend can drive this platform.
   * `false` for native clients with no web build (e.g. Riichi City), which
   * are only reachable through the MITM proxy. The UI hides / disables the
   * Chromium option and forces MITM for these.
   */
  supportsChromium: boolean
  /**
   * Whether auto-start sessions can drive this platform's lobby. Requires
   * platform-side support (calibrated lobby coordinates, a visual home
   * anchor, an `autostart.<platform>.*` vocabulary and an option group in
   * the control bar) — only Majsoul is wired today. The GameDashboard hides
   * the auto-start control bar for platforms without it, and the backend
   * refuses `autostart_start` independently.
   */
  supportsAutostart: boolean
}

export const PLATFORMS: PlatformInfo[] = [
  {
    kind: 'Majsoul',
    labelKey: 'platform.majsoul',
    descriptionKey: 'platform.majsoul_desc',
    defaultStartUrl: 'https://game.maj-soul.com/1/',
    supportsChromium: true,
    supportsAutostart: true,
  },
  {
    kind: 'Tenhou',
    labelKey: 'platform.tenhou',
    descriptionKey: 'platform.tenhou_desc',
    defaultStartUrl: 'https://tenhou.net/4/',
    supportsChromium: true,
    // Web client exists, but no lobby coordinates / option group yet.
    supportsAutostart: false,
  },
  {
    kind: 'RiichiCity',
    labelKey: 'platform.riichi_city',
    descriptionKey: 'platform.riichi_city_desc',
    // Native client, no web build → MITM only.
    defaultStartUrl: '',
    supportsChromium: false,
    supportsAutostart: false,
  },
]

const BY_KIND: Record<PlatformKind, PlatformInfo> = Object.fromEntries(
  PLATFORMS.map((p) => [p.kind, p]),
) as Record<PlatformKind, PlatformInfo>

export function platformInfo(kind: PlatformKind): PlatformInfo {
  return BY_KIND[kind]
}

/**
 * Set of every URL we have ever shipped as a "default" for a platform.
 * Used to decide whether `start_url` is still a known default (so it's
 * safe to swap for the new platform's default on a platform change) or
 * whether the user has customised it (in which case we leave it alone).
 */
const KNOWN_DEFAULT_URLS = new Set<string>(PLATFORMS.map((p) => p.defaultStartUrl))

export function isKnownDefaultStartUrl(url: string): boolean {
  return KNOWN_DEFAULT_URLS.has(url.trim())
}
