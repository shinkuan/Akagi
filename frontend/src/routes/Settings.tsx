import { useEffect, useState } from 'react'
import { Link, useBlocker } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { getVersion } from '@tauri-apps/api/app'
import { toast } from 'sonner'
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
import { HAS_TAURI, invoke } from '@/lib/tauri'
import { openExternal } from '@/lib/external'
import { useSidebar } from '@/hooks/useSidebar'
import { useAnnouncementStore } from '@/stores/announcementStore'
import { useCaptureStore } from '@/stores/captureStore'
import { useConfigStore } from '@/stores/configStore'
import { selectHasNotifiableUpdate, useUpdaterStore } from '@/stores/updaterStore'
import {
  SCALE_DEFAULT,
  SCALE_MAX,
  SCALE_MIN,
  SCALE_STEP,
  useUiPrefsStore,
} from '@/stores/uiPrefsStore'
import {
  CUSTOM_BASE_VARS,
  THEME_MODES,
  THEME_PALETTES,
  useThemeStore,
  type CustomBaseVar,
  type ThemeMode,
  type ThemePalette,
} from '@/stores/themeStore'
import {
  PLATFORMS,
  isKnownDefaultStartUrl,
  platformInfo,
} from '@/lib/platforms'
import {
  effectiveCaptureMode,
  withCaptureMode,
  withChromiumPatch,
  withIngestPatch,
} from '@/lib/captureConfig'
import { IngestUserscriptPanel } from '@/components/IngestUserscriptPanel'
import {
  OVERLAY_OPACITY_MAX,
  OVERLAY_OPACITY_MIN,
  OVERLAY_TOP_N_MAX,
  OVERLAY_TOP_N_MIN,
} from '@/types'
import type {
  AppConfig,
  CaptureMode,
  DelayMode,
  DelayModelConfig,
  DetectedBrowser,
  GithubMirrorMode,
  NetworkConfig,
  OverlayConfig,
  PlatformKind,
} from '@/types'

