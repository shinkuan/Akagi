import { useEffect, useState } from 'react'
import { invoke } from '@/lib/tauri'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Button } from '@/components/ui/button'

interface NextGameStatus {
  enabled: boolean
  limited: boolean
  limit: number
  remaining: number
  started: number
  reason: string
  connection?: string
  ready?: boolean
}

const PREFS_KEY = 'akagi.next-game.preferences'
function preferences() {
  try {
    const p = JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}')
    return { limited: p.limited !== false, limit: Number.isInteger(p.limit) && p.limit >= 1 && p.limit <= 99 ? String(p.limit) : '3' }
  } catch {
    return { limited: true, limit: '3' }
  }
}

export function NextGameCard({ canStart }: { canStart: boolean }) {
  const [prefs, setPrefs] = useState(preferences)
  const [status, setStatus] = useState<NextGameStatus | null>(null)
  const [error, setError] = useState('')
  const [connectionError, setConnectionError] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    let pending = false
    const refresh = async () => {
      if (pending) return
      pending = true
      try {
        const s = await invoke<NextGameStatus>('get_next_game_status')
        if (!cancelled) { setStatus(s); setConnectionError('') }
      } catch (e) {
        if (!cancelled) setConnectionError(String(e))
      } finally { pending = false }
    }
    void refresh()
    const timer = setInterval(() => void refresh(), 1500)
    return () => { cancelled = true; clearInterval(timer) }
  }, [])

  const enabled = status?.enabled ?? false
  const limited = enabled ? status!.limited : prefs.limited
  const limit = enabled ? String(status!.limit) : prefs.limit
  const validLimit = Number.isInteger(Number(limit)) && Number(limit) >= 1 && Number(limit) <= 99
  const changePrefs = (p: typeof prefs) => {
    setPrefs(p)
    if (Number.isInteger(Number(p.limit)) && Number(p.limit) >= 1 && Number(p.limit) <= 99) {
      localStorage.setItem(PREFS_KEY, JSON.stringify({ limited: p.limited, limit: Number(p.limit) }))
    }
  }
  const control = async (on: boolean) => {
    setBusy(true); setError('')
    try {
      const result = await invoke<NextGameStatus>('control_next_game', {
        control: { enabled: on, limited: prefs.limited, limit: validLimit ? Number(limit) : 3 },
      })
      setStatus(result)
    } catch (e) { setError(String(e)) } finally { setBusy(false) }
  }

  return <Card id="auto-next-game">
    <CardHeader><CardTitle>自动续局</CardTitle></CardHeader>
    <CardContent className="grid gap-4">
      <div className="flex items-center justify-between gap-4">
        <div><Label htmlFor="next-game-enabled">自动开始下一场</Label>
          <p className="mt-1 text-xs text-muted-foreground">雀魂整场结束后，自动点击“再来一场”并确认匹配。即时生效。</p></div>
        <Switch id="next-game-enabled" checked={enabled}
          disabled={busy || (!enabled && (!canStart || !status || !!connectionError || (limited && !validLimit)))}
          onCheckedChange={(value) => void control(value)} />
      </div>
      {!canStart && <p className="text-xs text-amber-600">请先保存设置：游戏平台选雀魂、抓包选 Chromium，并启用自动打牌。</p>}
      <div className="flex items-center justify-between gap-4">
        <Label htmlFor="next-game-limited">限制续局次数</Label>
        <Switch id="next-game-limited" checked={limited} disabled={busy || enabled}
          onCheckedChange={(value) => changePrefs({ ...prefs, limited: value })} />
      </div>
      <div className="flex items-center justify-between gap-4">
        <div><Label htmlFor="next-game-limit">额外场数</Label><p className="mt-1 text-xs text-muted-foreground">填 3 表示当前这场结束后，再开 3 场；关闭次数限制则持续续局。</p></div>
        <Input id="next-game-limit" className="w-24" type="number" min={1} max={99} step={1}
          value={limit} disabled={busy || enabled || !limited}
          onChange={(e) => changePrefs({ ...prefs, limit: e.target.value })} />
      </div>
      {limited && !validLimit && <p className="text-sm text-red-500">额外场数必须是 1–99 的整数。</p>}
      <div className="flex gap-8 text-sm">
        <span>已自动开始：<b>{status?.started ?? 0}</b> 场</span>
        <span>剩余：<b>{status?.enabled ? (status.limited ? status.remaining : '不限') : status?.started ? status.remaining : limited ? limit : '不限'}</b> 场</span>
      </div>
      <div className="rounded-md bg-muted px-3 py-2 text-sm" role="status">{connectionError || status?.reason || '正在连接续局组件…'}</div>
      {error && <p className="text-sm text-red-500" role="alert">{error}</p>}
      <div className="flex items-center gap-4">
        <Button variant="outline" disabled={busy || !enabled} onClick={() => void control(false)}>停止后续续局</Button>
        <p className="text-xs text-muted-foreground">停止不会退出当前牌局或取消已提交的匹配。关闭 Akagi 后续局也会停止。</p>
      </div>
    </CardContent>
  </Card>
}
