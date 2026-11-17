import { create } from 'zustand'
import type { LayoutItem } from 'react-grid-layout'
import {
  ALL_TILES,
  DEFAULT_HIDDEN,
  DEFAULT_HIDDEN_3P,
  DEFAULT_LAYOUTS,
  DEFAULT_LAYOUTS_3P,
  type Breakpoint,
  type TileId,
} from '@/tiles/defaults'

// Storage keys are stable. The schema version lives *inside* the JSON
// payload (see `SCHEMA`) so we can evolve the saved shape without ever
// touching the key name (which used to look like `akagi.v5.*` — easy to
// confuse with the "Akagi V3" app version).
//
// Constraints (`minW` / `minH` / `maxW` / `maxH`) are NOT persisted; they
// come from `DEFAULT_LAYOUTS` at load time, so changing a constraint
// propagates without any migration.
const LAYOUTS_KEY_4P = 'akagi.dashboard.layouts.4p'
const HIDDEN_KEY_4P = 'akagi.dashboard.hidden.4p'
const LAYOUTS_KEY_3P = 'akagi.dashboard.layouts.3p'
const HIDDEN_KEY_3P = 'akagi.dashboard.hidden.3p'

// Bump only when the on-disk *shape* of layouts/hidden actually changes
// (e.g. wrapping the per-breakpoint object inside something else, or
// renaming the breakpoint keys). Constraint tweaks do NOT need a bump.
// Mismatched payloads are discarded → defaults rebuild on next load.
const SCHEMA = 1
type StoredEnvelope<T> = { schema: number; data: T }

function envelope<T>(data: T): StoredEnvelope<T> {
  return { schema: SCHEMA, data }
}

function unwrap(parsed: unknown): unknown {
  if (parsed && typeof parsed === 'object' && 'schema' in (parsed as object) && 'data' in (parsed as object)) {
    const env = parsed as StoredEnvelope<unknown>
    return env.schema === SCHEMA ? env.data : null
  }
  // Unwrapped payload from a pre-envelope save → discard, rebuild from defaults.
  return null
}

export type Layouts = Record<Breakpoint, LayoutItem[]>
export type HiddenSet = Record<Breakpoint, TileId[]>
export type GameMode = '4p' | '3p'

type LayoutStore = {
  /** Current display mode — selected by the dashboard from `game.num_players`. */
  mode: GameMode
  layouts4p: Layouts
  hidden4p: HiddenSet
  layouts3p: Layouts
  hidden3p: HiddenSet
  /** Active preset for the current `mode`. Computed; do not set directly. */
  layouts: Layouts
  hidden: HiddenSet
  setMode: (mode: GameMode) => void
  setLayouts: (l: Layouts) => void
  hide: (id: TileId, bp: Breakpoint) => void
  show: (id: TileId, bp: Breakpoint) => void
  reset: () => void
}

function deepClone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v))
}

// Re-apply current dev-set constraints (minW/minH/maxW/maxH) onto a saved
// item. Saved items only own the user-controlled fields (x/y/w/h); w/h are
// clamped to whatever the current defaults allow so a tightened maxW or
// raised minW takes effect on the next load without a version bump.
// Items not present in DEFAULT_LAYOUTS (e.g. removed tiles) are dropped.
function mergeItemWithDefault(saved: LayoutItem, def: LayoutItem): LayoutItem {
  const minW = def.minW ?? 1
  const minH = def.minH ?? 1
  const maxW = def.maxW ?? Infinity
  const maxH = def.maxH ?? Infinity
  const w = Math.min(Math.max(saved.w ?? def.w, minW), maxW)
  const h = Math.min(Math.max(saved.h ?? def.h, minH), maxH)
  return {
    ...def,
    x: saved.x ?? def.x,
    y: saved.y ?? def.y,
    w,
    h,
  }
}

