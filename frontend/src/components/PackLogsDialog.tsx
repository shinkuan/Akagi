import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Eye, EyeOff, ShieldAlert } from 'lucide-react'
import { save } from '@tauri-apps/plugin-dialog'
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
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import type { LogSessionInfo, PackLogsRequest, PackSummary } from '@/types'

type Props = {
  open: boolean
  onOpenChange: (open: boolean) => void
  sessions: LogSessionInfo[]
  /** Pre-selected when the dialog opens: the session being viewed. */
  current: string | null
}

/** Pack chosen log sessions into one zip for a bug report. The `sensitive/`
 *  part of each session (full WebSocket frames) is left out unless the user
 *  opts in, and then encrypted with the password they give. */
export function PackLogsDialog({ open, onOpenChange, sessions, current }: Props) {
  const { t } = useTranslation()
  // Mounted only while open, so every open starts from these.
  const [all, setAll] = useState(false)
  const [chosen, setChosen] = useState<Set<string>>(() => new Set(current ? [current] : []))
  const [includeFull, setIncludeFull] = useState(false)
  const [password, setPassword] = useState('')
  const [showPassword, setShowPassword] = useState(false)
  const [busy, setBusy] = useState(false)

  const toggle = (name: string) =>
    setChosen((prev) => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name)
      else next.add(name)
      return next
    })

  const picked = all ? sessions.map((s) => s.name) : sessions.filter((s) => chosen.has(s.name)).map((s) => s.name)
  const legacyPicked = sessions.filter((s) => picked.includes(s.name) && s.legacy_layout).length

  const pack = async () => {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[-:T]/g, '')
    const dest = await save({
      title: t('logs.pack.save_title'),
      defaultPath: `akagi-logs-${stamp}.zip`,
      filters: [{ name: 'Zip', extensions: ['zip'] }],
    })
    if (!dest) return
    setBusy(true)
    try {
      const req: PackLogsRequest = {
        sessions: picked,
        include_sensitive: includeFull,
        password: includeFull && password ? password : null,
        dest,
      }
      const summary = await invoke<PackSummary>('pack_logs', { req })
      const full = !includeFull
        ? t('logs.pack.done_withheld')
        : summary.encrypted
          ? t('logs.pack.done_encrypted')
          : t('logs.pack.done_plain')
      toast.success(t('logs.pack.done'), {
        description: `${summary.path}\n${full}`,
      })
      onOpenChange(false)
    } catch (e) {
      toast.error(t('logs.pack.failed'), { description: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('logs.pack.title')}</DialogTitle>
          <DialogDescription>{t('logs.pack.description')}</DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-2 text-sm">
          <label className="flex items-center gap-2 cursor-pointer">
            <input type="radio" checked={all} onChange={() => setAll(true)} className="h-4 w-4" />
            {t('logs.pack.all', { count: sessions.length })}
          </label>
          <label className="flex items-center gap-2 cursor-pointer">
            <input type="radio" checked={!all} onChange={() => setAll(false)} className="h-4 w-4" />
            {t('logs.pack.choose')}
          </label>
          {!all && (
            <div className="ml-6 max-h-48 overflow-y-auto rounded border p-2 flex flex-col gap-1">
              {sessions.map((s) => (
                <label key={s.name} className="flex items-center gap-2 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={chosen.has(s.name)}
                    onChange={() => toggle(s.name)}
                    className="h-4 w-4"
                  />
                  <span className="font-mono">{s.name}</span>
                  {s.is_active && (
                    <span className="text-xs text-emerald-600 dark:text-emerald-300">({t('logs.active')})</span>
                  )}
                  {s.legacy_layout && (
                    <span className="text-xs text-muted-foreground">({t('logs.pack.legacy')})</span>
                  )}
                </label>
              ))}
            </div>
          )}
        </div>

        <div className="flex flex-col gap-3 text-sm">
          <div className="flex items-center gap-2">
            <Switch id="pack-include-full" checked={includeFull} onCheckedChange={setIncludeFull} />
            <Label htmlFor="pack-include-full">{t('logs.pack.include_full')}</Label>
          </div>
          {includeFull ? (
            <>
              <p className="flex gap-2 text-amber-600 dark:text-amber-400">
                <ShieldAlert className="h-4 w-4 shrink-0 mt-0.5" />
                {t('logs.pack.full_warning')}
              </p>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="pack-password">{t('logs.pack.password')}</Label>
                <div className="flex gap-2">
                  <Input
                    id="pack-password"
                    type={showPassword ? 'text' : 'password'}
                    autoComplete="new-password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={() => setShowPassword((v) => !v)}
                    aria-label={t('logs.pack.toggle_password')}
                  >
                    {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                  </Button>
                </div>
                <p className="text-muted-foreground">{t('logs.pack.password_hint')}</p>
                {password === '' && (
                  <p className="text-amber-600 dark:text-amber-400">{t('logs.pack.no_password')}</p>
                )}
              </div>
            </>
          ) : (
            legacyPicked > 0 && (
              <p className="text-muted-foreground">{t('logs.pack.legacy_note', { count: legacyPicked })}</p>
            )
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={busy}>
            {t('common.cancel')}
          </Button>
          <Button onClick={pack} disabled={busy || picked.length === 0}>
            {busy ? t('logs.pack.packing') : t('logs.pack.submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
