import { useEffect, useState } from 'react'
import { RouterProvider } from 'react-router-dom'
import { SidePanel } from '@/routes/SidePanel'
import { router } from '@/router'
import { HAS_TAURI, invoke } from '@/lib/tauri'
import type { AppConfig } from '@/types'

/**
 * What the `main` webview shows: the setup wizard until `first_run_completed`,
 * the right-hand summary panel afterwards.
 *
 * The decision is by *config*, not by label, because the same webview serves
 * both roles across the wizard boundary — the backend reshapes the window into
 * the three-webview layout the moment the wizard saves.
 */
export function MainWindowRoot() {
  const [cfg, setCfg] = useState<AppConfig | null>(null)
  // Outside Tauri there is nothing to load; inside, the first paint waits for
  // `get_config` so the wizard and the panel never flash into each other.
  const [loaded, setLoaded] = useState(!HAS_TAURI)

  useEffect(() => {
    if (!HAS_TAURI) return
    let alive = true
    invoke<AppConfig>('get_config')
      .then((c) => {
        if (alive) setCfg(c)
      })
      .catch(() => {})
      .finally(() => {
        if (alive) setLoaded(true)
      })
    return () => {
      alive = false
    }
  }, [])

  if (!loaded) return null
  // `null` config (backend still starting, or browser dev mode) renders the
  // full app: it is the safer default, and it owns its own /setup bounce.
  if (cfg && cfg.general.first_run_completed) return <SidePanel />
  return <RouterProvider router={router} />
}