// Ensure all four breakpoints exist as arrays AND every item has up-to-date
// constraints. Stale / partial localStorage data (older format, missing
// keys, non-array values) is replaced from defaults.
function normaliseLayouts(parsed: unknown, fallback: Layouts): Layouts {
  const fresh = deepClone(fallback)
  if (!parsed || typeof parsed !== 'object') return fresh
  const obj = parsed as Record<string, unknown>
  for (const bp of ['lg', 'md', 'sm', 'xs'] as const) {
    if (!Array.isArray(obj[bp])) continue
    const defByI = new Map(fresh[bp].map((d) => [d.i, d]))
    const savedByI = new Map(
      (obj[bp] as LayoutItem[])
        .filter((s) => s && typeof s === 'object' && typeof s.i === 'string')
        .map((s) => [s.i, s] as const),
    )
    const merged: LayoutItem[] = []
    // Saved tiles still in defaults — keep position, refresh constraints.
    for (const [i, saved] of savedByI) {
      const def = defByI.get(i)
      if (!def) continue // tile removed from defaults — drop it
      merged.push(mergeItemWithDefault(saved, def))
    }
    // Default tiles missing from save — add fresh (e.g. new tile shipped).
    for (const def of fresh[bp]) {
      if (!savedByI.has(def.i)) merged.push({ ...def })
    }
    fresh[bp] = merged
  }
  return fresh
}

function normaliseHidden(parsed: unknown, defaults: TileId[]): HiddenSet {
  const fallback: HiddenSet = {
    lg: [...defaults], md: [...defaults],
    sm: [...defaults], xs: [...defaults],
  }
  if (!parsed || typeof parsed !== 'object') return fallback
  const obj = parsed as Record<string, unknown>
  for (const bp of ['lg', 'md', 'sm', 'xs'] as const) {
    if (Array.isArray(obj[bp])) fallback[bp] = obj[bp] as TileId[]
  }
  return fallback
}

function loadLayouts(key: string, fallback: Layouts): Layouts {
  if (typeof localStorage === 'undefined') return deepClone(fallback)
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return deepClone(fallback)
    return normaliseLayouts(unwrap(JSON.parse(raw)), fallback)
  } catch {
    return deepClone(fallback)
  }
}

function loadHidden(key: string, defaults: TileId[]): HiddenSet {
  if (typeof localStorage === 'undefined') return normaliseHidden(null, defaults)
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return normaliseHidden(null, defaults)
    return normaliseHidden(unwrap(JSON.parse(raw)), defaults)
  } catch {
    return normaliseHidden(null, defaults)
  }
}

// Persist only the user-controllable fields. Constraints are recomputed
// from defaults at load time, so saving them would just create stale data.
function stripConstraints(layouts: Layouts): Record<Breakpoint, Pick<LayoutItem, 'i' | 'x' | 'y' | 'w' | 'h'>[]> {
  const out = {} as Record<Breakpoint, Pick<LayoutItem, 'i' | 'x' | 'y' | 'w' | 'h'>[]>
  for (const bp of ['lg', 'md', 'sm', 'xs'] as const) {
    out[bp] = (layouts[bp] ?? []).map(({ i, x, y, w, h }) => ({ i, x, y, w, h }))
  }
  return out
}

function persistMode(
  mode: GameMode,
  layouts: Layouts,
  hidden: HiddenSet,
) {
  if (typeof localStorage === 'undefined') return
  try {
    const lk = mode === '3p' ? LAYOUTS_KEY_3P : LAYOUTS_KEY_4P
    const hk = mode === '3p' ? HIDDEN_KEY_3P : HIDDEN_KEY_4P
    localStorage.setItem(lk, JSON.stringify(envelope(stripConstraints(layouts))))
    localStorage.setItem(hk, JSON.stringify(envelope(hidden)))
  } catch {
    /* quota exceeded — ignore */
  }
}

function defaultEntry(id: TileId, bp: Breakpoint, mode: GameMode): LayoutItem {
  const presets = mode === '3p' ? DEFAULT_LAYOUTS_3P : DEFAULT_LAYOUTS
  const def = presets[bp].find((l) => l.i === id)
  if (def) return { ...def }
  return { i: id, x: 0, y: 999, w: 4, h: 4, minW: 2, minH: 2 }
}

