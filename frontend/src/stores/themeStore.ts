import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'

// Frontend-only theme preference. Two axes:
//   * `mode`    — light / dark / system (toggles the `.dark` class on <html>)
//   * `palette` — color palette (sets `data-theme` on <html>)
//
// For named palettes (default / crimson / slate) the full CSS variable set
// lives in `src/index.css`. For `custom`, this store generates a stylesheet
// at runtime from the user-edited base colors (background / foreground /
// primary / primary-foreground / border) and derives the rest via CSS
// `color-mix()` so every shadcn token stays coherent.
//
// FOUC is prevented by an inline pre-hydration script in `index.html` that
// reads the same localStorage envelope (plus `akagi.theme.css` for custom
// palettes) before this module loads.

export type ThemeMode = 'light' | 'dark' | 'system'
export type ThemePalette = 'default' | 'crimson' | 'slate' | 'custom'

export const THEME_MODES: readonly ThemeMode[] = ['light', 'dark', 'system']
export const THEME_PALETTES: readonly ThemePalette[] = ['default', 'crimson', 'slate', 'custom']

export const THEME_MODE_DEFAULT: ThemeMode = 'system'
export const THEME_PALETTE_DEFAULT: ThemePalette = 'default'

// Subset the user can edit in the custom editor. The remaining tokens are
// derived via color-mix() so the palette stays internally consistent when
// the base colors change.
export const CUSTOM_BASE_VARS = [
  'background',
  'foreground',
  'primary',
  'primary-foreground',
  'border',
] as const
export type CustomBaseVar = (typeof CUSTOM_BASE_VARS)[number]

// Full shadcn variable inventory — what `generateCustomCss` emits.
export const SHADCN_VARS = [
  'background',
  'foreground',
  'card',
  'card-foreground',
  'popover',
  'popover-foreground',
  'primary',
  'primary-foreground',
  'secondary',
  'secondary-foreground',
  'muted',
  'muted-foreground',
  'accent',
  'accent-foreground',
  'destructive',
  'border',
  'input',
  'ring',
  'chart-1',
  'chart-2',
  'chart-3',
  'chart-4',
  'chart-5',
  'sidebar',
  'sidebar-foreground',
  'sidebar-primary',
  'sidebar-primary-foreground',
  'sidebar-accent',
  'sidebar-accent-foreground',
  'sidebar-border',
  'sidebar-ring',
] as const
export type ShadcnVar = (typeof SHADCN_VARS)[number]

export type CustomVarsMap = Partial<Record<ShadcnVar, string>>
export type CustomTheme = { light: CustomVarsMap; dark: CustomVarsMap }

const DEFAULT_CUSTOM_LIGHT: CustomVarsMap = {
  background: '#fafafa',
  foreground: '#111111',
  primary: '#0a7d52',
  'primary-foreground': '#ffffff',
  border: '#d4d4d4',
}
const DEFAULT_CUSTOM_DARK: CustomVarsMap = {
  background: '#0f1a1c',
  foreground: '#f0f5f3',
  primary: '#34d399',
  'primary-foreground': '#0f1a1c',
  border: '#2c3a3e',
}

const STYLE_TAG_ID = 'akagi-custom-theme'
const CSS_STORAGE_KEY = 'akagi.theme.css'

type ThemeStore = {
  mode: ThemeMode
  palette: ThemePalette
  custom: CustomTheme
  setMode: (mode: ThemeMode) => void
  setPalette: (palette: ThemePalette) => void
  setCustomVar: (mode: 'light' | 'dark', name: CustomBaseVar, value: string) => void
  resetCustom: () => void
  /**
   * Accepts either a URL pointing at a tweakcn / shadcn theme JSON or the
   * raw JSON body pasted as text. Replaces the stored custom palette on
   * success.
   */
  importCustom: (input: string) => Promise<void>
}

