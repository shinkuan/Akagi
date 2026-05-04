import { useEffect, useState } from 'react'
import { Link, useBlocker } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { invoke } from '@/lib/tauri'
import { useSidebar } from '@/hooks/useSidebar'
import { useCaptureStore } from '@/stores/captureStore'
import { useConfigStore } from '@/stores/configStore'
import {
  SCALE_DEFAULT,
  SCALE_MAX,
  SCALE_MIN,
  SCALE_STEP,
  useUiPrefsStore,
} from '@/stores/uiPrefsStore'
import {
  PLATFORMS,
  isKnownDefaultStartUrl,
  platformInfo,
} from '@/lib/platforms'
import type { AppConfig, CaptureMode, DetectedBrowser, PlatformKind } from '@/types'

export function Settings() {
  const { t } = useTranslation()
  const stored = useConfigStore((s) => s.config)
  const setStored = useConfigStore((s) => s.setConfig)
  const [draft, setDraft] = useState<AppConfig | null>(stored)
  const [saving, setSaving] = useState(false)
  const [err, setErr] = useState<string | null>(null)

  useEffect(() => {
    if (stored) setDraft(stored)
  }, [stored])

  useEffect(() => {
    if (!stored) {
      invoke<AppConfig>('get_config').then(setStored).catch(() => {})
    }
  }, [stored, setStored])

  const dirty = !!draft && !!stored && JSON.stringify(draft) !== JSON.stringify(stored)

  const blocker = useBlocker(
    ({ currentLocation, nextLocation }) =>
      dirty && currentLocation.pathname !== nextLocation.pathname,
  )

  useEffect(() => {
    if (!dirty) return
    const handler = (e: BeforeUnloadEvent) => {
      e.preventDefault()
      e.returnValue = ''
    }
    window.addEventListener('beforeunload', handler)
    return () => window.removeEventListener('beforeunload', handler)
  }, [dirty])

  if (!draft) {
    return <div className="p-6 text-muted-foreground">{t('settings.loading_config')}</div>
  }

  const save = async () => {
    setSaving(true)
    setErr(null)
    try {
      await invoke('update_config', { newConfig: draft })
      setStored(draft)
    } catch (e) {
      setErr(String(e))
    } finally {
      setSaving(false)
    }
  }

  const saveAndLeave = async () => {
    setSaving(true)
    setErr(null)
    try {
      await invoke('update_config', { newConfig: draft })
      setStored(draft)
      blocker.proceed?.()
    } catch (e) {
      setErr(String(e))
      blocker.reset?.()
    } finally {
      setSaving(false)
    }
  }

  const discardAndLeave = () => {
    setDraft(stored)
    blocker.proceed?.()
  }

  return (
    <div className="p-6 w-full flex flex-col gap-6">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('settings.title')}</h1>
        <div className="flex gap-2">
          <Button variant="ghost" asChild>
            <Link to="/setup?rerun=1">{t('settings.rerun_setup')}</Link>
          </Button>
          <Button variant="outline" onClick={() => setDraft(stored)} disabled={!dirty || saving}>
            {t('common.reset')}
          </Button>
          <Button onClick={save} disabled={!dirty || saving}>
            {saving ? t('common.saving') : t('common.save')}
          </Button>
        </div>
      </header>

      {err && (
        <div className="rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-400">
          {err}
        </div>
      )}

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.general')}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <Field label={t('settings.language')}>
            <Select
              value={draft.general.language}
              onValueChange={(v) => setDraft({ ...draft, general: { ...draft.general, language: v } })}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="zh-TW">繁體中文</SelectItem>
                <SelectItem value="zh-CN">简体中文</SelectItem>
                <SelectItem value="ja">日本語</SelectItem>
                <SelectItem value="en">English</SelectItem>
              </SelectContent>
            </Select>
          </Field>
          <UiScaleField />
          <SidebarHoverField />
        </CardContent>
      </Card>

      <PlatformCard draft={draft} setDraft={setDraft} />

      <CaptureCard draft={draft} setDraft={setDraft} />

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.logging')}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <Field label={t('settings.directory')}>
            <Input
              value={draft.logging.dir}
              onChange={(e) => setDraft({ ...draft, logging: { ...draft.logging, dir: e.target.value } })}
            />
          </Field>
          <Field label={t('settings.app_log_level')}>
            <Input
              value={draft.logging.level}
              onChange={(e) => setDraft({ ...draft, logging: { ...draft.logging, level: e.target.value } })}
              placeholder="info"
            />
          </Field>
          <Field label={t('settings.crate_log_level')}>
            <Input
              value={draft.logging.all_level}
              onChange={(e) => setDraft({ ...draft, logging: { ...draft.logging, all_level: e.target.value } })}
              placeholder="warn"
            />
          </Field>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('settings.bots')}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4">
          <Toggle
            label={t('settings.bot_enabled')}
            value={draft.bot.enabled}
            onChange={(v) => setDraft({ ...draft, bot: { ...draft.bot, enabled: v } })}
          />
          <Toggle
            label={t('settings.auto_sync')}
            value={draft.bot.auto_sync}
            onChange={(v) => setDraft({ ...draft, bot: { ...draft.bot, auto_sync: v } })}
          />
          <Field label={t('settings.active_bot_4p')}>
            <Input
              value={draft.bot.active_4p}
              onChange={(e) => setDraft({ ...draft, bot: { ...draft.bot, active_4p: e.target.value } })}
              placeholder="mortal"
            />
          </Field>
          <Field label={t('settings.active_bot_3p')}>
            <Input
              value={draft.bot.active_3p}
              onChange={(e) => setDraft({ ...draft, bot: { ...draft.bot, active_3p: e.target.value } })}
              placeholder={t('common.none_paren')}
            />
          </Field>
          <Field label={t('settings.bot_directory')}>
            <Input
              value={draft.bot.dir}
              onChange={(e) => setDraft({ ...draft, bot: { ...draft.bot, dir: e.target.value } })}
            />
          </Field>
        </CardContent>
      </Card>

      <AutoplayCard draft={draft} setDraft={setDraft} />

      <Dialog
        open={blocker.state === 'blocked'}
        onOpenChange={(open) => {
          if (!open) blocker.reset?.()
        }}
      >
        <DialogContent showCloseButton={false}>
          <DialogHeader>
            <DialogTitle>{t('settings.unsaved_title')}</DialogTitle>
            <DialogDescription>
              {t('settings.unsaved_desc')}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="bg-transparent p-0 border-0 mx-0 mb-0">
            <Button variant="outline" size="sm" onClick={() => blocker.reset?.()} disabled={saving}>
              {t('common.stay')}
            </Button>
            <Button variant="destructive" size="sm" onClick={discardAndLeave} disabled={saving}>
              {t('common.discard')}
            </Button>
            <Button size="sm" onClick={saveAndLeave} disabled={saving}>
              {saving ? t('common.saving') : t('settings.save_and_leave')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="grid gap-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
    </div>
  )
}

