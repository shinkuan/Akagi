import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { HAS_TAURI, invoke } from '@/lib/tauri'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'

/** Which bundled default the user was last asked about (see below). */
const OFFERED_KEY = 'akagi.delayScriptOffered'

type DelayScriptStatus = {
  state: 'missing' | 'current' | 'outdated' | 'modified'
  /** Names the bundled default; changes when an update ships a new one. */
  version: string
}

/** An untouched `delay.lua` from an older version is updated on its own at
 *  startup. An edited one is not — the edits would be lost — so when an
 *  update ships a new default, ask once whether to replace it (the old file
 *  is kept as a backup). Either answer, or dismissing, settles it for that
 *  default; the next new default asks again. */
export function DelayScriptUpdateDialog() {
  const { t } = useTranslation()
  const [version, setVersion] = useState<string | null>(null)

  useEffect(() => {
    if (!HAS_TAURI) return
    let cancelled = false
    invoke<DelayScriptStatus>('delay_script_status')
      .then((s) => {
        if (cancelled || s.state !== 'modified') return
        if (localStorage.getItem(OFFERED_KEY) === s.version) return
        setVersion(s.version)
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [])

  const settle = () => {
    if (version) localStorage.setItem(OFFERED_KEY, version)
    setVersion(null)
  }

  const replace = async () => {
    try {
      const backup = await invoke<string | null>('replace_delay_script')
      toast.success(
        t('delay_script_update.replaced'),
        backup ? { description: t('delay_script_update.backup', { path: backup }) } : undefined,
      )
    } catch (e) {
      toast.error(t('delay_script_update.failed'), { description: String(e) })
    }
    settle()
  }

  return (
    <Dialog open={version !== null} onOpenChange={(open) => !open && settle()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('delay_script_update.title')}</DialogTitle>
          <DialogDescription>{t('delay_script_update.body')}</DialogDescription>
        </DialogHeader>
        <p className="text-xs text-muted-foreground">{t('delay_script_update.note')}</p>
        <DialogFooter>
          <Button variant="outline" onClick={settle}>
            {t('delay_script_update.keep')}
          </Button>
          <Button onClick={replace}>{t('delay_script_update.replace')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