export const useLayoutStore = create<LayoutStore>((set, get) => {
  const layouts4p = loadLayouts(LAYOUTS_KEY_4P, DEFAULT_LAYOUTS)
  const hidden4p = loadHidden(HIDDEN_KEY_4P, DEFAULT_HIDDEN)
  const layouts3p = loadLayouts(LAYOUTS_KEY_3P, DEFAULT_LAYOUTS_3P)
  const hidden3p = loadHidden(HIDDEN_KEY_3P, DEFAULT_HIDDEN_3P)
  return {
    mode: '4p',
    layouts4p,
    hidden4p,
    layouts3p,
    hidden3p,
    layouts: layouts4p,
    hidden: hidden4p,

    setMode: (mode) => {
      const s = get()
      if (s.mode === mode) return
      const layouts = mode === '3p' ? s.layouts3p : s.layouts4p
      const hidden = mode === '3p' ? s.hidden3p : s.hidden4p
      set({ mode, layouts, hidden })
    },

    setLayouts: (layouts) => {
      const s = get()
      persistMode(s.mode, layouts, s.hidden)
      set(
        s.mode === '3p'
          ? { layouts, layouts3p: layouts }
          : { layouts, layouts4p: layouts },
      )
    },

    hide: (id, bp) => {
      const s = get()
      const layouts = deepClone(s.layouts)
      layouts[bp] = (layouts[bp] ?? []).filter((l) => l.i !== id)
      const hidden = deepClone(s.hidden)
      hidden[bp] = [...new Set([...(hidden[bp] ?? []), id])]
      persistMode(s.mode, layouts, hidden)
      set(
        s.mode === '3p'
          ? { layouts, hidden, layouts3p: layouts, hidden3p: hidden }
          : { layouts, hidden, layouts4p: layouts, hidden4p: hidden },
      )
    },

    show: (id, bp) => {
      const s = get()
      const layouts = deepClone(s.layouts)
      if (!(layouts[bp] ?? []).some((l) => l.i === id)) {
        layouts[bp] = [...(layouts[bp] ?? []), defaultEntry(id, bp, s.mode)]
      }
      const hidden = deepClone(s.hidden)
      hidden[bp] = (hidden[bp] ?? []).filter((x) => x !== id)
      persistMode(s.mode, layouts, hidden)
      set(
        s.mode === '3p'
          ? { layouts, hidden, layouts3p: layouts, hidden3p: hidden }
          : { layouts, hidden, layouts4p: layouts, hidden4p: hidden },
      )
    },

    reset: () => {
      const s = get()
      const presets = s.mode === '3p' ? DEFAULT_LAYOUTS_3P : DEFAULT_LAYOUTS
      const defaultsHidden = s.mode === '3p' ? DEFAULT_HIDDEN_3P : DEFAULT_HIDDEN
      const layouts = deepClone(presets)
      const hidden: HiddenSet = {
        lg: [...defaultsHidden], md: [...defaultsHidden],
        sm: [...defaultsHidden], xs: [...defaultsHidden],
      }
      persistMode(s.mode, layouts, hidden)
      set(
        s.mode === '3p'
          ? { layouts, hidden, layouts3p: layouts, hidden3p: hidden }
          : { layouts, hidden, layouts4p: layouts, hidden4p: hidden },
      )
    },
  }
})

// Helper: compute visible tiles for a given breakpoint. `mode` controls
// which canonical tile order applies (3p drops `player-3` from the menu).
export function visibleTilesFor(
  bp: Breakpoint,
  hidden: HiddenSet,
  mode: GameMode = '4p',
): TileId[] {
  const hide = new Set(hidden[bp] ?? [])
  const tiles = mode === '3p' ? ALL_TILES.filter((id) => id !== 'player-3') : ALL_TILES
  return tiles.filter((id) => !hide.has(id))
}
