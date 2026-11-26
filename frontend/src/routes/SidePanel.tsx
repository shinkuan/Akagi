import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Bot, Cloud, Sparkles } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { BotShowList } from '@/components/BotShowList'
import { PurchaseDialog } from '@/components/PurchaseDialog'
import { useTauriBridge } from '@/hooks/useTauriBridge'
import { HAS_TAURI, invoke, listen } from '@/lib/tauri'
import { useGameStore } from '@/stores/gameStore'
import { useBotStore } from '@/stores/botStore'
import { useNotifyStore } from '@/stores/notifyStore'
import { useConfigStore } from '@/stores/configStore'
import { useApiStatusStore } from '@/stores/apiStatusStore'
import { bakazeFor, kyokuLabel } from '@/lib/format'
import { pickShow, visibleItems } from '@/lib/botShow'
import { cn } from '@/lib/utils'

/**
 * The right-hand panel of the single-window layout (the `main` webview,
 * narrowed to 320 px): three summary cards and a status footer.
 *
 * It is deliberately a *summary* — the full pages (Overview, History,
 * Settings) open from the toolbar in their own windows and keep every
 * existing control. Everything here reads the same stores the full pages do,
 * so the two never disagree.
 */
export function SidePanel() {
  const { t } = useTranslation()
  const game = useGameStore((s) => s.game)
  const bot = useBotStore((s) => s.status)
  const cfg = useConfigStore((s) => s.config)
  const responses = useNotifyStore((s) => s.responses)
  const apiDegraded = useApiStatusStore((s) => s.degraded)
  const [endpoint, setEndpoint] = useState<string | null>(null)
  const [buyOpen, setBuyOpen] = useState(false)

  // The panel is its own webview: it needs the same event hydration the full
  // app gets from <App/>, or the cards would sit empty.
  useTauriBridge()

  useEffect(() => {
    if (!HAS_TAURI) return
    const off = listen<string | null>('connection-endpoint', setEndpoint)
    return () => {
      off.then((f) => f())
    }
  }, [])

  const show = useMemo(() => {
    const last = responses[responses.length - 1]
    return last ? pickShow(last.meta) : null
  }, [responses])
  const items = visibleItems(show, 3)

  const us = game?.our_seat ?? 0
  const selfWind = game ? bakazeFor(us, game.oya, game.num_players) : null
  const idle = bot.state !== 'ready'

  const apiEnabled = cfg?.bot?.api?.enabled ?? false

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto bg-background p-3 text-sm">
      {/* ---- Bot state ---- */}
      <Card className="gap-2 py-3">
        <CardHeader className="px-3 pb-1">
          <CardTitle className="flex items-center justify-between text-sm">
            <span className="flex items-center gap-2">
              <Bot className="h-4 w-4 text-muted-foreground" />
              {t('panel.bot_status')}
            </span>
            <span
              className={cn(
                'rounded-full px-2 py-0.5 text-[10px] font-medium',
                idle
                  ? 'bg-muted text-muted-foreground'
                  : 'bg-emerald-500/15 text-emerald-400',
              )}
            >
              {idle ? t('panel.status_idle') : t('panel.status_ready')}
            </span>
          </CardTitle>
        </CardHeader>
        <CardContent className="px-3">
          {!game ? (
            <p className="py-3 text-center text-xs text-muted-foreground">
              {t('panel.waiting_decision')}
            </p>
          ) : (
            <div className="grid grid-cols-4 gap-2 text-center">
              <Stat label={t('mahjong.kyoku')} value={kyokuLabel(game.bakaze, game.kyoku)} />
              <Stat label={t('mahjong.honba')} value={`×${game.honba}`} />
              <Stat label={t('mahjong.kyotaku')} value={`×${game.kyotaku}`} />
              <Stat label={t('mahjong.self_wind')} value={selfWind ?? '—'} />
            </div>
          )}
        </CardContent>
      </Card>

      {/* ---- AI recommendation ---- */}
      <Card className="gap-2 py-3">
        <CardHeader className="px-3 pb-1">
          <CardTitle className="flex items-center justify-between text-sm">
            <span className="flex items-center gap-2">
              <Sparkles className="h-4 w-4 text-muted-foreground" />
              {t('panel.ai_recommend')}
            </span>

          </CardTitle>
        </CardHeader>
        <CardContent className="px-3">
          {items.length === 0 ? (
            <p className="py-6 text-center text-xs text-muted-foreground">
              {t('panel.waiting_data')}
            </p>
          ) : (
            <BotShowList items={items} variant="tile" />
          )}
        </CardContent>
      </Card>

      {/* ---- Cloud inference ---- */}
      <Card className="gap-2 py-3">
        <CardHeader className="px-3 pb-1">
          <CardTitle className="flex items-center justify-between text-sm">
            <span className="flex items-center gap-2">
              <Cloud className="h-4 w-4 text-muted-foreground" />
              {t('panel.cloud')}
            </span>
            <span className="text-[10px] text-muted-foreground">
              {apiDegraded
                ? t('panel.cloud_degraded')
                : apiEnabled
                  ? t('panel.cloud_active')
                  : t('panel.cloud_off')}
            </span>
          </CardTitle>
        </CardHeader>
        <CardContent className="grid gap-2 px-3">
          <p className="text-xs leading-relaxed text-muted-foreground">
            {t('panel.cloud_desc')}
          </p>
          <Button size="sm" variant="outline" className="w-full" onClick={() => setBuyOpen(true)}>
            {t('bots.api.buy')}
          </Button>
        </CardContent>
      </Card>

      {/* ---- Footer ---- */}
      <div className="mt-auto flex items-center justify-between px-1 text-[10px] text-muted-foreground">
        <span>{t('panel.footer_idle')}</span>
        <span className="truncate tabular-nums">{endpoint ?? '—'}</span>
      </div>

      {buyOpen && cfg && (
        <PurchaseDialog
          baseUrl={cfg.bot.api.base_url}
          proxy={cfg.bot.api.proxy}
          currentKey={cfg.bot.api.key}
          onClose={() => setBuyOpen(false)}
          onNewKey={(key) => {
            const next = { ...cfg, bot: { ...cfg.bot, api: { ...cfg.bot.api, key } } }
            useConfigStore.getState().setConfig(next)
            invoke('update_config', { newConfig: next }).catch(() => {})
          }}
        />
      )}
    </div>
  )
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-0.5">
      <span className="text-[10px] text-muted-foreground">{label}</span>
      <span className="text-sm font-semibold tabular-nums">{value}</span>
    </div>
  )
}
