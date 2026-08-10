import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  CheckCircle2,
  RefreshCw,
  Eye,
  EyeOff,
  Ticket,
  Activity,
  ShoppingCart,
  AlertTriangle,
  XCircle,
  Lock,
  Info,
  Network,
} from 'lucide-react'
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
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { invoke } from '@/lib/tauri'
import { toast } from '@/components/ui/sonner'
import { PurchaseDialog } from '@/components/PurchaseDialog'
import { useConfigStore } from '@/stores/configStore'
import { usePurchaseStore, type PurchasePhase } from '@/stores/purchaseStore'
import { effectiveProxy, isValidProxyUrl } from '@/lib/proxy'
import {
  apiProvider,
  selectedApiProfile,
  withSelectedApiProfile,
  type ApiProfile,
} from '@/lib/nativeApi'
import type { KeyStatus, ModelInfo, NativeApiConfig, RedeemResponse } from '@/types'

/** Shape of a key the server issues: 32 letters and digits, nothing else. */
const KEY_PATTERN = /^[A-Za-z0-9]{32}$/

/**
 * Controlled editor for the built-in bot's cloud-inference settings
 * (`bot.api`). Fully controlled via `value` / `onChange` so it can bind to the
 * config store (Bots page) or to a wizard draft (Setup) alike. Persistence of
 * the whole config is the caller's job (a Save button, or the wizard's Finish);
 * the only thing this component persists on its own is a freshly-minted key.
 *
 * Enabling the API queries the available models once and auto-fills any empty
 * 4p / 3p model slot with the first model the key may use for that mode.
 */