function prefersDark(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

function resolveDark(mode: ThemeMode): boolean {
  if (mode === 'dark') return true
  if (mode === 'light') return false
  return prefersDark()
}

// Per-token derivation expression when the user has not supplied an explicit
// value. Each expression resolves at paint time via CSS color-mix(), so
// changing a base color cascades through the dependent tokens automatically.
function deriveExpression(name: ShadcnVar, vars: CustomVarsMap): string {
  const explicit = vars[name]
  if (explicit) return explicit
  switch (name) {
    case 'background':
      return '#fafafa'
    case 'foreground':
      return '#111111'
    case 'primary':
      return '#0a7d52'
    case 'primary-foreground':
      return '#ffffff'
    case 'border':
      return '#d4d4d4'
    case 'card':
    case 'popover':
    case 'sidebar':
      return 'var(--background)'
    case 'card-foreground':
    case 'popover-foreground':
    case 'secondary-foreground':
    case 'accent-foreground':
    case 'sidebar-foreground':
    case 'sidebar-accent-foreground':
      return 'var(--foreground)'
    case 'secondary':
      return 'color-mix(in oklch, var(--foreground) 6%, var(--background))'
    case 'muted':
      return 'color-mix(in oklch, var(--foreground) 5%, var(--background))'
    case 'muted-foreground':
      return 'color-mix(in oklch, var(--foreground) 50%, var(--background))'
    case 'accent':
      return 'color-mix(in oklch, var(--primary) 18%, var(--background))'
    case 'destructive':
      return 'oklch(0.65 0.23 25)'
    case 'input':
      return 'color-mix(in oklch, var(--border) 80%, var(--background))'
    case 'ring':
    case 'sidebar-primary':
    case 'sidebar-ring':
    case 'chart-1':
      return 'var(--primary)'
    case 'sidebar-primary-foreground':
      return 'var(--primary-foreground)'
    case 'sidebar-accent':
      return 'var(--accent)'
    case 'sidebar-border':
      return 'var(--border)'
    case 'chart-2':
      return 'color-mix(in oklch, var(--primary) 60%, oklch(0.6 0.15 200))'
    case 'chart-3':
      return 'color-mix(in oklch, var(--primary) 40%, oklch(0.55 0.14 280))'
    case 'chart-4':
      return 'color-mix(in oklch, var(--primary) 60%, oklch(0.7 0.16 80))'
    case 'chart-5':
      return 'color-mix(in oklch, var(--primary) 40%, oklch(0.65 0.17 30))'
  }
}

function block(selector: string, vars: CustomVarsMap): string {
  const lines = SHADCN_VARS.map(
    (v) => `  --${v}: ${deriveExpression(v, vars)};`,
  ).join('\n')
  return `${selector} {\n${lines}\n}`
}

export function generateCustomCss(theme: CustomTheme): string {
  return [
    block(':root[data-theme="custom"]', theme.light),
    block('.dark[data-theme="custom"]', theme.dark),
  ].join('\n\n')
}

function applyCustomCss(css: string | null): void {
  if (typeof document === 'undefined') return
  const existing = document.getElementById(STYLE_TAG_ID) as HTMLStyleElement | null
  if (css) {
    const el = existing ?? document.createElement('style')
    if (!existing) {
      el.id = STYLE_TAG_ID
      document.head.appendChild(el)
    }
    if (el.textContent !== css) el.textContent = css
    try {
      localStorage.setItem(CSS_STORAGE_KEY, css)
    } catch {
      /* quota — ignore */
    }
  } else {
    existing?.remove()
    try {
      localStorage.removeItem(CSS_STORAGE_KEY)
    } catch {
      /* ignore */
    }
  }
}

function applyTheme(mode: ThemeMode, palette: ThemePalette, custom: CustomTheme): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', resolveDark(mode))
  if (palette === 'default') {
    root.removeAttribute('data-theme')
  } else {
    root.setAttribute('data-theme', palette)
  }
  if (palette === 'custom') {
    applyCustomCss(generateCustomCss(custom))
  } else {
    applyCustomCss(null)
  }
}

// Tweakcn / shadcn registry shape:
//   { cssVars: { light: { background: "...", ... }, dark: { ... } } }
// Modern tweakcn ships everything as oklch(...) strings; legacy shadcn ships
// "H S% L%" triplets that need to be wrapped in hsl(...) so CSS can parse
// them.
function parseTheme(raw: unknown): CustomTheme {
  if (!raw || typeof raw !== 'object') throw new Error('Invalid theme payload')
  const obj = raw as Record<string, unknown>
  const cssVars = (obj.cssVars as Record<string, unknown> | undefined) ?? obj
  const lightRaw = (cssVars as Record<string, unknown>)?.light as
    | Record<string, unknown>
    | undefined
  const darkRaw = (cssVars as Record<string, unknown>)?.dark as
    | Record<string, unknown>
    | undefined
  if (!lightRaw && !darkRaw) {
    throw new Error('Missing cssVars.light / cssVars.dark')
  }
  const mapVars = (src?: Record<string, unknown>): CustomVarsMap => {
    const out: CustomVarsMap = {}
    if (!src) return out
    for (const key of SHADCN_VARS) {
      const v = src[key]
      if (typeof v === 'string' && v.trim()) {
        const trimmed = v.trim()
        // Legacy shadcn HSL triplet — no function wrapper, e.g. "240 5% 10%".
        const looksLikeHslTriplet =
          /^[\d.\-]+\s+[\d.]+%\s+[\d.]+%(\s*\/\s*[\d.%]+)?$/.test(trimmed)
        out[key] = looksLikeHslTriplet ? `hsl(${trimmed})` : trimmed
      }
    }
    return out
  }
  return {
    light: mapVars(lightRaw),
    dark: mapVars(darkRaw),
  }
}

