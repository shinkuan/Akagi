import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'

// Frontend-only theme preference (mode + accent). Lives in localStorage
// alongside the other UI prefs — `AppConfig` (Rust) is intentionally not
// touched. Mode toggles `.dark` on <html>; accent sets `data-theme`.
//
// FOUC is prevented by an inline pre-hydration script in `index.html` that
// reads the same localStorage key before this module loads.

export type ThemeMode = 'light' | 'dark' | 'system'
export type ThemeAccent = 'default' | 'crimson' | 'slate'

export const THEME_MODES: readonly ThemeMode[] = ['light', 'dark', 'system']
export const THEME_ACCENTS: readonly ThemeAccent[] = ['default', 'crimson', 'slate']

export const THEME_MODE_DEFAULT: ThemeMode = 'system'
export const THEME_ACCENT_DEFAULT: ThemeAccent = 'default'

type ThemeStore = {
  mode: ThemeMode
  accent: ThemeAccent
  setMode: (mode: ThemeMode) => void
  setAccent: (accent: ThemeAccent) => void
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

function applyTheme(mode: ThemeMode, accent: ThemeAccent): void {
  if (typeof document === 'undefined') return
  const root = document.documentElement
  root.classList.toggle('dark', resolveDark(mode))
  if (accent === 'default') {
    root.removeAttribute('data-theme')
  } else {
    root.setAttribute('data-theme', accent)
  }
}

export const useThemeStore = create(
  persist<ThemeStore>(
    (set) => ({
      mode: THEME_MODE_DEFAULT,
      accent: THEME_ACCENT_DEFAULT,
      setMode: (mode) => {
        set({ mode })
        applyTheme(mode, useThemeStore.getState().accent)
      },
      setAccent: (accent) => {
        set({ accent })
        applyTheme(useThemeStore.getState().mode, accent)
      },
    }),
    {
      name: 'akagi.theme',
      storage: createJSONStorage(() => localStorage),
      onRehydrateStorage: () => (state) => {
        if (state) applyTheme(state.mode, state.accent)
      },
    },
  ),
)

// Re-apply when the OS dark-mode preference changes, while mode === 'system'.
if (typeof window !== 'undefined' && window.matchMedia) {
  const mql = window.matchMedia('(prefers-color-scheme: dark)')
  const onChange = () => {
    const { mode, accent } = useThemeStore.getState()
    if (mode === 'system') applyTheme(mode, accent)
  }
  // addEventListener is the modern API; the deprecated addListener fallback is
  // not needed for any browser the Tauri webview ships with.
  mql.addEventListener('change', onChange)
}

// Apply on module load. The persist middleware reads localStorage
// synchronously (client-only Vite app — see useSidebar.ts), so this picks up
// the user's stored preference. The inline script in index.html has already
// applied the same values before paint to prevent FOUC; this call is a no-op
// in the common case and a corrective sync if the script and store disagree.
applyTheme(useThemeStore.getState().mode, useThemeStore.getState().accent)