function Toggle({ label, value, onChange }: { label: string; value: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-center justify-between">
      <Label>{label}</Label>
      <Switch checked={value} onCheckedChange={onChange} />
    </div>
  )
}

function SidebarHoverField() {
  const { t } = useTranslation()
  const isHoverOpen = useSidebar((s) => s.settings.isHoverOpen)
  const disabled = useSidebar((s) => s.settings.disabled)
  const setSettings = useSidebar((s) => s.setSettings)
  return (
    <div className="grid gap-1.5">
      <Toggle
        label={t('settings.appearance_sidebar_peek')}
        value={isHoverOpen}
        onChange={(v) => setSettings({ isHoverOpen: v })}
      />
      <Toggle
        label={t('settings.appearance_hide_sidebar')}
        value={disabled}
        onChange={(v) => setSettings({ disabled: v })}
      />
      <span className="text-xs text-muted-foreground">
        {t('settings.appearance_sidebar_hint')}
      </span>
    </div>
  )
}

function UiScaleField() {
  const { t } = useTranslation()
  const scale = useUiPrefsStore((s) => s.scale)
  const setScale = useUiPrefsStore((s) => s.setScale)
  const resetScale = useUiPrefsStore((s) => s.resetScale)
  const pct = Math.round(scale * 100)
  return (
    <div className="grid gap-1.5">
      <div className="flex items-center justify-between">
        <Label>{t('settings.ui_scale')}</Label>
        <div className="flex items-center gap-2">
          <span className="font-mono text-sm tabular-nums w-12 text-right">{pct}%</span>
          <Button
            variant="outline"
            size="sm"
            onClick={resetScale}
            disabled={scale === SCALE_DEFAULT}
          >
            {t('common.reset')}
          </Button>
        </div>
      </div>
      <input
        type="range"
        min={SCALE_MIN}
        max={SCALE_MAX}
        step={SCALE_STEP}
        value={scale}
        onChange={(e) => setScale(parseFloat(e.target.value))}
        className="w-full accent-primary"
        aria-label={t('settings.ui_scale')}
      />
      <span className="text-xs text-muted-foreground">
        {t('settings.ui_scale_hint')}
      </span>
    </div>
  )
}

