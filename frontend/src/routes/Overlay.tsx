import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X } from 'lucide-react'
import { BotShowList } from '@/components/BotShowList'
import { pickShow, visibleItems } from '@/lib/botShow'
import { invoke, listen } from '@/lib/tauri'
import type { AppConfig, BotResponse, OverlayConfig, ShowMeta } from '@/types'

// The always-on-top overlay window's entire UI. Mounted by main.tsx instead of
// the router when the window label is `overlay` — no sidebar, no statusbar, no
// route tree, and deliberately no `useTauriBridge`: this window wants one event
// (`bot-response`), not the whole app's state.
//
// The window itself is transparent; the rounded card below is the only thing
// that gets drawn. Everything inside the card is `pointer-events: none` so that
// a mousedown anywhere lands on the drag region and moves the window — except
// the close button, which opts back in.

// Only used if `get_config` fails — a blank overlay would be worse than a
// default-looking one. Mirrors `OverlayConfig::default()`.
const FALLBACK: OverlayConfig = {
  enabled: false,
  top_n: 3,
  opacity: 0.95,
  always_on_top: true,
}

export function Overlay() {
  const { t } = useTranslation()
  const [show, setShow] = useState<ShowMeta | null>(null)
  const [cfg, setCfg] = useState<OverlayConfig>(FALLBACK)

  useEffect(() => {
    const unlistens: Array<() => void> = []
    let cancelled = false

    // The window is opened by the backend, so config is already on disk;
    // `overlay-config` keeps us in step with later edits in Settings.
    invoke<AppConfig>('get_config')
      .then((c) => {
        if (!cancelled) setCfg(c.overlay)
      })
      .catch(() => {
        /* keep the fallback — a blank overlay would be worse than a default one */
      })

    listen<BotResponse>('bot-response', (r) => {
      const s = pickShow(r.meta)
      // Responses without a `show` block (e.g. a bare `none`) must not wipe
      // the last real suggestion off the screen.
      if (s) setShow(s)
    }).then((u) => unlistens.push(u))

    listen<OverlayConfig>('overlay-config', (c) => setCfg(c)).then((u) => unlistens.push(u))

    return () => {
      cancelled = true
      unlistens.forEach((u) => u())
    }
  }, [])

  const items = visibleItems(show, cfg.top_n)

  // Closing the overlay persists `enabled = false`, so it stays closed across
  // restarts. Settings is the way back.
  const close = () => {
    void invoke('set_overlay_enabled', { enabled: false }).catch(() => {})
  }

  return (
    <div
      data-tauri-drag-region
      className="h-screen w-screen overflow-hidden p-1 select-none cursor-grab active:cursor-grabbing"
    >
      <div
        data-tauri-drag-region
        className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-border bg-background shadow-lg [&_*]:pointer-events-none"
        style={{ opacity: cfg.opacity }}
      >
        <header
          data-tauri-drag-region
          className="flex shrink-0 items-center gap-1 px-2 pt-1.5 pb-1"
        >
          <span className="flex-1 truncate text-[11px] font-medium text-muted-foreground">
            {show?.title ?? t('tile.bot_show_default_title')}
          </span>
          <button
            type="button"
            onClick={close}
            aria-label={t('overlay.close')}
            title={t('overlay.close')}
            className="shrink-0 cursor-pointer text-muted-foreground hover:text-foreground"
            style={{ pointerEvents: 'auto' }}
          >
            <X className="size-3.5" />
          </button>
        </header>

        {/* min-h-0 is what gives this a definite height to hand down: without it
            a flex child refuses to shrink below its content, the rows never get
            a height to split, and the list collapses to hugging its content. */}
        <div className="min-h-0 flex-1 overflow-hidden px-1.5 pb-1.5">
          {items.length === 0 ? (
            <span className="px-1 text-xs text-muted-foreground">{t('overlay.empty')}</span>
          ) : (
            <BotShowList items={items} variant="overlay" />
          )}
        </div>
      </div>
    </div>
  )
}
