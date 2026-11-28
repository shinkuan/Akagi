import { useEffect, useRef, useState } from 'react'
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
import {
  canStartCalibration,
  formatGames,
  needsCalibrationPrompt,
} from '@/lib/mouseCalibration'
import type { MouseCalibrationStatus } from '@/types'

/** "Calibrate first": opened by the Settings autoplay switch, and whenever
 *  the saved settings enter "autoplay on for Mahjong Soul, not calibrated" —
 *  at startup, or after switching platform and saving. */
export function MouseCalibrationPrompt() {
  const { t } = useTranslation()
  const open = useMouseCalibrationStore((s) => s.promptOpen)
  const openPrompt = useMouseCalibrationStore((s) => s.openPrompt)
  const closePrompt = useMouseCalibrationStore((s) => s.closePrompt)
  const status = useMouseCalibrationStore((s) => s.status)
  const setStatus = useMouseCalibrationStore((s) => s.setStatus)
  const config = useConfigStore((s) => s.config)
  const [busy, setBusy] = useState(false)
  const needs = needsCalibrationPrompt(config, status)
  const needed = useRef(false)
  const canStart = canStartCalibration(config)

  useEffect(() => {
    if (needs && !needed.current) openPrompt()
    needed.current = needs
  }, [needs, openPrompt])

  const required = formatGames(status?.required_games ?? 2)
  const recommended = formatGames(status?.recommended_games ?? 5)

  const start = async () => {
    setBusy(true)
    try {
      setStatus(await invoke<MouseCalibrationStatus>('start_mouse_calibration'))
      closePrompt()
      toast.success(t('mouse_calibration.started'))
    } catch (e) {
      toast.error(t('mouse_calibration.start_failed'), { description: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && closePrompt()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('mouse_calibration.prompt_title')}</DialogTitle>
          <DialogDescription>{t('mouse_calibration.prompt_body')}</DialogDescription>
        </DialogHeader>
        <ul className="list-disc space-y-1 pl-5 text-xs text-muted-foreground">
          <li>{t('mouse_calibration.prompt_play', { required })}</li>
          <li>{t('mouse_calibration.prompt_more', { recommended })}</li>
          <li>{t('mouse_calibration.prompt_window')}</li>
          <li>{t('mouse_calibration.prompt_paused')}</li>
        </ul>
        {!canStart && (
          <p className="text-xs text-amber-500">{t('mouse_calibration.prompt_save_first')}</p>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={closePrompt}>
            {t('mouse_calibration.later')}
          </Button>
          <Button onClick={start} disabled={busy || !canStart}>
            {t('mouse_calibration.start')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
