import { createHashRouter, redirect } from 'react-router-dom'
import App from '@/App.tsx'
import { Overview } from '@/routes/Overview'
import { GameDashboard } from '@/routes/GameDashboard'
import { Bots } from '@/routes/Bots'
import { History } from '@/routes/History'
import { Review } from '@/routes/Review'
import { Logs } from '@/routes/Logs'
import { Settings } from '@/routes/Settings'
import { Setup } from '@/routes/Setup'
import { HAS_TAURI, invoke } from '@/lib/tauri'
import type { AppConfig } from '@/types'

/** The hash router shared by the panel's wizard branch and the detached pages. */
// Loader on the protected branch: bounce to /setup when first_run_completed
// is false. The /setup route lives outside this loader so it can render
// without recursion. Browser-only mode (no Tauri) skips the check entirely.
const requireFirstRunCompleted = async () => {
  if (!HAS_TAURI) return null
  try {
    const cfg = await invoke<AppConfig>('get_config')
    if (!cfg.general.first_run_completed) {
      return redirect('/setup')
    }
  } catch {
    // If get_config fails the rest of the UI surfaces it; don't gate on it.
  }
  return null
}

export const router = createHashRouter([
  { path: '/setup', element: <Setup /> },
  {
    element: <App />,
    loader: requireFirstRunCompleted,
    children: [
      { index: true, element: <Overview /> },
      { path: 'game', element: <GameDashboard /> },
      { path: 'bots', element: <Bots /> },
      { path: 'history', element: <History /> },
      { path: 'review', element: <Review /> },
      { path: 'logs', element: <Logs /> },
      { path: 'settings', element: <Settings /> },
    ],
  },
])

