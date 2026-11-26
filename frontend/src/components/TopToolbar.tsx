import { useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { History, LayoutDashboard, Minus, Settings as SettingsIcon, Square, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { HAS_TAURI, invoke, listen } from '@/lib/tauri'
import { useBotStore } from '@/stores/botStore'
import { BOT_LABEL, CAPTURE_LABEL, TONE_DOT, botTone, captureTone } from '@/lib/statusIndicators'
import { useCaptureStore } from '@/stores/captureStore'
import type { AppConfig, BotStatus, CaptureStatus } from '@/types'

/**
 * The toolbar strip of the single-window layout (`top` webview, 42 px tall).
 *
 * One segmented control for the four assist modes, two status LEDs, the
 * product name, the page shortcuts, and the window buttons — the window is
 * frameless, so minimize / maximize / close live here.
 *
 * The modes are a *view* over four backend flags (`bot.enabled`,
 * `autoplay.enabled`, `autoplay.scope`, `overlay.enabled`); `set_assist_mode`
 * writes them atomically. Deriving the active segment from the flags (rather
 * than storing a mode of its own) keeps the toolbar honest when Settings
 * changes the same flags underneath it.
 */
type AssistMode = 'off' | 'recommend' | 'auto' | 'full_auto'

const MODES: AssistMode[] = ['off', 'recommend', 'auto', 'full_auto']

function deriveMode(cfg: AppConfig | null): AssistMode {
  if (!cfg) return 'off'
  if (!cfg.bot.enabled) return 'off'
  if (!cfg.autoplay.enabled) return 'recommend'
  return cfg.autoplay.scope === 'discards_only' ? 'auto' : 'full_auto'
}

export function TopToolbar() {
  const { t } = useTranslation()
  const [cfg, setCfg] = useState<AppConfig | null>(null)
  const [busy, setBusy] = useState(false)
  const bot = useBotStore((s) => s.status)
  const capture = useCaptureStore((s) => s.status)

  const refresh = () => {
    if (!HAS_TAURI) return
    invoke<AppConfig>('get_config')
      .then(setCfg)
      .catch(() => {})
  }

  useEffect(() => {
    refresh()
    // The panel and the tool windows can change the same flags; re-read on
    // focus so coming back from Settings shows the current mode.
    const unlisten = HAS_TAURI
      ? getCurrentWindow().onFocusChanged(({ payload }) => {
          if (payload) refresh()
        })
      : Promise.resolve(() => {})
    // Status dots: this webview is its own document, so it needs its own
    // subscription — the panel's hydration does not reach it.
    const subs = [
      listen<CaptureStatus>('capture-status', (s) => useCaptureStore.getState().set(s)),
      listen<BotStatus>('bot-status', (s) => useBotStore.getState().setStatus(s)),
    ]
    if (HAS_TAURI) {
      invoke<{ capture_status: CaptureStatus; bot_status: BotStatus }>('get_status')
        .then((snap) => {
          useCaptureStore.getState().set(snap.capture_status)
          useBotStore.getState().setStatus(snap.bot_status)
        })
        .catch(() => {})
    }
    return () => {
      unlisten.then((f) => f())
      subs.forEach((p) => p.then((f) => f()))
    }
  }, [])

  const mode = deriveMode(cfg)

  const setMode = async (next: AssistMode) => {
    if (busy) return
    setBusy(true)
    try {
      await invoke('set_assist_mode', { mode: next })
      refresh()
    } catch {
      /* the toast from the backend reports the failure */
    } finally {
      setBusy(false)
    }
  }

  const openPage = (route: string) => {
    invoke('open_tool_window', { route }).catch(() => {})
  }

  const win = HAS_TAURI ? getCurrentWindow() : null

  return (
    <div
      data-tauri-drag-region
      className="flex h-full items-center gap-1 bg-card px-2 text-xs text-foreground select-none"
    >
      {/* Mode switcher */}
      <div
        data-tauri-drag-region
        className="flex shrink-0 items-center rounded-md border border-border/60 p-0.5"
      >
        {MODES.map((m) => (
          <button
            key={m}
            type="button"
            disabled={busy}
            onClick={() => setMode(m)}
            className={cn(
              'rounded px-2 py-1 text-[11px] font-medium transition-colors',
              mode === m
                ? 'bg-primary text-primary-foreground'
                : 'text-muted-foreground hover:text-foreground',
            )}
          >
            {t(`mode.${m}`)}
          </button>
        ))}
      </div>

      {/* Connection + bot LEDs */}
      <div className="ml-2 flex shrink-0 items-center gap-3">
        <span className="flex items-center gap-1.5">
          <span className={cn('h-1.5 w-1.5 rounded-full', TONE_DOT[captureTone(capture.state)])} />
          <span className="text-muted-foreground">
            {t(CAPTURE_LABEL[captureTone(capture.state)])}
          </span>
        </span>
        <span className="flex items-center gap-1.5">
          <span className={cn('h-1.5 w-1.5 rounded-full', TONE_DOT[botTone(bot.state)])} />
          <span className="text-muted-foreground">{t(BOT_LABEL[bot.state])}</span>
        </span>
      </div>

      {/* Product name, centred */}
      <div data-tauri-drag-region className="flex flex-1 justify-center">
        <span className="text-[11px] font-semibold tracking-wide text-muted-foreground">
          AkagiMS
        </span>
      </div>

      {/* Page shortcuts */}
      <div className="flex shrink-0 items-center gap-0.5">
        <ToolButton label={t('nav.overview')} onClick={() => openPage('overview')}>
          <LayoutDashboard className="h-3.5 w-3.5" />
        </ToolButton>
        <ToolButton label={t('nav.history')} onClick={() => openPage('history')}>
          <History className="h-3.5 w-3.5" />
        </ToolButton>
        <ToolButton label={t('nav.settings')} onClick={() => openPage('settings')}>
          <SettingsIcon className="h-3.5 w-3.5" />
        </ToolButton>
      </div>

      {/* Window controls — the window is frameless */}
      <div className="ml-1 flex shrink-0 items-center">
        <WindowButton label="minimize" onClick={() => win?.minimize()}>
          <Minus className="h-3.5 w-3.5" />
        </WindowButton>
        <WindowButton label="maximize" onClick={() => win?.toggleMaximize()}>
          <Square className="h-3 w-3" />
        </WindowButton>
        <WindowButton label="close" danger onClick={() => win?.close()}>
          <X className="h-3.5 w-3.5" />
        </WindowButton>
      </div>
    </div>
  )
}

function ToolButton({
  label,
  onClick,
  children,
}: {
  label: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="rounded p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
    >
      {children}
    </button>
  )
}

function WindowButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string
  onClick: () => void
  danger?: boolean
  children: ReactNode
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={cn(
        'rounded p-1.5 text-muted-foreground transition-colors',
        danger ? 'hover:bg-red-500 hover:text-white' : 'hover:bg-muted hover:text-foreground',
      )}
    >
      {children}
    </button>
  )
}
