import { useTranslation } from 'react-i18next'
import { useConfigStore } from '@/stores/configStore'
import { useApiStatusStore } from '@/stores/apiStatusStore'
import { useCaptureStore } from '@/stores/captureStore'
import { useBotStore } from '@/stores/botStore'
import {
  BOT_LABEL,
  botTone,
  CAPTURE_LABEL,
  captureTone,
  TONE_DOT,
  type IndicatorTone,
} from '@/lib/statusIndicators'
import { NATIVE_3P, NATIVE_4P } from '@/lib/nativeBots'

function IndicatorDot({
  tone,
  label,
  title,
}: {
  tone: IndicatorTone
  label: string
  title?: string
}) {
  return (
    <span className="flex items-center gap-1.5" title={title}>
      <span className={`h-1.5 w-1.5 rounded-full ${TONE_DOT[tone]}`} />
      {label}
    </span>
  )
}

export function Statusbar() {
  const { t } = useTranslation()
  const config = useConfigStore((s) => s.config)
  const degraded = useApiStatusStore((s) => s.degraded)
  const apiError = useApiStatusStore((s) => s.error)
  const capture = useCaptureStore((s) => s.status)
  const bot = useBotStore((s) => s.status)

  // Capture LED — "is Akagi reading the game?" (proxy / capture transport).
  // Tooltip surfaces the capture descriptor, or the error message on failure,
  // so the user can see *why* it's down.
  const capTone = captureTone(capture.state)
  const captureTitle =
    capture.state === 'error'
      ? capture.error
      : capture.state === 'running' || capture.state === 'starting'
        ? capture.descriptor
        : undefined

  // Bot LED — "is the recommendation engine up?". Independent of capture: the
  // bot can error (broken env, failed spawn) while capture keeps running.
  const botT = botTone(bot.state)
  const botTitle =
    bot.state === 'error'
      ? bot.error
      : bot.state === 'loading'
        ? `${bot.bot} · ${t(`status.stage_${bot.stage}`)}`
        : bot.state === 'ready' || bot.state === 'stopped'
          ? bot.bot
          : undefined

  // "Using online API" = a native bot is active AND the cloud API is fully
  // configured (enabled + URL + key). Config-derived so it reflects intent even
  // between games; health (degraded) comes from the live backend notifications.
  const api = config?.bot.api
  const nativeActive =
    config?.bot.active_4p === NATIVE_4P ||
    config?.bot.active_4p === NATIVE_3P ||
    config?.bot.active_3p === NATIVE_4P ||
    config?.bot.active_3p === NATIVE_3P
  const usingApi =
    !!api &&
    api.enabled &&
    api.base_url.trim() !== '' &&
    api.key.trim() !== '' &&
    nativeActive

  return (
    <footer className="flex items-center justify-between border-t border-border px-4 py-1.5 text-xs text-muted-foreground bg-muted/30">
      <IndicatorDot
        tone={capTone}
        label={t(CAPTURE_LABEL[capTone])}
        title={captureTitle}
      />
      <span className="flex items-center gap-3">
        {usingApi && (
          <IndicatorDot
            tone={degraded ? 'fault' : 'live'}
            label={degraded ? t('status.api_degraded') : t('status.api_active')}
            title={
              degraded
                ? apiError ?? t('status.api_degraded_hint')
                : t('status.api_active_hint')
            }
          />
        )}
        <IndicatorDot
          tone={botT}
          label={t(BOT_LABEL[bot.state])}
          title={botTitle}
        />
      </span>
    </footer>
  )
}
