import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Circle, FolderOpen, Package, Pause, Play, RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { invoke } from '@/lib/tauri'
import { useLogsStore } from '@/stores/logsStore'
import { useInspectorStore } from '@/stores/inspectorStore'
import type {
  InspectorEntry,
  LogSessionInfo,
  ReadInspectorResponse,
  ReadLogResponse,
} from '@/types'
import { PackLogsDialog } from '@/components/PackLogsDialog'
import { DiagnosticView } from './DiagnosticView'
import { InspectorView } from './InspectorView'

type TabValue = 'diagnostic' | 'inspector'

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/**
 * Logs route shell.
 *
 * Owns the controls common to both tabs — session picker, live/pause,
 * auto-scroll, refresh, open folder — plus session-load orchestration.
 * Each tab renders its own filter bar + list + detail panel and
 * subscribes to its own live-tail hook gated on the active tab.
 *
 * Why one shell with tabs vs. two separate routes: the user is debugging
 * one session at a time. Switching between "what does my app think it's
 * doing" (diagnostic) and "what's actually flowing through the pipeline"
 * (inspector) without losing the session selection or live-tail state is
 * the entire point.
 */
export function Logs() {
  const { t } = useTranslation()

  const sessions = useLogsStore((s) => s.sessions)
  const setSessions = useLogsStore((s) => s.setSessions)
  const currentSession = useLogsStore((s) => s.currentSession)
  const setCurrentSession = useLogsStore((s) => s.setCurrentSession)
  const activeSession = useLogsStore((s) => s.activeSession)
  const setActiveSession = useLogsStore((s) => s.setActiveSession)
  const isLive = useLogsStore((s) => s.isLive)
  const setIsLive = useLogsStore((s) => s.setIsLive)
  const autoScroll = useLogsStore((s) => s.autoScroll)
  const setAutoScroll = useLogsStore((s) => s.setAutoScroll)
  const setLogEntries = useLogsStore((s) => s.setEntries)
  const clearLogEntries = useLogsStore((s) => s.clearEntries)
  const setInspectorEntries = useInspectorStore((s) => s.setEntries)
  const clearInspectorEntries = useInspectorStore((s) => s.clearEntries)

  const [tab, setTab] = useState<TabValue>('diagnostic')
  const [busy, setBusy] = useState(false)
  const [packOpen, setPackOpen] = useState(false)

  const loadSession = async (name: string) => {
    setBusy(true)
    try {
      clearLogEntries()
      clearInspectorEntries()
      // Both tabs need their data, regardless of which is active right
      // now — the user can flip between them at any time and we don't
      // want to round-trip a fresh fetch on every flip.
      const [logResp, inspResp] = await Promise.all([
        invoke<ReadLogResponse>('read_log_session', {
          req: { session: name, offset: 0, limit: 2000 },
        }),
        invoke<ReadInspectorResponse>('read_inspector', {
          req: { session: name, offset: 0, limit: 2000 },
        }),
      ])
      setLogEntries(logResp.entries)
      setInspectorEntries(inspResp.entries)
    } catch (err) {
      console.warn('session load failed:', err)
    } finally {
      setBusy(false)
    }
  }

  useEffect(() => {
    let cancelled = false
    void (async () => {
      setBusy(true)
      try {
        const list = await invoke<LogSessionInfo[]>('list_log_sessions')
        if (cancelled) return
        setSessions(list)
        const active = list.find((s) => s.is_active) ?? null
        if (active) {
          setActiveSession(active.name)
          setCurrentSession(active.name)
          await loadSession(active.name)
        }
      } catch (err) {
        console.warn('list_log_sessions failed:', err)
      } finally {
        if (!cancelled) setBusy(false)
      }
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const onSelectSession = async (name: string) => {
    if (name === currentSession) return
    setCurrentSession(name)
    await loadSession(name)
  }

  const onRefresh = async () => {
    if (!currentSession) return
    await loadSession(currentSession)
    try {
      const list = await invoke<LogSessionInfo[]>('list_log_sessions')
      setSessions(list)
    } catch {
      /* noop */
    }
  }

  const onOpenFolder = async () => {
    try {
      await invoke('open_log_folder', { session: currentSession ?? null })
    } catch (err) {
      console.warn('open_log_folder failed:', err)
    }
  }

  const onOpenPack = async () => {
    try {
      setSessions(await invoke<LogSessionInfo[]>('list_log_sessions'))
    } catch {
      /* keep the list we have */
    }
    setPackOpen(true)
  }

  const isViewingActive = currentSession != null && currentSession === activeSession
  const liveOnDiagnostic = tab === 'diagnostic' && isViewingActive && isLive
  const liveOnInspector = tab === 'inspector' && isViewingActive && isLive

  const handleScrolledAway = () => setAutoScroll(false)

  return (
    <div className="p-6 flex flex-col gap-4 w-full h-full min-h-0">
      <header className="flex items-center justify-between flex-wrap gap-3">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-semibold">{t('logs.title')}</h1>
          {isViewingActive && (
            <Badge
              variant="outline"
              className={
                isLive
                  ? 'gap-1.5 border-emerald-500/40 text-emerald-600 dark:text-emerald-300'
                  : 'gap-1.5 border-muted-foreground/40 text-muted-foreground'
              }
            >
              <Circle
                className={`h-2 w-2 ${isLive ? 'fill-emerald-500 text-emerald-500 animate-pulse' : 'fill-muted-foreground text-muted-foreground'}`}
              />
              {isLive ? t('logs.live') : t('logs.paused')}
            </Badge>
          )}
        </div>
        <div className="flex gap-2 items-center flex-wrap">
          <label className="flex items-center gap-1.5 text-sm text-muted-foreground select-none">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.target.checked)}
              className="h-4 w-4 cursor-pointer"
            />
            {t('logs.auto_scroll')}
          </label>
          {isViewingActive && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setIsLive(!isLive)}
              className="gap-1.5"
            >
              {isLive ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
              {isLive ? t('logs.pause') : t('logs.resume')}
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={onRefresh}
            disabled={busy}
            className="gap-1.5"
          >
            <RefreshCw className={`h-4 w-4 ${busy ? 'animate-spin' : ''}`} />
            {t('common.refresh')}
          </Button>
          <Button variant="outline" size="sm" onClick={onOpenPack} className="gap-1.5">
            <Package className="h-4 w-4" />
            {t('logs.pack.button')}
          </Button>
          <Button size="sm" onClick={onOpenFolder} className="gap-1.5">
            <FolderOpen className="h-4 w-4" />
            {t('logs.open_folder')}
          </Button>
        </div>
      </header>

      <Card>
        <CardContent className="flex items-center gap-3 flex-wrap py-3">
          <span className="text-sm text-muted-foreground whitespace-nowrap">
            {t('logs.session')}
          </span>
          <Select value={currentSession ?? ''} onValueChange={onSelectSession}>
            <SelectTrigger className="w-[280px]">
              <SelectValue placeholder={t('logs.session_placeholder')} />
            </SelectTrigger>
            <SelectContent>
              {sessions.map((s) => (
                <SelectItem key={s.name} value={s.name}>
                  <span className="font-mono">{s.name}</span>
                  {s.is_active && (
                    <span className="ml-2 text-xs text-emerald-600 dark:text-emerald-300">
                      ({t('logs.active')})
                    </span>
                  )}
                  <span className="ml-2 text-xs text-muted-foreground">
                    {formatBytes(s.size_bytes)}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </CardContent>
      </Card>

      <Tabs
        value={tab}
        onValueChange={(v) => setTab(v as TabValue)}
        className="flex-1 min-h-0"
      >
        <TabsList>
          <TabsTrigger value="diagnostic">{t('logs.tab_diagnostic')}</TabsTrigger>
          <TabsTrigger value="inspector">{t('logs.tab_inspector')}</TabsTrigger>
        </TabsList>
        <TabsContent value="diagnostic" className="flex-1 min-h-0 flex flex-col mt-0">
          <DiagnosticView
            liveEnabled={liveOnDiagnostic}
            autoScroll={autoScroll}
            onUserScrolledAway={handleScrolledAway}
          />
        </TabsContent>
        <TabsContent value="inspector" className="flex-1 min-h-0 flex flex-col mt-0">
          <InspectorView
            liveEnabled={liveOnInspector}
            autoScroll={autoScroll}
            onUserScrolledAway={handleScrolledAway}
          />
        </TabsContent>
      </Tabs>

      {packOpen && (
        <PackLogsDialog
          open={packOpen}
          onOpenChange={setPackOpen}
          sessions={sessions}
          current={currentSession}
        />
      )}
    </div>
  )
}

// Re-export so any existing imports of `InspectorEntry` from this module
// keep working without churning unrelated callers.
export type { InspectorEntry }