export function Settings() {
  const { t, i18n } = useTranslation()
  const stored = useConfigStore((s) => s.config)
  const setStored = useConfigStore((s) => s.setConfig)
  const [draft, setDraft] = useState<AppConfig | null>(stored)
  const [saving, setSaving] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  // Riichi City autoplay is MITM frame injection — pop a risk warning the
  // moment the draft combines the two, from either direction (platform
  // switched to Riichi City while autoplay is on, or autoplay switched on
  // while the platform is Riichi City).
  const [rcAutoplayWarnOpen, setRcAutoplayWarnOpen] = useState(false)

  useEffect(() => {
    // Sync the editable draft from the store when it (re)loads.
    // eslint-disable-next-line react-hooks/set-state-in-effect
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
          {/* Language applies instantly and self-persists to localStorage via
              i18next, mirroring the setup wizard and the sidebar switcher.
              It is intentionally decoupled from the config draft/save flow —
              nothing reads config.general.language to drive the UI language. */}
          <Field label={t('settings.language')}>
            <Select
              value={i18n.language}
              onValueChange={(v) => void i18n.changeLanguage(v)}
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
          <div className="grid gap-1.5">
            <Toggle
              label={t('settings.developer_mode')}
              value={draft.general.developer_mode}
              onChange={(v) =>
                setDraft({ ...draft, general: { ...draft.general, developer_mode: v } })
              }
            />
            <span className="text-xs text-muted-foreground">
              {t('settings.developer_mode_hint')}
            </span>
          </div>
        </CardContent>
      </Card>

      <AppearanceCard />

      <OverlayCard draft={draft} setDraft={setDraft} />

      <PlatformCard
        draft={draft}
        setDraft={setDraft}
        onRiichiCityAutoplay={() => setRcAutoplayWarnOpen(true)}
      />

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

      <AutoplayCard
        draft={draft}
        setDraft={setDraft}
        onRiichiCityAutoplay={() => setRcAutoplayWarnOpen(true)}
      />

      <NetworkCard draft={draft} setDraft={setDraft} />

      <UpdatesCard />

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

      <Dialog open={rcAutoplayWarnOpen} onOpenChange={setRcAutoplayWarnOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('settings.autoplay.rc_warning_title')}</DialogTitle>
            <DialogDescription>
              {t('settings.autoplay.rc_warning_desc')}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter className="bg-transparent p-0 border-0 mx-0 mb-0">
            <Button size="sm" onClick={() => setRcAutoplayWarnOpen(false)}>
              {t('settings.autoplay.rc_warning_ack')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

/** GitHub download routing — accelerator-mirror fallback for regions where
 *  GitHub is blocked. Used by the in-app updater and the bot installer;
 *  Chrome-for-Testing downloads fall back to a mirror automatically and
 *  need no setting here. */
function NetworkCard({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const n = draft.network
  const patch = (p: Partial<NetworkConfig>) =>
    setDraft({ ...draft, network: { ...n, ...p } })
  const custom = n.github_custom_mirror.trim()
  const customInvalid = custom !== '' && !/^https?:\/\/\S+$/.test(custom)

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.network_title')}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Field
          label={t('settings.github_mirror_label')}
          hint={t('settings.github_mirror_hint')}
        >
          <Select
            value={n.github_mirror_mode}
            onValueChange={(v) => patch({ github_mirror_mode: v as GithubMirrorMode })}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="auto">{t('settings.github_mirror_auto')}</SelectItem>
              <SelectItem value="direct">{t('settings.github_mirror_direct')}</SelectItem>
              <SelectItem value="mirror">{t('settings.github_mirror_mirror')}</SelectItem>
            </SelectContent>
          </Select>
        </Field>
        <Field
          label={t('settings.custom_mirror_label')}
          hint={t('settings.custom_mirror_hint')}
        >
          <Input
            value={n.github_custom_mirror}
            onChange={(e) => patch({ github_custom_mirror: e.target.value })}
            placeholder="https://gh-proxy.com"
            className="font-mono"
            disabled={n.github_mirror_mode === 'direct'}
          />
          {customInvalid && (
            <span className="text-xs text-red-400">
              {t('settings.custom_mirror_invalid')}
            </span>
          )}
        </Field>
      </CardContent>
    </Card>
  )
}

/** The always-on-top suggestion overlay. Applied on save, like every other
 *  card here — `update_config` opens, closes, or retunes the window. */
function OverlayCard({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const o = draft.overlay
  const patch = (p: Partial<OverlayConfig>) =>
    setDraft({ ...draft, overlay: { ...o, ...p } })

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.overlay')}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="grid gap-1.5">
          <Toggle
            label={t('settings.overlay_enabled')}
            value={o.enabled}
            onChange={(v) => patch({ enabled: v })}
          />
          <span className="text-xs text-muted-foreground">
            {t('settings.overlay_enabled_hint')}
          </span>
        </div>

        <div className="grid gap-1.5">
          <div className="flex items-center justify-between">
            <Label>{t('settings.overlay_top_n')}</Label>
            <span className="w-12 text-right font-mono text-sm tabular-nums">{o.top_n}</span>
          </div>
          <input
            type="range"
            min={OVERLAY_TOP_N_MIN}
            max={OVERLAY_TOP_N_MAX}
            step={1}
            value={o.top_n}
            onChange={(e) => patch({ top_n: parseInt(e.target.value, 10) })}
            className="w-full accent-primary"
            aria-label={t('settings.overlay_top_n')}
          />
        </div>

        <div className="grid gap-1.5">
          <div className="flex items-center justify-between">
            <Label>{t('settings.overlay_opacity')}</Label>
            <span className="w-12 text-right font-mono text-sm tabular-nums">
              {Math.round(o.opacity * 100)}%
            </span>
          </div>
          <input
            type="range"
            min={OVERLAY_OPACITY_MIN}
            max={OVERLAY_OPACITY_MAX}
            step={0.05}
            value={o.opacity}
            onChange={(e) => patch({ opacity: parseFloat(e.target.value) })}
            className="w-full accent-primary"
            aria-label={t('settings.overlay_opacity')}
          />
        </div>

        <div className="grid gap-1.5">
          <Toggle
            label={t('settings.overlay_always_on_top')}
            value={o.always_on_top}
            onChange={(v) => patch({ always_on_top: v })}
          />
          <span className="text-xs text-muted-foreground">
            {t('settings.overlay_always_on_top_hint')}
          </span>
        </div>
      </CardContent>
    </Card>
  )
}

function AppearanceCard() {
  const { t } = useTranslation()
  const mode = useThemeStore((s) => s.mode)
  const palette = useThemeStore((s) => s.palette)
  const setMode = useThemeStore((s) => s.setMode)
  const setPalette = useThemeStore((s) => s.setPalette)
  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('settings.appearance')}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Field label={t('settings.mode')}>
          <Select value={mode} onValueChange={(v) => setMode(v as ThemeMode)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {THEME_MODES.map((m) => (
                <SelectItem key={m} value={m}>
                  {t(`settings.mode_${m}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <Field label={t('settings.theme')}>
          <Select value={palette} onValueChange={(v) => setPalette(v as ThemePalette)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {THEME_PALETTES.map((p) => (
                <SelectItem key={p} value={p}>
                  {t(`settings.theme_${p}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        {palette === 'custom' && <CustomEditor />}
        <UiScaleField />
        <SidebarHoverField />
      </CardContent>
    </Card>
  )
}

// Native <input type="color"> only accepts opaque hex. Use the browser's CSS
// engine to canonicalise any input value (oklch/hsl/rgb/named) to rgb, then
// emit hex — no extra dependency.
function toHex(value: string | undefined): string {
  if (!value) return '#000000'
  const trimmed = value.trim()
  if (/^#[0-9a-f]{6}$/i.test(trimmed)) return trimmed.toLowerCase()
  if (typeof document === 'undefined') return '#000000'
  const probe = document.createElement('div')
  probe.style.color = trimmed
  document.body.appendChild(probe)
  const computed = getComputedStyle(probe).color
  probe.remove()
  const m = computed.match(/^rgba?\(\s*(\d+)[,\s]+(\d+)[,\s]+(\d+)/i)
  if (!m) return '#000000'
  const hex = [m[1], m[2], m[3]]
    .map((n) => parseInt(n, 10).toString(16).padStart(2, '0'))
    .join('')
  return `#${hex}`
}

function CustomEditor() {
  const { t } = useTranslation()
  const custom = useThemeStore((s) => s.custom)
  const setCustomVar = useThemeStore((s) => s.setCustomVar)
  const resetCustom = useThemeStore((s) => s.resetCustom)
  const importCustom = useThemeStore((s) => s.importCustom)
  const [importInput, setImportInput] = useState('')
  const [importing, setImporting] = useState(false)
  const [importError, setImportError] = useState<string | null>(null)
  const [importSuccess, setImportSuccess] = useState(false)

  const handleImport = async () => {
    if (!importInput.trim()) return
    setImporting(true)
    setImportError(null)
    setImportSuccess(false)
    try {
      await importCustom(importInput)
      setImportInput('')
      setImportSuccess(true)
    } catch (e) {
      setImportError(e instanceof Error ? e.message : String(e))
    } finally {
      setImporting(false)
    }
  }

  return (
    <div className="grid gap-4 rounded-md border border-border/50 p-3">
      <p className="text-xs text-muted-foreground">{t('settings.custom_hint')}</p>
      <CustomColorRow which="light" vars={custom.light} onChange={setCustomVar} />
      <CustomColorRow which="dark" vars={custom.dark} onChange={setCustomVar} />
      <div className="grid gap-2 rounded-md border border-border/40 p-2">
        <Label className="text-sm">{t('settings.custom_import')}</Label>
        <span className="text-xs text-muted-foreground">
          {t('settings.custom_import_hint')}
        </span>
        <textarea
          value={importInput}
          onChange={(e) => {
            setImportInput(e.target.value)
            setImportError(null)
            setImportSuccess(false)
          }}
          placeholder="https://tweakcn.com/themes/<id>"
          rows={2}
          className="w-full rounded-md border border-input bg-background px-3 py-2 font-mono text-xs focus:outline-none focus:ring-2 focus:ring-ring resize-y"
          spellCheck={false}
        />
        {importError && (
          <span className="text-xs text-red-500">{importError}</span>
        )}
        {importSuccess && (
          <span className="text-xs text-emerald-600 dark:text-emerald-400">
            {t('settings.custom_import_ok')}
          </span>
        )}
        <div className="flex justify-end">
          <Button
            variant="outline"
            size="sm"
            onClick={handleImport}
            disabled={importing || !importInput.trim()}
          >
            {importing
              ? t('settings.custom_importing')
              : t('settings.custom_import_button')}
          </Button>
        </div>
      </div>
      <div className="flex justify-end">
        <Button variant="outline" size="sm" onClick={resetCustom}>
          {t('settings.custom_reset')}
        </Button>
      </div>
    </div>
  )
}

function CustomColorRow({
  which,
  vars,
  onChange,
}: {
  which: 'light' | 'dark'
  vars: Partial<Record<CustomBaseVar, string>>
  onChange: (which: 'light' | 'dark', name: CustomBaseVar, value: string) => void
}) {
  const { t } = useTranslation()
  return (
    <div className="grid gap-2">
      <Label className="text-sm">{t(`settings.custom_${which}`)}</Label>
      <div className="grid grid-cols-5 gap-2">
        {CUSTOM_BASE_VARS.map((name) => (
          <div key={name} className="grid gap-1">
            <span className="text-[10px] text-muted-foreground truncate" title={name}>
              {t(`settings.custom_var_${name}`)}
            </span>
            <input
              type="color"
              aria-label={`${which} ${name}`}
              value={toHex(vars[name])}
              onChange={(e) => onChange(which, name, e.target.value)}
              className="h-9 w-full rounded border border-border cursor-pointer bg-transparent"
            />
          </div>
        ))}
      </div>
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
  onRiichiCityAutoplay,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
  onRiichiCityAutoplay: () => void
}) {
  const { t } = useTranslation()
  const current = draft.platform.kind
  const setKind = (kind: PlatformKind) => {
    if (kind === current) return
    if (kind === 'RiichiCity' && (draft.autoplay?.enabled ?? false)) {
      onRiichiCityAutoplay()
    }
    // If the user hasn't customised the Chromium start URL, swap it to
    // the new platform's default so the next launch lands on the right
    // game. A user-customised URL is left alone — the URL field below
    // shows the platform default as a hint either way.
    const info = platformInfo(kind)
    const oldStart = draft.capture.chromium.start_url
    const nextStart = isKnownDefaultStartUrl(oldStart)
      ? info.defaultStartUrl
      : oldStart
    setDraft({
      ...draft,
      platform: { kind },
      capture: {
        ...draft.capture,
        // Native-only platforms (no web client) are MITM-only — force the
        // mode so we never persist an unusable Chromium capture for them.
        mode: info.supportsChromium ? draft.capture.mode : 'mitm',
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
  onRiichiCityAutoplay,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
  onRiichiCityAutoplay: () => void
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
      verify_input_ms: 300,
      click_retries: 2,
      reload_after_failures: 3,
      dealer_first_discard_extra_delay_ms: 2000,
    },
    delay: defaultDelayModel(),
  }
  const delay = ap.delay ?? defaultDelayModel()
  const captureIsChromium = draft.capture?.mode === 'chromium'
  // Riichi City autoplay runs through the MITM proxy (frame injection), so
  // the Chromium-mode requirement only applies to the click platforms.
  const platformIsRiichiCity = draft.platform?.kind === 'RiichiCity'
  const setApField = (patch: Partial<typeof ap>) =>
    setDraft({ ...draft, autoplay: { ...ap, ...patch } })
  const setMajsoulField = (patch: Partial<typeof ap.majsoul>) =>
    setDraft({
      ...draft,
      autoplay: { ...ap, majsoul: { ...ap.majsoul, ...patch } },
    })
  const setDelayField = (patch: Partial<DelayModelConfig>) =>
    setDraft({
      ...draft,
      autoplay: { ...ap, delay: { ...delay, ...patch } },
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
          onChange={(v) => {
            setApField({ enabled: v })
            if (v && platformIsRiichiCity) onRiichiCityAutoplay()
          }}
        />
        <p className="text-xs text-muted-foreground">
          {t('settings.autoplay.enable_help')}
        </p>
        {ap.enabled && !captureIsChromium && !platformIsRiichiCity && (
          <p className="text-xs text-amber-500">
            {t('settings.autoplay.requires_chromium')}
          </p>
        )}
        {/* Delay policy: exactly one of legacy (fixed uniform) or the
            Lua-scripted human-like model is active. */}
        <Field label={t('settings.autoplay.delay_mode')}>
          <Select
            value={delay.mode}
            onValueChange={(v) => setDelayField({ mode: v as DelayMode })}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="lua">
                {t('settings.autoplay.delay_mode_lua')}
              </SelectItem>
              <SelectItem value="legacy">
                {t('settings.autoplay.delay_mode_legacy')}
              </SelectItem>
            </SelectContent>
          </Select>
        </Field>
        {delay.mode === 'lua' && (
          <p className="text-xs text-muted-foreground">
            {t('settings.autoplay.delay_mode_lua_help')}
          </p>
        )}
        {delay.mode === 'legacy' && (
          <>
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
          </>
        )}
        <Field
          label={t('settings.autoplay.min_delay')}
          hint={t('settings.autoplay.min_delay_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={delay.min_delay_ms}
            onChange={(e) =>
              // Clamp: a typed negative would fail u32 deserialization
              // on save (min={0} doesn't block typing a minus sign).
              setDelayField({
                min_delay_ms: Math.max(0, Number(e.target.value || 0)),
              })
            }
          />
        </Field>
        <Field
          label={t('settings.autoplay.min_button_delay')}
          hint={t('settings.autoplay.min_button_delay_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={delay.min_button_delay_ms}
            onChange={(e) =>
              setDelayField({
                min_button_delay_ms: Math.max(0, Number(e.target.value || 0)),
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
          label={t('settings.autoplay.verify_input')}
          hint={t('settings.autoplay.verify_input_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.verify_input_ms}
            onChange={(e) =>
              setMajsoulField({
                verify_input_ms: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field
          label={t('settings.autoplay.click_retries')}
          hint={t('settings.autoplay.click_retries_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.click_retries}
            onChange={(e) =>
              setMajsoulField({
                click_retries: Number(e.target.value || 0),
              })
            }
          />
        </Field>
        <Field
          label={t('settings.autoplay.reload_after_failures')}
          hint={t('settings.autoplay.reload_after_failures_hint')}
        >
          <Input
            type="number"
            inputMode="numeric"
            min={0}
            value={ap.majsoul.reload_after_failures}
            onChange={(e) =>
              setMajsoulField({
                reload_after_failures: Number(e.target.value || 0),
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

/** Mirror of `DelayModelConfig::default()` on the Rust side. */
function defaultDelayModel(): DelayModelConfig {
  return {
    mode: 'lua',
    min_delay_ms: 1000,
    min_button_delay_ms: 1600,
    distribution: 'log_normal',
    lognormal: {
      dahai_tedashi: [0.87, 0.62],
      dahai_tsumogiri: [0.52, 0.53],
      post_call_dahai: [0.52, 0.42],
      reach: [1.1, 0.55],
      claim: [0.26, 0.57],
      hora: [0.15, 0.5],
    },
    bank_on_long_thought: true,
    riichi_extra_ms: 0,
    kan_extra_ms: 0,
    safety_margin_ms: 1000,
    bank_use_fraction: 0.25,
    bank_max_single_ms: 5000,
    no_budget_cap_ms: 15000,
  }
}

function CaptureCard({
  draft,
  setDraft,
}: {
  draft: AppConfig
  setDraft: (c: AppConfig) => void
}) {
  const { t } = useTranslation()
  const supportsChromium = platformInfo(draft.platform.kind).supportsChromium
  // Native-only platforms (no web client) are MITM-only; present MITM as the
  // effective mode even if a stale config selected a browser mode.
  const mode: CaptureMode = effectiveCaptureMode(draft.capture.mode, supportsChromium)
  const chromium = draft.capture.chromium
  const captureStatus = useCaptureStore((s) => s.status)
  const savedIngestAddr = useConfigStore((s) => s.config?.capture.ingest.addr)
  // "Install" fetches the script from the running backend, so it needs
  // ingest capture live on the address being shown — not just selected in
  // an unsaved draft.
  const ingestLive =
    captureStatus.state === 'running' &&
    captureStatus.kind === 'ingest' &&
    savedIngestAddr === draft.capture.ingest.addr
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
      // probe() sets detecting/detected state; intentional on mode switch.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      probe()
    }
  }, [mode]) // eslint-disable-line react-hooks/exhaustive-deps

  const setMode = (v: CaptureMode) =>
    setDraft({ ...draft, capture: withCaptureMode(draft.capture, v) })
  const setChromium = (patch: Partial<typeof chromium>) =>
    setDraft({ ...draft, capture: withChromiumPatch(draft.capture, patch) })
  const setIngestAddr = (addr: string) =>
    setDraft({ ...draft, capture: withIngestPatch(draft.capture, { addr }) })

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
              <SelectItem value="chromium" disabled={!supportsChromium}>
                {t('settings.capture_chromium_option')}
              </SelectItem>
              <SelectItem value="ingest" disabled={!supportsChromium}>
                {t('ingest.option')}
              </SelectItem>
            </SelectContent>
          </Select>
        </Field>

        {!supportsChromium && (
          <p className="text-xs text-amber-500">{t('settings.capture_mitm_only')}</p>
        )}

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
            <Toggle
              label={t('settings.block_telemetry')}
              value={draft.proxy.block_telemetry}
              onChange={(v) => setDraft({ ...draft, proxy: { ...draft.proxy, block_telemetry: v } })}
            />
            <span className="text-xs text-muted-foreground">
              {t('settings.block_telemetry_hint')}
            </span>
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

        {mode === 'ingest' && (
          <>
            <Field label={t('ingest.addr_label')} hint={t('ingest.addr_hint')}>
              <Input
                value={draft.capture.ingest.addr}
                onChange={(e) => setIngestAddr(e.target.value)}
                placeholder="127.0.0.1:23411"
              />
            </Field>
            <IngestUserscriptPanel addr={draft.capture.ingest.addr} endpointLive={ingestLive} />
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
    // Mount-time load of the installed-bots list; refresh() sets state.
    // eslint-disable-next-line react-hooks/set-state-in-effect
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

function UpdatesCard() {
  const { t, i18n } = useTranslation()
  const autoCheckEnabled = useUpdaterStore((s) => s.autoCheckEnabled)
  const setAutoCheckEnabled = useUpdaterStore((s) => s.setAutoCheckEnabled)
  const lastChecked = useUpdaterStore((s) => s.lastChecked)
  const skippedVersion = useUpdaterStore((s) => s.skippedVersion)
  const clearSkipped = useUpdaterStore((s) => s.clearSkipped)
  const checkNow = useUpdaterStore((s) => s.checkNow)
  const checking = useUpdaterStore((s) => s.checking)
  const pending = useUpdaterStore((s) => s.pendingUpdate)
  const hasNotifiable = useUpdaterStore(selectHasNotifiableUpdate)
  const openDialog = useUpdaterStore((s) => s.openDialog)

  const [currentVersion, setCurrentVersion] = useState('—')
  useEffect(() => {
    if (!HAS_TAURI) return
    getVersion().then(setCurrentVersion).catch(() => {})
  }, [])

  const handleCheck = async () => {
    const before = useUpdaterStore.getState().pendingUpdate?.latest_tag ?? null
    await checkNow(true)
    const after = useUpdaterStore.getState().pendingUpdate
    if (!after) {
      toast.success(t('updates.settings.up_to_date'))
    } else if (after.latest_tag !== before) {
      // New release surfaced for the first time this session — let the
      // existing UpdateNotifier toast logic handle it instead of toasting
      // twice. If it's the same release we already knew about, the
      // explicit click deserves a one-shot acknowledgement.
    } else {
      toast.info(
        t('updates.settings.available', { version: after.latest_version }),
        {
          action: { label: t('updates.toast.action'), onClick: openDialog },
        },
      )
    }
  }

  const lastCheckedLabel = lastChecked
    ? t('updates.settings.last_checked', {
        when: new Date(lastChecked).toLocaleString(i18n.language),
      })
    : t('updates.settings.never_checked')

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('updates.settings.title')}</CardTitle>
      </CardHeader>
      <CardContent className="grid gap-4">
        <Field label={t('updates.settings.current')}>
          <div className="flex items-center gap-2">
            <span className="font-mono">v{currentVersion}</span>
            {hasNotifiable && pending && (
              <Button
                size="sm"
                variant="default"
                onClick={openDialog}
                className="h-7"
              >
                {t('updates.settings.available', {
                  version: pending.latest_version,
                })}
              </Button>
            )}
          </div>
        </Field>
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-muted-foreground">{lastCheckedLabel}</span>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              onClick={() => useAnnouncementStore.getState().openHistory()}
            >
              {t('announcements.dialog.settings_button')}
            </Button>
            <Button
              size="sm"
              variant="outline"
              onClick={handleCheck}
              disabled={checking || !HAS_TAURI}
            >
              {checking ? t('updates.settings.checking') : t('updates.settings.check_now')}
            </Button>
          </div>
        </div>
        <Toggle
          label={t('updates.settings.auto_check')}
          value={autoCheckEnabled}
          onChange={setAutoCheckEnabled}
        />
        <Field label={t('updates.settings.skipped_label')}>
          <div className="flex items-center justify-between">
            <span className="text-sm">
              {skippedVersion ?? t('updates.settings.none_skipped')}
            </span>
            {skippedVersion && (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  clearSkipped()
                  toast.success(
                    t('updates.settings.clear_skipped') + ': ' + skippedVersion,
                  )
                }}
              >
                {t('updates.settings.clear_skipped')}
              </Button>
            )}
          </div>
        </Field>
        {pending && (
          <div className="flex items-center justify-end">
            <Button
              size="sm"
              variant="ghost"
              onClick={() => openExternal(pending.html_url)}
            >
              {t('updates.dialog.open_release')}
            </Button>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
