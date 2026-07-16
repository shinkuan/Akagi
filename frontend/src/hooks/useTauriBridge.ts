import { useEffect } from 'react'
import { HAS_TAURI, invoke, listen } from '@/lib/tauri'
import type {
  AnalysisResult,
  BotResponse,
  BotStatus,
  CaptureStatus,
  GameRecord,
  GameStateSnapshot,
  HistoryEvent,
  MahgenView,
  MjaiEvent,
  Notification,
  OverlayConfig,
  Snapshot,
} from '@/types'
import { useGameStore } from '@/stores/gameStore'
import { useAnalysisStore } from '@/stores/analysisStore'
import { useBotStore } from '@/stores/botStore'
import { useCaptureStore } from '@/stores/captureStore'
import { useNotifyStore } from '@/stores/notifyStore'
import { useApiStatusStore } from '@/stores/apiStatusStore'
import { useInstallStore } from '@/stores/installStore'
import { useConfigStore } from '@/stores/configStore'
import { useHistoryStore } from '@/stores/historyStore'
import { toast, type ToastSeverity } from '@/components/ui/sonner'
import { notifyTitle, notifyBody } from '@/lib/notifyText'

// Backend `Notification.level` ∈ {info,success,warn,error}; toast helper
// uses `warning`. Map across.
const TOAST_SEVERITY: Record<Notification['level'], ToastSeverity> = {
  info: 'info',
  success: 'success',
  warn: 'warning',
  error: 'error',
}

// One-shot bridge mounted from <App>. Subscribes to all Tauri events,
// hydrates initial state, and unsubscribes on unmount.
export function useTauriBridge() {
  useEffect(() => {
    if (!HAS_TAURI) return

    const unlistens: Array<() => void> = []
    let cancelled = false

    const refreshGame = async () => {
      try {
        const [snap, view] = await Promise.all([
          invoke<GameStateSnapshot | null>('get_game_snapshot'),
          invoke<MahgenView | null>('get_mahgen_view'),
        ])
        if (cancelled) return
        useGameStore.getState().setGame(snap)
        useGameStore.getState().setView(view)
      } catch {
        /* ignore: backend may not be ready */
      }
    }

    ;(async () => {
      try {
        const status = await invoke<Snapshot>('get_status')
        if (cancelled) return
        useConfigStore.getState().setConfig(status.config)
        useConfigStore.getState().setLogDir(status.log_dir)
        useBotStore.getState().setStatus(status.bot_status)
        useCaptureStore.getState().set(status.capture_status)
      } catch {
        /* ignore */
      }
      await refreshGame()
      try {
        const a = await invoke<AnalysisResult | null>('get_analysis')
        if (!cancelled) useAnalysisStore.getState().set(a)
      } catch {
        /* ignore */
      }
      // Game history: load all records once at startup. The History
      // tab is the only consumer; loading lazily there forces a
      // round-trip on first nav, so do it eagerly while the bridge
      // is warm. Explicit large `limit` defends against any backend
      // version that still applies a default cap on `limit=0`.
      try {
        useHistoryStore.getState().setLoading(true)
        const records = await invoke<GameRecord[]>('list_game_history', {
          filter: null,
          limit: 1_000_000,
          offset: 0,
        })
        if (!cancelled) useHistoryStore.getState().setRecords(records)
      } catch {
        /* ignore: store stays empty */
      } finally {
        useHistoryStore.getState().setLoading(false)
      }
    })()

    listen<MjaiEvent>('mjai-event', (e) => {
      // Fresh game: clear any lingering online-API outage from the last one.
      if (e.type === 'start_game') useApiStatusStore.getState().reset()
      useNotifyStore.getState().pushEvent(e)
      void refreshGame()
    }).then((u) => unlistens.push(u))

    listen<AnalysisResult>('analysis-result', (a) => {
      useAnalysisStore.getState().set(a)
    }).then((u) => unlistens.push(u))

    listen<BotStatus>('bot-status', (s) => {
      useBotStore.getState().setStatus(s)
    }).then((u) => unlistens.push(u))

    listen<CaptureStatus>('capture-status', (s) => {
      useCaptureStore.getState().set(s)
    }).then((u) => unlistens.push(u))

    listen<BotResponse>('bot-response', (r) => {
      useNotifyStore.getState().pushResponse(r)
    }).then((u) => unlistens.push(u))

    // The overlay window can turn itself off (its × button). Mirror that back
    // into the config store so the Game page's toggle doesn't keep claiming the
    // overlay is open.
    listen<OverlayConfig>('overlay-config', (o) => {
      useConfigStore.getState().setOverlay(o)
    }).then((u) => unlistens.push(u))

    listen<HistoryEvent>('history-recorded', (ev) => {
      if (ev.kind === 'recorded') {
        useHistoryStore.getState().prepend(ev.record)
      } else if (ev.kind === 'deleted') {
        useHistoryStore.getState().remove(ev.id)
      }
    }).then((u) => unlistens.push(u))

    listen<Notification>('notify', (n) => {
      useNotifyStore.getState().pushToast(n)
      // Drive the persistent "Online API" health indicator (Statusbar) off the
      // same channel the backend uses for degrade/recover toasts.
      if (n.id === 'native-api-health') {
        useApiStatusStore
          .getState()
          .setDegraded(n.level === 'warn' || n.level === 'error', n.body)
      }
      // Feed env install/sync progress to the blocking overlay (which is
      // shown for the duration of `withInstallBlock`). Ids: `bot-install-*`
      // (GitHub install/reinstall) and `bot-sync-*` ("Reinstall environment").
      if (n.id && (n.id.startsWith('bot-install-') || n.id.startsWith('bot-sync-'))) {
        useInstallStore.getState().setProgress(n)
      }
      toast[TOAST_SEVERITY[n.level]](notifyTitle(n), {
        description: notifyBody(n),
        id: n.id,
        ...(n.sticky ? { duration: Infinity } : {}),
      })
    }).then((u) => unlistens.push(u))

    return () => {
      cancelled = true
      unlistens.forEach((u) => u())
    }
  }, [])
}