export function NativeApiFields({
  value,
  onChange,
  onKeyMinted,
}: {
  value: NativeApiConfig
  onChange: (api: NativeApiConfig) => void
  /**
   * Called with the full api config after a redeem mints a **new** key, so the
   * caller can persist it immediately (single-use code, key shown once). The
   * Bots page passes `persistApiConfig`; the Setup wizard omits this and lets
   * its Finish step persist — persisting mid-wizard would fight the wizard's
   * draft↔store re-sync.
   */
  onKeyMinted?: (api: NativeApiConfig) => void | Promise<void>
}) {
  const { t } = useTranslation()
  const [showKey, setShowKey] = useState(false)
  const [checking, setChecking] = useState(false)
  const [enabling, setEnabling] = useState(false)
  const [checkingHealth, setCheckingHealth] = useState(false)
  const [testingProxy, setTestingProxy] = useState(false)
  const [loadingModels, setLoadingModels] = useState(false)
  const [status, setStatus] = useState<KeyStatus | null>(null)
  const [models, setModels] = useState<ModelInfo[] | null>(null)
  const [redeemOpen, setRedeemOpen] = useState(false)
  const [buyOpen, setBuyOpen] = useState(false)
  const purchasePhase = usePurchaseStore((s) => s.phase)
  const [err, setErr] = useState<string | null>(null)

  const provider = apiProvider(value)
  const profile = selectedApiProfile(value)
  const set = (patch: Partial<NativeApiConfig>) => onChange({ ...value, ...patch })
  const setProfile = (patch: Partial<ApiProfile>) =>
    onChange(withSelectedApiProfile(value, patch))
  const hasUrlKey = profile.base_url.trim() !== '' && profile.key.trim() !== ''

  // The server URL is a developer-only field: pointing a novice at a rogue
  // server is the obvious scam vector, so the input stays locked unless
  // developer mode (Settings → General) is on. Read from the *saved* config,
  // not the edited draft — flipping the toggle must be deliberate and saved,
  // never part of the same unsaved edit that changes the URL.
  const devMode = useConfigStore((s) => s.config?.general.developer_mode ?? false)

  /** Key we have already adopted, so re-renders don't re-query the server. */
  const adoptedKeyRef = useRef<string | null>(null)

  // Latest `value` as of the last committed render. Async handlers (the model
  // auto-fill in `toggleEnabled` / `adoptKey`) resolve against this instead of
  // the snapshot captured when the request started, so a slow request never
  // clobbers what the user typed while it was in flight.
  const valueRef = useRef(value)
  useEffect(() => {
    valueRef.current = value
  }, [value])

  const queryModels = async (v: NativeApiConfig): Promise<ModelInfo[]> => {
    const selected = selectedApiProfile(v)
    const list = await invoke<ModelInfo[]>('native_api_models', {
      provider: apiProvider(v),
      baseUrl: selected.base_url,
      proxy: effectiveProxy(v),
      key: selected.key,
    })
    setModels(list)
    return list
  }

  /**
   * A complete key was typed or pasted. Take it from there: ask the server what
   * models it grants, pick a default for each mode, and switch cloud inference
   * on.
   *
   * Entering a key is the user saying they want the cloud model — making them
   * then find a toggle (which sits above the field they just filled) and a model
   * dropdown only invites the state where a key is set and nothing uses it.
   *
   * A key the server rejects enables nothing: the error surfaces and the bot
   * stays local, which beats a config that claims cloud inference it cannot do.
   */
  const adoptKey = async (key: string) => {
    const current = valueRef.current
    const currentProvider = apiProvider(current)
    const next = withSelectedApiProfile(current, { key })
    const nextProfile = selectedApiProfile(next)
    // The original service documents an exact key shape, so it can be adopted
    // while typing. FlyA only documents the `flyat_` prefix; avoid retrying the
    // network on every character and let Enable / Check / Fetch trigger it.
    if (
      currentProvider === 'flya' ||
      !KEY_PATTERN.test(key) ||
      !nextProfile.base_url.trim() ||
      adoptedKeyRef.current === key
    ) {
      onChange(next)
      return
    }
    adoptedKeyRef.current = key
    onChange(next)
    setEnabling(true)
    setErr(null)
    try {
      const list = await queryModels(next)
      const cur = valueRef.current
      const curProfile = selectedApiProfile(cur)
      const filled = withSelectedApiProfile(cur, {
        model_4p: curProfile.model_4p || list.find((m) => m.game === '4p')?.id || '',
        model_3p: curProfile.model_3p || list.find((m) => m.game === '3p')?.id || '',
      })
      filled.enabled = true
      onChange(filled)
    } catch (e) {
      // Let the next edit try again — a rejected key is often a half-pasted one.
      adoptedKeyRef.current = null
      setErr(String(e))
    } finally {
      setEnabling(false)
    }
  }

  // Turning the API on: flip immediately for a responsive switch, then query
  // the models once and fill any empty 4p/3p slot with the first eligible model
  // for that mode. Never clobbers a model the user already chose.
  const toggleEnabled = async (on: boolean) => {
    const next = { ...value, enabled: on }
    const nextProfile = selectedApiProfile(next)
    if (!on || !(nextProfile.base_url.trim() && nextProfile.key.trim())) {
      onChange(next)
      return
    }
    onChange(next)
    setEnabling(true)
    setErr(null)
    try {
      const list = await queryModels(next)
      const first4p = list.find((m) => m.game === '4p')?.id
      const first3p = list.find((m) => m.game === '3p')?.id
      // Merge the auto-filled models into the CURRENT value, not the pre-toggle
      // snapshot: the user may have typed into any field during the 1–2s fetch.
      // Fill only still-empty model slots so a model the user just chose wins.
      const cur = valueRef.current
      const curProfile = selectedApiProfile(cur)
      const filled4p = curProfile.model_4p || first4p || ''
      const filled3p = curProfile.model_3p || first3p || ''
      if (filled4p !== curProfile.model_4p || filled3p !== curProfile.model_3p) {
        onChange(withSelectedApiProfile(cur, { model_4p: filled4p, model_3p: filled3p }))
      }
    } catch (e) {
      setErr(String(e))
    } finally {
      setEnabling(false)
    }
  }

  const checkKey = async () => {
    if (!hasUrlKey) {
      setErr(t('bots.api.need_url_key'))
      return
    }
    setChecking(true)
    setErr(null)
    setStatus(null)
    try {
      setStatus(
        await invoke<KeyStatus>('native_api_key_status', {
          provider,
          baseUrl: profile.base_url,
          proxy: effectiveProxy(value),
          key: profile.key,
        }),
      )
    } catch (e) {
      setErr(String(e))
    } finally {
      setChecking(false)
    }
  }

  const fetchModels = async () => {
    if (!hasUrlKey) {
      setErr(t('bots.api.need_url_key'))
      return
    }
    setLoadingModels(true)
    setErr(null)
    try {
      await queryModels(value)
    } catch (e) {
      setErr(String(e))
    } finally {
      setLoadingModels(false)
    }
  }

  // Shared sink for a freshly-minted key (redeemed code or purchase): the
  // code/key is single-use and shown once, so reflect it in the edited value
  // immediately and let the caller persist it (Bots page) before it can be
  // lost. The Setup wizard omits `onKeyMinted` and persists on Finish.
  // Also flips `enabled` on: whoever just paid for (or redeemed) a key wants
  // it used — empty model slots are fine, the server picks its defaults.
  const adoptNewKey = async (key: string) => {
    const nextApi = withSelectedApiProfile({ ...value, enabled: true }, { key })
    onChange(nextApi)
    if (onKeyMinted) {
      try {
        await onKeyMinted(nextApi)
      } catch (e) {
        toast.error(t('bots.api.save_failed'), { description: String(e) })
      }
    }
  }

  const health = async () => {
    if (profile.base_url.trim() === '') {
      setErr(t('bots.api.need_url'))
      return
    }
    setCheckingHealth(true)
    setErr(null)
    try {
      const h = await invoke<{ status: string; models: string[] }>('native_api_health', {
        baseUrl: profile.base_url,
        proxy: effectiveProxy(value),
      })
      toast.success(t('bots.api.health_ok', { status: h.status }), {
        description: h.models.join(', '),
      })
    } catch (e) {
      setErr(String(e))
    } finally {
      setCheckingHealth(false)
    }
  }

  // Verify the proxy actually carries traffic: a health check forced through
  // the typed proxy. The button is only reachable with a valid, enabled proxy,
  // so reaching the server proves the tunnel works end to end.
  const testProxy = async () => {
    if (profile.base_url.trim() === '') {
      setErr(t('bots.api.need_url'))
      return
    }
    setTestingProxy(true)
    setErr(null)
    try {
      if (provider === 'flya') {
        if (!profile.key.trim()) {
          setErr(t('bots.api.need_url_key'))
          return
        }
        await invoke<ModelInfo[]>('native_api_models', {
          provider,
          baseUrl: profile.base_url,
          proxy: value.proxy.trim(),
          key: profile.key,
        })
        toast.success(t('bots.api.proxy_ok', { status: 'FlyA' }))
      } else {
        const h = await invoke<{ status: string; models: string[] }>('native_api_health', {
          baseUrl: profile.base_url,
          proxy: value.proxy.trim(),
        })
        toast.success(t('bots.api.proxy_ok', { status: h.status }))
      }
    } catch (e) {
      setErr(String(e))
    } finally {
      setTestingProxy(false)
    }
  }

  return (
    <div className="grid gap-4">
      <p className="text-sm text-muted-foreground">{t('bots.api.desc')}</p>
      <p className="text-xs text-muted-foreground -mt-2">{t('bots.api.apply_immediately')}</p>

      <div className="grid gap-1.5">
        <Label>{t('bots.api.provider')}</Label>
        <Select
          value={provider}
          onValueChange={(next: 'original' | 'flya') => {
            adoptedKeyRef.current = null
            setModels(null)
            setStatus(null)
            setErr(null)
            set({ provider: next })
          }}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="original">{t('bots.api.provider_original')}</SelectItem>
            <SelectItem value="flya">{t('bots.api.provider_flya')}</SelectItem>
          </SelectContent>
        </Select>
        <span className="text-xs text-muted-foreground">{t('bots.api.provider_hint')}</span>
      </div>

      <div className="flex items-center justify-between gap-4">
        <div className="flex flex-col">
          <Label>{t('bots.api.enable')}</Label>
          <span className="text-xs text-muted-foreground">{t('bots.api.enable_hint')}</span>
        </div>
        <div className="flex items-center gap-2">
          {enabling && <RefreshCw className="h-4 w-4 animate-spin text-muted-foreground" />}
          <Switch checked={value.enabled} onCheckedChange={(v) => void toggleEnabled(v)} />
        </div>
      </div>

      {/* The locked explanation is a hover tooltip on the wrapper (not text
          under the field) — the disabled input itself swallows pointer events,
          so the title has to live on an enabled ancestor. */}
      <div className="grid gap-1.5" title={devMode ? undefined : t('bots.api.base_url_locked')}>
        <Label className="flex items-center gap-1.5">
          {t('bots.api.base_url')}
          {!devMode && <Lock className="h-3 w-3 text-muted-foreground" />}
        </Label>
        <Input
          value={profile.base_url}
          onChange={(e) => setProfile({ base_url: e.target.value })}
          placeholder={provider === 'flya' ? 'https://api.nashout.com' : 'https://mjapi.shinkuan.me'}
          autoComplete="off"
          spellCheck={false}
          disabled={!devMode}
        />
      </div>

      <div className="grid gap-1.5">
        <div className="flex items-center justify-between gap-4">
          <Label className="flex items-center gap-1.5">
            {t('bots.api.proxy')}
            <TooltipProvider>
              <Tooltip delayDuration={100}>
                <TooltipTrigger asChild>
                  <Info className="h-3.5 w-3.5 cursor-help text-muted-foreground" />
                </TooltipTrigger>
                <TooltipContent side="right">{t('bots.api.proxy_hint')}</TooltipContent>
              </Tooltip>
            </TooltipProvider>
          </Label>
          <Switch
            checked={value.proxy_enabled}
            onCheckedChange={(on) => set({ proxy_enabled: on })}
          />
        </div>
        <div className="flex gap-2">
          <Input
            value={value.proxy}
            onChange={(e) => set({ proxy: e.target.value })}
            placeholder="socks5://127.0.0.1:1080"
            autoComplete="off"
            spellCheck={false}
            className="font-mono"
            disabled={!value.proxy_enabled}
          />
          <Button
            variant="outline"
            size="icon"
            className="shrink-0"
            onClick={testProxy}
            disabled={testingProxy || !value.proxy_enabled || !isValidProxyUrl(value.proxy)}
            title={t('bots.api.test_proxy')}
          >
            {testingProxy ? (
              <RefreshCw className="h-4 w-4 animate-spin" />
            ) : (
              <Network className="h-4 w-4" />
            )}
          </Button>
        </div>
        {value.proxy_enabled && !isValidProxyUrl(value.proxy) && (
          <span className="text-xs text-red-400">{t('bots.api.proxy_invalid')}</span>
        )}
      </div>

      <div className="grid gap-1.5">
        <Label>{t('bots.api.key')}</Label>
        <div className="flex gap-2">
          <Input
            type={showKey ? 'text' : 'password'}
            value={profile.key}
            onChange={(e) => void adoptKey(e.target.value.trim())}
            placeholder="••••••••••••••••••••••••••••••••"
            autoComplete="off"
            spellCheck={false}
            className="font-mono"
          />
          <Button
            variant="outline"
            size="icon"
            className="shrink-0"
            onClick={() => setShowKey((s) => !s)}
            title={showKey ? t('bots.api.hide_key') : t('bots.api.show_key')}
          >
            {showKey ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
          </Button>
        </div>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="grid gap-1.5">
          <Label>{t('bots.api.model_4p')}</Label>
          <Input
            value={profile.model_4p}
            onChange={(e) => setProfile({ model_4p: e.target.value })}
            placeholder="4p-model"
            autoComplete="off"
            spellCheck={false}
            className="font-mono"
          />
        </div>
        <div className="grid gap-1.5">
          <Label>{t('bots.api.model_3p')}</Label>
          <Input
            value={profile.model_3p}
            onChange={(e) => setProfile({ model_3p: e.target.value })}
            placeholder="3p-model"
            autoComplete="off"
            spellCheck={false}
            className="font-mono"
          />
        </div>
      </div>
      <span className="text-xs text-muted-foreground -mt-2">{t('bots.api.model_hint')}</span>

      {models && (
        <div className="grid gap-1.5">
          <Label className="text-xs text-muted-foreground">{t('bots.api.models_title')}</Label>
          <div className="flex flex-wrap gap-1.5">
            {models.length === 0 && (
              <span className="text-xs text-muted-foreground">{t('bots.api.models_empty')}</span>
            )}
            {models.map((m) => (
              <Button
                key={m.id}
                size="sm"
                variant="secondary"
                className="h-7 font-mono text-xs"
                title={m.desc}
                onClick={() =>
                  setProfile(m.game === '3p' ? { model_3p: m.id } : { model_4p: m.id })
                }
              >
                {m.id}
              </Button>
            ))}
          </div>
        </div>
      )}

      {status && (
        <div className="grid grid-cols-2 gap-x-6 gap-y-1 rounded-md border border-border p-3 text-sm sm:grid-cols-3">
          <Kv label={t('bots.api.plan')} value={status.plan || '—'} />
          <Kv label={t('bots.api.expires')} value={status.expires_at || '—'} />
          <Kv label={t('bots.api.usage')} value={`${status.usage_today} / ${status.rpd}`} />
          <Kv label={t('bots.api.rpm')} value={String(status.rpm)} />
          <Kv label={t('bots.api.topk')} value={String(status.topk)} />
        </div>
      )}

      {err && <span className="text-sm text-red-400 [overflow-wrap:anywhere]">{err}</span>}

      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={checkKey} disabled={checking} className="gap-1.5">
          <CheckCircle2 className="h-4 w-4" />
          {checking ? t('bots.api.checking') : t('bots.api.check_key')}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={fetchModels}
          disabled={loadingModels}
          className="gap-1.5"
        >
          <RefreshCw className={`h-4 w-4 ${loadingModels ? 'animate-spin' : ''}`} />
          {t('bots.api.fetch_models')}
        </Button>
        {provider === 'original' && (
          <Button
            variant="outline"
            size="sm"
            onClick={health}
            disabled={checkingHealth}
            className="gap-1.5"
          >
            <Activity className={`h-4 w-4 ${checkingHealth ? 'animate-spin' : ''}`} />
            {t('bots.api.health')}
          </Button>
        )}
        {provider === 'original' && (
          <>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setRedeemOpen(true)}
              className="gap-1.5"
            >
              <Ticket className="h-4 w-4" />
              {t('bots.api.redeem')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setBuyOpen(true)}
              className="gap-1.5"
            >
              <ShoppingCart className="h-4 w-4" />
              {t('bots.api.buy')}
            </Button>
          </>
        )}
      </div>

      {provider === 'original' && purchasePhase !== 'idle' && !buyOpen && (
        <PurchaseChip phase={purchasePhase} onOpen={() => setBuyOpen(true)} />
      )}

      {provider === 'original' && redeemOpen && (
        <RedeemDialog
          baseUrl={profile.base_url}
          proxy={effectiveProxy(value)}
          currentKey={profile.key}
          onClose={() => setRedeemOpen(false)}
          onNewKey={(key) => void adoptNewKey(key)}
        />
      )}

      {provider === 'original' && buyOpen && (
        <PurchaseDialog
          baseUrl={profile.base_url}
          proxy={effectiveProxy(value)}
          currentKey={profile.key}
          onClose={() => setBuyOpen(false)}
          onNewKey={(key) => void adoptNewKey(key)}
        />
      )}
    </div>
  )
}

