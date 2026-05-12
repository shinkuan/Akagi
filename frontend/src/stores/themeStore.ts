import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'

// Frontend-only theme preference. Two axes:
//   * `mode`    — light / dark / system (toggles the `.dark` class on <html>)
//   * `palette` — full color palette (sets `data-theme` on <html>)
//
// Each palette declares ALL CSS variables for both `:root` and `.dark` in
// `src/index.css`, following shadcn / tweakcn convention. This is why
// switching the palette repaints the entire UI (backgrounds, cards,
// sidebars, charts) instead of just the primary color — see ui.shadcn.com
// /docs/theming.
//
// FOUC is prevented by an inline pre-hydration script in `index.html` that
// reads the same localStorage key before this module loads.

export type ThemeMode = 'light' | 'dark' | 'system'
export type ThemePalette = 'default' | 'crimson' | 'slate'

export const THEME_MODES: readonly ThemeMode[] = ['light', 'dark', 'system']
export const THEME_PALETTES: readonly ThemePalette[] = ['default', 'crimson', 'slate']

export const THEME_MODE_DEFAULT: ThemeMode = 'system'
export const THEME_PALETTE_DEFAULT: ThemePalette = 'default'

type ThemeStore = {
  mode: ThemeMode
  palette: ThemePalette
  setMode: (mode: ThemeMode) => void
  setPalette: (palette: ThemePalette) => void
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

function applyTheme(mode: ThemeMode, palette: ThemePalette): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', resolveDark(mode))
  if (palette === 'default') {
    root.removeAttribute('data-theme')
  } else {
    root.setAttribute('data-theme', palette)
  }
}

export const useThemeStore = create(
  persist<ThemeStore>(
    (set) => ({
      mode: THEME_MODE_DEFAULT,
      palette: THEME_PALETTE_DEFAULT,
      setMode: (mode) => {
        set({ mode })
        applyTheme(mode, useThemeStore.getState().palette)
      },
      setPalette: (palette) => {
        set({ palette })
        applyTheme(useThemeStore.getState().mode, palette)
      },
    }),
    {
      name: 'akagi.theme',
      storage: createJSONStorage(() => localStorage),
      onRehydrateStorage: () => (state) => {
        if (state) applyTheme(state.mode, state.palette)
      },
    },
  ),
)

// Re-apply when the OS dark-mode preference changes, while mode === 'system'.
if (typeof window !== 'undefined' && window.matchMedia) {
  const mql = window.matchMedia('(prefers-color-scheme: dark)')
  const onChange = () => {
    const { mode, palette } = useThemeStore.getState()
    if (mode === 'system') applyTheme(mode, palette)
  }
  mql.addEventListener('change', onChange)
}

// Apply on module load. The persist middleware reads localStorage
// synchronously (client-only Vite app — see useSidebar.ts), so this picks up
// the user's stored preference. The inline script in index.html has already
// applied the same values before paint to prevent FOUC; this call is a no-op
// in the common case and a corrective sync if the script and store disagree.
applyTheme(useThemeStore.getState().mode, useThemeStore.getState().palette)