function PlatformCard({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const current = draft.platform.kind
  const setKind = (kind: PlatformKind) => {
    if (kind === current) return
    // If the user hasn't customised the Chromium start URL, swap it to
    // the new platform's default so the next launch lands on the right
    // game. A user-customised URL is left alone — the URL field below
    // shows the platform default as a hint either way.
    const oldStart = draft.capture.chromium.start_url
    const nextStart = isKnownDefaultStartUrl(oldStart)
      ? platformInfo(kind).defaultStartUrl
      : oldStart
    setDraft({
      ...draft,
      platform: { kind },
      capture: {
        ...draft.capture,
        chromium: { ...draft.capture.chromium, start_url: nextStart },
      },
    })
  }
  const info = platformInfo(current)
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.platform_card_title')}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Field
          label={t('settings.platform_game_label')}
          hint={t('settings.platform_game_hint')}
        >
          <Select value={current} onValueChange={(v) => setKind(v as PlatformKind)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PLATFORMS.map((p) => (
                <SelectItem key={p.kind} value={p.kind}>
                  {t(p.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <p className="text-xs text-muted-foreground">{t(info.descriptionKey)}</p>
      </CardContent>
    </Card>
  )
}

function AutoplayCard({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const ap = draft.autoplay ?? {
    enabled: false,
    majsoul: {
      pre_click_delay_min_ms: 1000,
      pre_click_delay_max_ms: 3000,
      inter_click_delay_ms: 300,
      hover_delay_ms: 150,
      click_hold_ms: 50,
      dealer_first_discard_extra_delay_ms: 2000,
    },
  }
  const captureIsChromium = draft.capture?.mode === 'chromium'
  const setApField = (patch: Partial<typeof ap>) =>
    setDraft({ ...draft, autoplay: { ...ap, ...patch } })
  const setMajsoulField = (patch: Partial<typeof ap.majsoul>) =>
    setDraft({
      ...draft,
      autoplay: { ...ap, majsoul: { ...ap.majsoul, ...patch } },
    })
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.autoplay.title')}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Toggle
          label={t('settings.autoplay.enable')}
          value={ap.enabled}
          onChange={(v) => setApField({ enabled: v })}
        />
        <p className="text-xs text-muted-foreground">
          {t('settings.autoplay.enable_help')}
        </p>
        {ap.enabled && !captureIsChromium && (
          <p className="text-xs text-amber-500">
            {t('settings.autoplay.requires_chromium')}
          </p>
        )}
        <Field label={t('settings.autoplay.pre_click_delay_min')}>
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.pre_click_delay_min_ms}
            onChange={(e) =>
              setMajsoulField({
                pre_click_delay_min_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field label={t('settings.autoplay.pre_click_delay_max')}>
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.pre_click_delay_max_ms}
            onChange={(e) =>
              setMajsoulField({
                pre_click_delay_max_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field label={t('settings.autoplay.inter_click_delay')}>
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.inter_click_delay_ms}
            onChange={(e) =>
              setMajsoulField({
                inter_click_delay_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field
          label={t('settings.autoplay.hover_delay')}
          hint={t('settings.autoplay.hover_delay_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.hover_delay_ms}
            onChange={(e) =>
              setMajsoulField({
                hover_delay_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field label={t('settings.autoplay.click_hold')}>
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.click_hold_ms}
            onChange={(e) =>
              setMajsoulField({
                click_hold_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field
          label={t('settings.autoplay.dealer_first_discard_extra_delay')}
          hint={t('settings.autoplay.dealer_first_discard_extra_delay_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.dealer_first_discard_extra_delay_ms}
            onChange={(e) =>
              setMajsoulField({
                dealer_first_discard_extra_delay_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <p className="text-xs text-muted-foreground">
          {t('settings.autoplay.platform_note')}
        </p>
      </CardContent>
    </Card>
  )
}

function CaptureCard({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const mode: CaptureMode = draft.capture?.mode ?? 'mitm'
  const chromium = draft.capture?.chromium ?? {
    executable: '',
    user_data_dir: '',
    start_url: platformInfo(draft.platform.kind).defaultStartUrl,
    cft_channel: 'stable',
    force_cft: false,
    extra_args: [],
  }
  const [detected, setDetected] = useState<DetectedBrowser[] | null>(null)
  const [detecting, setDetecting] = useState(false)

  const probe = async () => {
    setDetecting(true)
    try {
      const list = await invoke<DetectedBrowser[]>('detect_system_chrome')
      setDetected(list)
    } catch {
      setDetected([])
    } finally {
      setDetecting(false)
    }
  }

  useEffect(() => {
    if (mode === 'chromium' && detected === null) {
      probe()
    }
  }, [mode]) // eslint-disable-line react-hooks/exhaustive-deps

  const setMode = (v: CaptureMode) =>
    setDraft({
      ...draft,
      capture: {
        mode: v,
        chromium,
      },
    })
  const setChromium = (patch: Partial<typeof chromium>) =>
    setDraft({
      ...draft,
      capture: {
        mode,
        chromium: { ...chromium, ...patch },
      },
    })

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <CardTitle>{t('settings.capture_card_title')}</CardTitle>
          <CaptureStatusBar />
        </div>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Field label={t('settings.capture_mode_label')} hint={t('settings.capture_mode_hint')}>
          <Select value={mode} onValueChange={(v) => setMode(v as CaptureMode)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="mitm">{t('settings.capture_mitm_option')}</SelectItem>
              <SelectItem value="chromium">{t('settings.capture_chromium_option')}</SelectItem>
            </SelectContent>
          </Select>
        </Field>

        {mode === 'mitm' && (
          <>
            <Toggle
              label={t('settings.proxy_enabled')}
              value={draft.proxy.enabled}
              onChange={(v) => setDraft({ ...draft, proxy: { ...draft.proxy, enabled: v } })}
            />
            <Field label={t('settings.address')}>
              <Input
                value={draft.proxy.addr}
                onChange={(e) => setDraft({ ...draft, proxy: { ...draft.proxy, addr: e.target.value } })}
                placeholder="127.0.0.1:23410"
              />
            </Field>
            <Field label={t('settings.ca_dir')} hint={t('settings.ca_dir_hint')}>
              <Input
                value={draft.proxy.ca_dir}
                onChange={(e) => setDraft({ ...draft, proxy: { ...draft.proxy, ca_dir: e.target.value } })}
              />
            </Field>
          </>
        )}

        {mode === 'chromium' && (
          <>
            <Field label={t('settings.browser_executable')} hint={t('settings.browser_executable_hint')}>
              <Input
                value={chromium.executable}
                onChange={(e) => setChromium({ executable: e.target.value })}
                placeholder="/usr/bin/google-chrome"
              />
            </Field>
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs text-muted-foreground">
                {detecting
                  ? t('common.detecting')
                  : detected === null
                    ? t('settings.detect_status_initial')
                    : detected.length === 0
                      ? t('settings.detect_status_none')
                      : t('settings.detect_status_detected', { paths: detected.map((d) => d.path).join(', ') })}
              </span>
              <Button variant="outline" size="sm" onClick={probe} disabled={detecting}>
                {detecting ? t('common.detecting') : t('common.detect')}
              </Button>
            </div>
            <Field label={t('settings.user_data_dir')} hint={t('settings.user_data_dir_hint')}>
              <Input
                value={chromium.user_data_dir}
                onChange={(e) => setChromium({ user_data_dir: e.target.value })}
                placeholder={t('common.default')}
              />
            </Field>
            <Field
              label={t('settings.start_url')}
              hint={t('settings.start_url_hint', {
                platform: t(platformInfo(draft.platform.kind).labelKey),
                url: platformInfo(draft.platform.kind).defaultStartUrl,
              })}
            >
              <Input
                value={chromium.start_url}
                onChange={(e) => setChromium({ start_url: e.target.value })}
                placeholder={platformInfo(draft.platform.kind).defaultStartUrl}
              />
            </Field>
            <Toggle
              label={t('settings.force_cft')}
              value={chromium.force_cft}
              onChange={(v) => setChromium({ force_cft: v })}
            />
            <CftPanel chromium={chromium} setChromium={setChromium} />
          </>
        )}
      </CardContent>
    </Card>
  )
}

const CAPTURE_DOT: Record<string, string> = {
  running: 'bg-emerald-500',
  starting: 'bg-amber-500',
  stopped: 'bg-zinc-500',
  error: 'bg-red-500',
}

function CaptureStatusBar() {
  const { t } = useTranslation()
  const status = useCaptureStore((s) => s.status)
  const [busy, setBusy] = useState(false)

  const dot = CAPTURE_DOT[status.state] ?? 'bg-zinc-500'
  const detail =
    'descriptor' in status && status.descriptor
      ? status.descriptor
      : status.state === 'stopped'
        ? '—'
        : ''

  const restart = async () => {
    setBusy(true)
    try {
      await invoke('restart_capture')
    } catch {
      /* surfaced via notify */
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex items-center gap-3">
      <div className="flex items-center gap-1.5 text-xs">
        <span className={`h-2 w-2 rounded-full ${dot}`} />
        <span className="capitalize font-medium">{status.state}</span>
        {detail && (
          <span className="font-mono text-muted-foreground truncate max-w-[200px]" title={detail}>
            {detail}
          </span>
        )}
      </div>
      <Button variant="outline" size="sm" onClick={restart} disabled={busy}>
        {busy ? t('settings.restarting') : t('settings.restart_capture')}
      </Button>
    </div>
  )
}

function CftPanel({
  chromium,
  setChromium,
}: {
  chromium: AppConfig['capture']['chromium']
  setChromium: (patch: Partial<AppConfig['capture']['chromium']>) => void
}) {
  const { t } = useTranslation()
  const [installed, setInstalled] = useState<string[] | null>(null)
  const [busy, setBusy] = useState<'idle' | 'downloading' | 'removing'>('idle')

  const refresh = async () => {
    try {
      const list = await invoke<string[]>('list_cft_installed')
      setInstalled(list)
    } catch {
      setInstalled([])
    }
  }

  useEffect(() => {
    refresh()
  }, [])

  const download = async () => {
    setBusy('downloading')
    try {
      await invoke<string>('download_chrome_for_testing', {
        channel: chromium.cft_channel || 'stable',
      })
      await refresh()
    } catch (e) {
      console.error('CfT download failed:', e)
    } finally {
      setBusy('idle')
    }
  }

  const remove = async (version: string) => {
    setBusy('removing')
    try {
      await invoke('remove_chrome_for_testing', { version })
      await refresh()
    } catch (e) {
      console.error('CfT remove failed:', e)
    } finally {
      setBusy('idle')
    }
  }

  return (
    <div className="grid gap-2 rounded-md border border-border/50 p-3">
      <div className="flex items-center justify-between">
        <Label>{t('settings.cft_title')}</Label>
        <span className="text-xs text-muted-foreground">
          {installed === null
            ? t('settings.cft_status_loading')
            : installed.length === 0
              ? t('settings.cft_status_none')
              : t('settings.cft_status_count', { count: installed.length })}
        </span>
      </div>
      <Field label={t('settings.cft_channel')} hint={t('settings.cft_channel_hint')}>
        <Input
          value={chromium.cft_channel}
          onChange={(e) => setChromium({ cft_channel: e.target.value })}
          placeholder="stable"
        />
      </Field>
      <div className="flex items-center justify-end gap-2">
        <Button variant="outline" size="sm" onClick={refresh} disabled={busy !== 'idle'}>
          {t('common.refresh')}
        </Button>
        <Button onClick={download} disabled={busy !== 'idle'} size="sm">
          {busy === 'downloading' ? t('common.downloading') : t('common.download')}
        </Button>
      </div>
      {installed && installed.length > 0 && (
        <ul className="grid gap-1 text-sm">
          {installed.map((v) => (
            <li key={v} className="flex items-center justify-between rounded bg-muted/40 px-2 py-1">
              <span>{v}</span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => remove(v)}
                disabled={busy !== 'idle'}
              >
                {t('common.remove')}
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