/** Compact status row for a purchase running with its dialog closed, so the
 *  buyer can find their way back to it (or see that it completed). */
function PurchaseChip({ phase, onOpen }: { phase: PurchasePhase; onOpen: () => void }) {
  const { t } = useTranslation()
  const [label, icon] =
    phase === 'redeem_failed'
      ? [t('bots.api.buy_chip_action'), <AlertTriangle key="i" className="h-4 w-4 text-amber-400" />]
      : phase === 'done'
        ? [t('bots.api.buy_chip_done'), <CheckCircle2 key="i" className="h-4 w-4 text-emerald-400" />]
        : phase === 'delivered'
          ? [t('bots.api.buy_chip_delivered'), <CheckCircle2 key="i" className="h-4 w-4 text-emerald-400" />]
          : phase === 'failed'
            ? [t('bots.api.buy_chip_failed'), <XCircle key="i" className="h-4 w-4 text-red-400" />]
            : [
                t('bots.api.buy_chip_pending'),
                <RefreshCw key="i" className="h-4 w-4 animate-spin text-muted-foreground" />,
              ]
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex w-fit items-center gap-2 rounded-md border border-border px-3 py-1.5 text-sm transition-colors hover:bg-accent"
    >
      {icon}
      {label}
    </button>
  )
}