// Tweakcn surfaces a theme at three different URLs depending on where the
// user grabs it. All three map to a single registry endpoint
// `tweakcn.com/r/themes/<slug-or-id>` which the registry serves as JSON
// under content negotiation. Auto-rewrite so users can paste straight out
// of the browser address bar without thinking about it.
//
//   share page : tweakcn.com/themes/<id>                 (HTML gallery view)
//   editor     : tweakcn.com/editor/theme?theme=<slug>   (HTML editor view)
//   registry   : tweakcn.com/r/themes/<slug-or-id>       (JSON, our target)
function normalizeThemeUrl(input: string): string {
  const raw = /^https?:\/\//i.test(input) ? input : `https://${input}`
  let u: URL
  try {
    u = new URL(raw)
  } catch {
    return raw
  }
  if (!/(^|\.)tweakcn\.com$/i.test(u.hostname)) return raw
  if (u.pathname === '/editor/theme' && u.searchParams.has('theme')) {
    const name = u.searchParams.get('theme') as string
    return `${u.protocol}//${u.host}/r/themes/${encodeURIComponent(name)}`
  }
  const shareMatch = u.pathname.match(/^\/themes\/([^/]+)\/?$/)
  if (shareMatch) {
    return `${u.protocol}//${u.host}/r/themes/${shareMatch[1]}`
  }
  return raw
}

async function loadTheme(input: string): Promise<CustomTheme> {
  const trimmed = input.trim()
  if (!trimmed) throw new Error('No input')
  let data: unknown
  if (trimmed.startsWith('{')) {
    data = JSON.parse(trimmed)
  } else {
    const url = normalizeThemeUrl(trimmed)
    const resp = await fetch(url, { headers: { Accept: 'application/json' } })
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`)
    data = await resp.json()
  }
  return parseTheme(data)
}

export const useThemeStore = create(
  persist<ThemeStore>(
    (set, get) => ({
      mode: THEME_MODE_DEFAULT,
      palette: THEME_PALETTE_DEFAULT,
      custom: { light: { ...DEFAULT_CUSTOM_LIGHT }, dark: { ...DEFAULT_CUSTOM_DARK } },
      setMode: (mode) => {
        set({ mode })
        const s = get()
        applyTheme(mode, s.palette, s.custom)
      },
      setPalette: (palette) => {
        set({ palette })
        const s = get()
        applyTheme(s.mode, palette, s.custom)
      },
      setCustomVar: (which, name, value) => {
        const prev = get().custom
        const nextCustom: CustomTheme = {
          light: which === 'light' ? { ...prev.light, [name]: value } : prev.light,
          dark: which === 'dark' ? { ...prev.dark, [name]: value } : prev.dark,
        }
        set({ custom: nextCustom })
        const s = get()
        applyTheme(s.mode, s.palette, nextCustom)
      },
      resetCustom: () => {
        const nextCustom: CustomTheme = {
          light: { ...DEFAULT_CUSTOM_LIGHT },
          dark: { ...DEFAULT_CUSTOM_DARK },
        }
        set({ custom: nextCustom })
        const s = get()
        applyTheme(s.mode, s.palette, nextCustom)
      },
      importCustom: async (input) => {
        const theme = await loadTheme(input)
        // Merge missing keys from the current custom map so half-populated
        // imports don't blank out tokens they didn't define.
        const prev = get().custom
        const merged: CustomTheme = {
          light: { ...prev.light, ...theme.light },
          dark: { ...prev.dark, ...theme.dark },
        }
        set({ custom: merged })
        const s = get()
        applyTheme(s.mode, s.palette, merged)
      },
    }),
    {
      name: 'akagi.theme',
      storage: createJSONStorage(() => localStorage),
      onRehydrateStorage: () => (state) => {
        if (state) applyTheme(state.mode, state.palette, state.custom)
      },
    },
  ),
)

// Re-apply when the OS dark-mode preference changes, while mode === 'system'.
if (typeof window !== 'undefined' && window.matchMedia) {
  const mql = window.matchMedia('(prefers-color-scheme: dark)')
  const onChange = () => {
    const { mode, palette, custom } = useThemeStore.getState()
    if (mode === 'system') applyTheme(mode, palette, custom)
  }
  mql.addEventListener('change', onChange)
}

// Apply on module load — the persist middleware reads localStorage
// synchronously (client-only Vite app). The inline script in index.html has
// already applied the same values before paint to prevent FOUC; this call
// reconciles any drift after hydration.
{
  const s = useThemeStore.getState()
  applyTheme(s.mode, s.palette, s.custom)
}
