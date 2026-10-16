import { Link } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Bot, Shield, ScrollText, Gamepad2, Settings as SettingsIcon, Download, X } from 'lucide-react'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useBotStore } from '@/stores/botStore'
import { useCaptureStore } from '@/stores/captureStore'
import { useConfigStore } from '@/stores/configStore'
import { useAnalysisStore } from '@/stores/analysisStore'
import { useUiPrefsStore } from '@/stores/uiPrefsStore'
import { fmtTime } from '@/lib/format'
import { captureTitleKey } from '@/lib/captureConfig'
import { AkagiWordmark } from '@/components/BrandLogo'
import { AKAGIMS_DOWNLOAD_URL, AKAGIMS_GITHUB_URL, openExternal } from '@/lib/external'
import akagimsScreenshot from '@/assets/akagims-fullauto.jpg'

const DOT: Record<string, string> = {
  ready:    'bg-emerald-500',
  running:  'bg-emerald-500',
  loading:  'bg-amber-500',
  starting: 'bg-amber-500',
  idle:     'bg-zinc-500',
  stopped:  'bg-zinc-500',
  error:    'bg-red-500',
}

export function Overview() {
  const { t } = useTranslation()
  const bot = useBotStore((s) => s.status)
  const capture = useCaptureStore((s) => s.status)
  const logDir = useConfigStore((s) => s.logDir)
  const lastAnalysis = useAnalysisStore((s) => s.updatedAt)

  const captureTitle = t(captureTitleKey('kind' in capture ? capture.kind : null))
  const captureDetail = 'descriptor' in capture && capture.descriptor ? capture.descriptor : '—'

  return (
    <div className="p-6 flex flex-col gap-6 w-full">
      <div className="flex justify-center pt-2 pb-1">
        <AkagiWordmark className="h-28" />
      </div>
      <header>
        <h1 className="text-2xl font-semibold">{t('overview.title')}</h1>
        <p className="text-sm text-muted-foreground">{t('overview.description')}</p>
      </header>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <StatusCard
          icon={Bot}
          title={t('overview.bot_card_title')}
          state={bot.state}
          detail={'bot' in bot && bot.bot ? bot.bot : '—'}
          extra={'actor_id' in bot ? `actor_id ${bot.actor_id}` : 'error' in bot ? bot.error : undefined}
        />
        <StatusCard
          icon={Shield}
          title={captureTitle}
          state={capture.state}
          detail={captureDetail}
          extra={'error' in capture ? capture.error : undefined}
        />
        <Card>
          <CardHeader className="flex flex-row items-center gap-2">
            <ScrollText className="h-4 w-4 text-muted-foreground" />
            <CardTitle className="text-sm uppercase tracking-wider">{t('overview.log_session')}</CardTitle>
          </CardHeader>
          <CardContent>
            <div className="font-mono text-xs break-all">{logDir || '—'}</div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm uppercase tracking-wider">{t('overview.last_analysis')}</CardTitle>
        </CardHeader>
        <CardContent>
          <span className="font-mono text-sm">
            {lastAnalysis ? fmtTime(new Date(lastAnalysis)) : '—'}
          </span>
        </CardContent>
      </Card>

      <div className="flex gap-2">
        <Button asChild>
          <Link to="/game" className="gap-1.5">
            <Gamepad2 className="h-4 w-4" />
            {t('overview.open_dashboard')}
          </Link>
        </Button>
        <Button asChild variant="outline">
          <Link to="/settings" className="gap-1.5">
            <SettingsIcon className="h-4 w-4" />
            {t('settings.title')}
          </Link>
        </Button>
      </div>

      <AkagiMsPromoCard />
    </div>
  )
}

// Dismissible AkagiMS promo — the persistent, quieter counterpart to the
// one-time announcement dialog (the sidebar footer link stays even after
// this card is closed).
function AkagiMsPromoCard() {
  const { t } = useTranslation()
  const dismissed = useUiPrefsStore((s) => s.akagimsCardDismissed)
  const dismiss = useUiPrefsStore((s) => s.markAkagimsCardDismissed)
  if (dismissed) return null

  return (
    <Card className="relative overflow-hidden">
      <Button
        variant="ghost"
        size="icon"
        className="absolute right-2 top-2 h-7 w-7 text-muted-foreground hover:text-foreground"
        onClick={dismiss}
        aria-label={t('announcements.akagims.card_dismiss')}
      >
        <X className="h-4 w-4" />
      </Button>
      <CardContent className="flex flex-col sm:flex-row items-start gap-4 pt-6">
        <button
          type="button"
          className="shrink-0 w-full sm:w-56 cursor-pointer"
          onClick={() => openExternal(AKAGIMS_GITHUB_URL)}
          aria-label="AkagiMS"
        >
          <img
            src={akagimsScreenshot}
            alt={t('announcements.akagims.screenshot_alt')}
            className="w-full rounded-md border border-border"
          />
        </button>
        <div className="min-w-0 flex flex-col gap-2">
          <div className="text-sm font-semibold">{t('announcements.akagims.card_title')}</div>
          <p className="text-xs text-muted-foreground">{t('announcements.akagims.card_desc')}</p>
          <div className="flex flex-wrap gap-2 pt-1">
            <Button size="sm" onClick={() => openExternal(AKAGIMS_DOWNLOAD_URL)} className="gap-1.5">
              <Download className="h-4 w-4" />
              {t('announcements.akagims.view_download')}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={() => openExternal(AKAGIMS_GITHUB_URL)}
            >
              GitHub
            </Button>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function StatusCard({
  icon: Icon, title, state, detail, extra,
}: {
  icon: typeof Bot
  title: string
  state: string
  detail: string
  extra?: string
}) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-center gap-2">
        <Icon className="h-4 w-4 text-muted-foreground" />
        <CardTitle className="text-sm uppercase tracking-wider">{title}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-2">
          <span className={`h-2 w-2 rounded-full ${DOT[state] ?? 'bg-zinc-500'}`} />
          <span className="capitalize text-sm font-medium">{state}</span>
        </div>
        <div className="text-xs font-mono text-muted-foreground mt-1">{detail}</div>
        {extra && <div className="text-xs text-muted-foreground mt-1">{extra}</div>}
      </CardContent>
    </Card>
  )
}