function Kv({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="font-mono [overflow-wrap:anywhere]">{value}</span>
    </div>
  )
}

function RedeemDialog({
  baseUrl,
  proxy,
  currentKey,
  onClose,
  onNewKey,
}: {
  baseUrl: string
  proxy: string
  currentKey: string
  onClose: () => void
  onNewKey: (key: string) => void
}) {
  const { t } = useTranslation()
  const [code, setCode] = useState('')
  const [email, setEmail] = useState('')
  const [renew, setRenew] = useState(false)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const [done, setDone] = useState<string | null>(null)

  const submit = async () => {
    if (baseUrl.trim() === '') {
      setErr(t('bots.api.need_url'))
      return
    }
    if (renew && currentKey.trim() === '') {
      setErr(t('bots.api.redeem_renew_hint'))
      return
    }
    setBusy(true)
    setErr(null)
    setDone(null)
    try {
      const resp = await invoke<RedeemResponse>('native_api_redeem', {
        baseUrl,
        proxy,
        code: code.trim(),
        email: email.trim() || undefined,
        renewKey: renew ? currentKey : undefined,
      })
      if (resp.key) {
        onNewKey(resp.key)
        setDone(t('bots.api.redeem_new_key', { last4: resp.key_last4, plan: resp.plan }))
        toast.success(t('bots.api.redeem_ok'))
      } else {
        setDone(t('bots.api.redeemed_extended', { last4: resp.key_last4, expires: resp.expires_at }))
        toast.success(t('bots.api.redeem_ok'))
      }
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('bots.api.redeem_title')}</DialogTitle>
        </DialogHeader>
        <div className="grid min-w-0 gap-4 py-2">
          <div className="grid gap-1.5">
            <Label>{t('bots.api.redeem_code')}</Label>
            <Input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="XXXXXXXXXXXXXXXX"
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
            />
          </div>
          <div className="flex items-center justify-between gap-4">
            <div className="flex flex-col">
              <Label>{t('bots.api.redeem_renew')}</Label>
              <span className="text-xs text-muted-foreground">{t('bots.api.redeem_renew_hint')}</span>
            </div>
            <Switch checked={renew} onCheckedChange={setRenew} disabled={currentKey.trim() === ''} />
          </div>
          {!renew && (
            <div className="grid gap-1.5">
              <Label>{t('bots.api.redeem_email')}</Label>
              <Input
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="you@example.com"
                autoComplete="off"
                spellCheck={false}
              />
              <span className="text-xs text-muted-foreground">{t('bots.api.redeem_email_hint')}</span>
            </div>
          )}
          {done && <span className="text-sm text-emerald-400 [overflow-wrap:anywhere]">{done}</span>}
          {err && <span className="text-sm text-red-400 [overflow-wrap:anywhere]">{err}</span>}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.close')}
          </Button>
          <Button onClick={submit} disabled={busy || code.trim() === ''}>
            {busy ? t('bots.api.redeeming') : t('bots.api.redeem_submit')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
