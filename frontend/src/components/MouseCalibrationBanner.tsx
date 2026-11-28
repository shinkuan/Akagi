import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { invoke } from '@/lib/tauri'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useConfigStore } from '@/stores/configStore'
import { useMouseCalibrationStore } from '@/stores/mouseCalibrationStore'
import { canFinish, formatGames, hasEnough } from '@/lib/mouseCalibration'
import type { FinishedCalibration, MouseCalibrationStatus } from '@/types'

/** Shown above every page while a mouse calibration is recording. */
export function MouseCalibrationBanner() {
  const { t } = useTranslation()
  const status = useMouseCalibrationStore((s) => s.status)
  const setStatus = useMouseCalibrationStore((s) => s.setStatus)
  const setConfig = useConfigStore((s) => s.setConfig)
  const [busy, setBusy] = useState(false)
  const [confirmCancel, setConfirmCancel] = useState(false)

  if (!status?.active) return null

  const finish = async () => {
    setBusy(true)
    try {
      const r = await invoke<FinishedCalibration>('finish_mouse_calibration')
      setConfig(r.config)
      const kept = r.kept.map((k) => t(`settings.autoplay.mouse.field_${k}`, { defaultValue: k }))
      toast.success(
        t('mouse_calibration.finished'),
        kept.length ? { description: t('mouse_calibration.kept', { fields: kept.join(', ') }) } : undefined,
      )
    } catch (e) {
      toast.error(t('mouse_calibration.finish_failed'), { description: String(e) })
    } finally {
      setBusy(false)
    }
  }

  const cancel = async () => {
    setConfirmCancel(false)
    try {
      setStatus(await invoke<MouseCalibrationStatus>('cancel_mouse_calibration'))
      toast(t('mouse_calibration.cancelled'))
    } catch (e) {
      toast.error(t('mouse_calibration.cancel_failed'), { description: String(e) })
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-amber-500/30 bg-amber-500/10 px-4 py-2 text-xs">
      <span className="font-medium text-amber-700 dark:text-amber-400">
        {t('mouse_calibration.banner_title')}
      </span>
      <span className="tabular-nums">
        {t('mouse_calibration.banner_progress', {
          games: formatGames(status.games),
          required: formatGames(status.required_games),
          recommended: formatGames(status.recommended_games),
          kyoku: status.kyoku,
        })}
      </span>
      <span className="text-muted-foreground">
        {status.recording
          ? t('mouse_calibration.banner_recording')
          : t('mouse_calibration.banner_waiting')}
      </span>
      {hasEnough(status) && (
        <span className="text-emerald-700 dark:text-emerald-400">
          {t('mouse_calibration.banner_enough')}
        </span>
      )}
      <span className="ml-auto flex gap-2">
        <Button size="sm" variant="outline" onClick={() => setConfirmCancel(true)} disabled={busy}>
          {t('mouse_calibration.cancel')}
        </Button>
        <Button size="sm" onClick={finish} disabled={busy || !canFinish(status)}>
          {t('mouse_calibration.finish')}
        </Button>
      </span>
      <Dialog open={confirmCancel} onOpenChange={setConfirmCancel}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t('mouse_calibration.cancel_title')}</DialogTitle>
            <DialogDescription>{t('mouse_calibration.cancel_body')}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmCancel(false)}>
              {t('mouse_calibration.keep_recording')}
            </Button>
            <Button variant="destructive" onClick={cancel}>
              {t('mouse_calibration.cancel_confirm')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
