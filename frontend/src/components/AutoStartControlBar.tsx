import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { toast } from '@/components/ui/sonner'
import { invoke } from '@/lib/tauri'
import { useConfigStore } from '@/stores/configStore'
import type {
  AppConfig,
  AutoStartStatus,
  PlayerCount,
  RoomTier,
  RoundLength,
} from '@/types'

// Manual autostart control on the GameDashboard: pick the room options /
// game count and Start / Stop. The whole bar is hidden while autoplay is off
// (autostart only queues games — autoplay plays them), and Start is greyed out
// mid-game (sessions may only start at the lobby); Stop works at any time.
//
// Platform seam: the bar shell (label, target count, Start/Stop, status
// readout) is platform-agnostic and reads autostart.ui.*; the room-selection
// controls are the Majsoul option group below, reading the
// autostart.majsoul.* vocabulary. Supporting another platform (e.g. Tenhou)
// means adding its own option group + autostart.<platform>.* keys and
// rendering it here based on the configured platform.

// Room tiers, rendered from the autostart.majsoul.tier.* keys (lowercased value).
const TIERS: RoomTier[] = ['Bronze', 'Silver', 'Gold', 'Jade', 'Throne']

// Majsoul Ranked room selection: tier / players / round length.
function MajsoulRankedOptions(props: {
  tier: RoomTier
  pc: PlayerCount
  rl: RoundLength
  onTier: (v: RoomTier) => void
  onPc: (v: PlayerCount) => void
  onRl: (v: RoundLength) => void
  disabled: boolean
}) {
  const { t } = useTranslation()
  const { tier, pc, rl, onTier, onPc, onRl, disabled } = props
  return (
    <>
      <Select value={tier} onValueChange={(v) => onTier(v as RoomTier)} disabled={disabled}>
        <SelectTrigger className="h-8 w-20" aria-label={t('autostart.ui.aria_tier')}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {TIERS.map((v) => (
            <SelectItem key={v} value={v}>
              {t(`autostart.majsoul.tier.${v.toLowerCase()}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Select value={pc} onValueChange={(v) => onPc(v as PlayerCount)} disabled={disabled}>
        <SelectTrigger className="h-8 w-20" aria-label={t('autostart.ui.aria_players')}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="Four">{t('autostart.majsoul.players.four')}</SelectItem>
          <SelectItem value="Three">{t('autostart.majsoul.players.three')}</SelectItem>
        </SelectContent>
      </Select>
      <Select value={rl} onValueChange={(v) => onRl(v as RoundLength)} disabled={disabled}>
        <SelectTrigger className="h-8 w-16" aria-label={t('autostart.ui.aria_length')}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="East">{t('autostart.majsoul.length.east')}</SelectItem>
          <SelectItem value="South">{t('autostart.majsoul.length.south')}</SelectItem>
        </SelectContent>
      </Select>
    </>
  )
}

export function AutoStartControlBar() {
  // `i18n` is only used for the backend error-key lookup below; everything else
  // is localized through `t`.
  const { t, i18n } = useTranslation()
  const cfg = useConfigStore((s) => s.config)
  const setConfig = useConfigStore((s) => s.setConfig)
  // Local overrides fall back to the saved config, so no seed effect is needed.
  const [tierO, setTierO] = useState<RoomTier | null>(null)
  const [pcO, setPcO] = useState<PlayerCount | null>(null)
  const [rlO, setRlO] = useState<RoundLength | null>(null)
  const [targetO, setTargetO] = useState<string | null>(null)
  const [status, setStatus] = useState<AutoStartStatus | null>(null)
  const [busy, setBusy] = useState(false)

  const tier = tierO ?? cfg?.autostart?.tier ?? 'Gold'
  const pc = pcO ?? cfg?.autostart?.player_count ?? 'Four'
  const rl = rlO ?? cfg?.autostart?.round_length ?? 'South'
  const target = targetO ?? String(cfg?.autostart?.target_game_count ?? 0)

  // Poll the session status for the running/count readout.
  useEffect(() => {
    let alive = true
    const tick = () =>
      invoke<AutoStartStatus>('autostart_status')
        .then((s) => {
          if (alive) setStatus(s)
        })
        .catch(() => {})
    tick()
    const id = setInterval(tick, 1500)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [])

  // One-shot status fetch so the Start/Stop button flips immediately instead of
  // waiting for the next 1.5s poll.
  const refreshStatus = async () => {
    try {
      setStatus(await invoke<AutoStartStatus>('autostart_status'))
    } catch {
      /* next poll will catch up */
    }
  }

  const start = async () => {
    setBusy(true)
    try {
      // The backend takes a u32 — clamp so a typed negative/huge value can't
      // fail deserialization before the handler even runs.
      const games = Math.min(Math.max(Math.trunc(Number(target) || 0), 0), 9999)
      try {
        await invoke('autostart_start', {
          tier,
          playerCount: pc,
          roundLength: rl,
          target: games,
        })
      } catch (e) {
        // The backend returns an autostart.error.* i18n key; localize it when
        // we know it, else show the raw string (older backend / unexpected err).
        const msg = i18n.exists(String(e)) ? t(String(e)) : String(e)
        toast.error(t('autostart.ui.start_failed'), { description: msg })
        return
      }
      // The session is running from here on — failures below are bookkeeping
      // only and must not be reported as a failed start.
      // autostart_start persists tier/mode/target server-side; refresh the
      // shared config store so a later Settings save can't write a stale
      // autostart section back over the live session.
      try {
        setConfig(await invoke<AppConfig>('get_config'))
        // Drop local overrides so the display snaps back to the persisted
        // (server-clamped) values instead of showing a stale typed value.
        setTierO(null)
        setPcO(null)
        setRlO(null)
        setTargetO(null)
      } catch {
        /* store refresh is best-effort; the next get_config call will align it */
      }
      await refreshStatus()
    } finally {
      setBusy(false)
    }
  }

  const stop = async () => {
    setBusy(true)
    try {
      await invoke('autostart_stop')
      await refreshStatus()
    } catch (e) {
      toast.error(t('autostart.ui.stop_failed'), { description: String(e) })
    } finally {
      setBusy(false)
    }
  }

  const active = status?.active ?? false
  const inGame = status?.in_game ?? false

  // Autostart rides on autoplay; without it the queued games would sit
  // unplayed, so don't offer the controls at all. Exception: while a session
  // is still active (autoplay switched off mid-run — the backend guard stops
  // it within a tick), keep the bar so Stop stays reachable.
  if (!cfg?.autoplay?.enabled && !active) return null

  return (
    <div className="flex items-center gap-2 flex-wrap px-4 py-2 border-b border-border bg-muted/10 text-sm">
      <span className="text-muted-foreground whitespace-nowrap">
        {t('autostart.ui.label')} · {t('autostart.majsoul.label')}
      </span>
      {/* Config edits mid-session don't affect the running session, so lock the
          inputs while active to avoid implying otherwise. */}
      <MajsoulRankedOptions
        tier={tier}
        pc={pc}
        rl={rl}
        onTier={setTierO}
        onPc={setPcO}
        onRl={setRlO}
        disabled={active}
      />
      <Input
        type="number"
        min={0}
        value={target}
        onChange={(e) => setTargetO(e.target.value)}
        className="h-8 w-20"
        placeholder={t('autostart.ui.games_placeholder')}
        aria-label={t('autostart.ui.aria_games')}
        disabled={active}
      />
      {active ? (
        <Button size="sm" variant="destructive" onClick={stop} disabled={busy}>
          {t('autostart.ui.stop')}
        </Button>
      ) : (
        // Start is only allowed at the lobby — greyed out while a game runs.
        <Button size="sm" onClick={start} disabled={busy || inGame}>
          {t('autostart.ui.start')}
        </Button>
      )}
      <span className="ml-1 text-xs text-muted-foreground whitespace-nowrap">
        {active
          ? status && status.target > 0
            ? t('autostart.ui.running', {
                done: status.games_done ?? 0,
                target: status.target,
              })
            : t('autostart.ui.running_unlimited', { done: status?.games_done ?? 0 })
          : inGame
            ? t('autostart.ui.in_game_hint')
            : t('autostart.ui.stopped')}
      </span>
    </div>
  )
}
