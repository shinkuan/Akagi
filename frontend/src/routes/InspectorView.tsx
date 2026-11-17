import { useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowDown, ArrowUp, Bot, Globe, Hash, Network, X, Zap } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useInspectorStore } from '@/stores/inspectorStore'
import { useInspectorStream } from '@/hooks/useInspectorStream'
import type { InspectorEntry, InspectorKind } from '@/types'

const KIND_BADGE: Record<InspectorKind, string> = {
  ws_frame: 'bg-cyan-500/15 text-cyan-700 border-cyan-500/30 dark:text-cyan-300',
  mjai_event: 'bg-emerald-500/15 text-emerald-700 border-emerald-500/30 dark:text-emerald-300',
  bot_reaction: 'bg-fuchsia-500/15 text-fuchsia-700 border-fuchsia-500/30 dark:text-fuchsia-300',
  http: 'bg-amber-500/15 text-amber-700 border-amber-500/30 dark:text-amber-300',
}

function formatTime(ms: number): string {
  const d = new Date(ms)
  const pad = (n: number, w = 2) => n.toString().padStart(w, '0')
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/// Stable per-row key for React. Timestamp + kind + a short content
/// digest avoids React-key collisions when two events land at the same
/// millisecond (common for mjai bursts at game start).
function entryKey(e: InspectorEntry, idx: number): string {
  if (e.kind === 'ws_frame') {
    return `${e.ts_ms}:${idx}:wf:${e.flow_id}:${e.size}`
  }
  if (e.kind === 'mjai_event') {
    return `${e.ts_ms}:${idx}:me:${e.event.type}`
  }
  if (e.kind === 'http') {
    return `${e.ts_ms}:${idx}:ht:${e.phase}:${e.exchange_id ?? ''}:${e.url}`
  }
  return `${e.ts_ms}:${idx}:br:${e.bot}:${e.actor_id}`
}

function entryHasActor(e: InspectorEntry, actor: number): boolean {
  if (e.kind === 'ws_frame') return false
  // Describes the client, not any seat — never matches a seat filter.
  if (e.kind === 'http') return false
  if (e.kind === 'bot_reaction') return e.actor_id === actor
  // mjai_event: many variants carry an `actor` field. Cast loosely —
  // variants without `actor` simply don't match.
  const ev = e.event as { actor?: number }
  return ev.actor === actor
}

/// Hex+ASCII dump of base64 bytes for the WS frame detail panel. Capped
/// at 512 bytes — anything bigger is unreadable in a side panel anyway.
function hexDump(b64: string, t: (k: string, opts?: Record<string, unknown>) => string): string {
  let bytes: Uint8Array
  try {
    const bin = atob(b64)
    bytes = new Uint8Array(bin.length)
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  } catch {
    return t('inspector.invalid_base64')
  }
  const truncated = bytes.length > 512
  const view = bytes.slice(0, 512)
  const lines: string[] = []
  for (let off = 0; off < view.length; off += 16) {
    const chunk = view.slice(off, off + 16)
    const hex = Array.from(chunk)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join(' ')
      .padEnd(48, ' ')
    const ascii = Array.from(chunk)
      .map((b) => (b >= 0x20 && b <= 0x7e ? String.fromCharCode(b) : '.'))
      .join('')
    lines.push(`${off.toString(16).padStart(6, '0')}  ${hex}  ${ascii}`)
  }
  if (truncated) lines.push(t('inspector.bytes_truncated', { count: bytes.length - 512 }))
  return lines.join('\n')
}

type InspectorViewProps = {
  /** Toggle from the parent — only true when this tab is visible AND
   *  the user is on the active session AND tail isn't paused. */
  liveEnabled: boolean
  autoScroll: boolean
  onUserScrolledAway: () => void
}

export function InspectorView({ liveEnabled, autoScroll, onUserScrolledAway }: InspectorViewProps) {
  const { t } = useTranslation()
  const entries = useInspectorStore((s) => s.entries)
  const filter = useInspectorStore((s) => s.filter)
  const setFilter = useInspectorStore((s) => s.setFilter)
  const toggleKind = useInspectorStore((s) => s.toggleKind)
  const selectedKey = useInspectorStore((s) => s.selectedKey)
  const setSelectedKey = useInspectorStore((s) => s.setSelectedKey)

  const listRef = useRef<HTMLDivElement>(null)

  useInspectorStream(liveEnabled)

  const filtered = useMemo(() => {
    const search = filter.search.trim().toLowerCase()
    return entries.filter((e) => {
      if (!filter.kinds.has(e.kind)) return false
      if (filter.actor != null && !entryHasActor(e, filter.actor)) return false
      if (search.length > 0) {
        // Build a cheap haystack per row. Pre-stringifying every entry
        // each render would be expensive at 5000 rows; substring search
        // on JSON.stringify is fast enough since the strings are small.
        const hay =
          e.kind === 'ws_frame'
            ? `${e.raw.format === 'withheld' ? '' : e.raw.data} ${e.parsed?.method ?? ''}`
            : e.kind === 'mjai_event'
              ? JSON.stringify(e.event)
              : e.kind === 'http'
                ? `${e.method} ${e.url} ${e.annotations?.map((a) => a.summary).join(' ') ?? ''}`
                : `${e.bot} ${JSON.stringify(e.action)} ${JSON.stringify(e.trigger)}`
        if (!hay.toLowerCase().includes(search)) return false
      }
      return true
    })
  }, [entries, filter])

  // Auto-scroll on new entries.
  useEffect(() => {
    if (!autoScroll) return
    const el = listRef.current
    if (!el) return
    el.scrollTop = el.scrollHeight
  }, [filtered.length, autoScroll])

  // Resolve the selected key against the filtered list. Plain `find`
  // pattern — over 5000 rows worst-case it's still microseconds, and
  // it satisfies React Compiler's preserved-memo rule (early returns
  // inside a useMemo trip "could not preserve memoization").
  const selectedEntry: { entry: InspectorEntry; key: string } | null = (() => {
    if (selectedKey == null) return null
    const idx = filtered.findIndex((e, i) => entryKey(e, i) === selectedKey)
    return idx === -1 ? null : { entry: filtered[idx], key: selectedKey }
  })()

  return (
    <div className="flex flex-col gap-3 flex-1 min-h-0">
      <Card>
        <CardContent className="flex items-center gap-3 flex-wrap py-3">
          <div className="flex items-center gap-1.5">
            <span className="text-sm text-muted-foreground whitespace-nowrap">
              {t('inspector.kind')}
            </span>
            <KindToggle kind="ws_frame" label={t('inspector.kind_ws_frame')} icon={<Network className="h-3 w-3" />} active={filter.kinds.has('ws_frame')} onToggle={toggleKind} />
            <KindToggle kind="mjai_event" label={t('inspector.kind_mjai_event')} icon={<Zap className="h-3 w-3" />} active={filter.kinds.has('mjai_event')} onToggle={toggleKind} />
            <KindToggle kind="bot_reaction" label={t('inspector.kind_bot_reaction')} icon={<Bot className="h-3 w-3" />} active={filter.kinds.has('bot_reaction')} onToggle={toggleKind} />
            <KindToggle kind="http" label={t('inspector.kind_http')} icon={<Globe className="h-3 w-3" />} active={filter.kinds.has('http')} onToggle={toggleKind} />
          </div>

          <div className="flex items-center gap-1.5">
            <span className="text-sm text-muted-foreground whitespace-nowrap">
              {t('inspector.actor')}
            </span>
            {[null, 0, 1, 2, 3].map((a) => {
              const active = filter.actor === a
              const label = a == null ? t('inspector.actor_all') : `${a}`
              return (
                <button
                  key={String(a)}
                  type="button"
                  onClick={() => setFilter({ actor: a })}
                  className={`px-2 py-0.5 text-xs font-mono rounded border transition-colors cursor-pointer ${
                    active
                      ? 'border-primary text-primary bg-primary/10'
                      : 'border-muted text-muted-foreground hover:bg-muted/40'
                  }`}
                >
                  {label}
                </button>
              )
            })}
          </div>

          <div className="flex items-center gap-1.5 flex-1 min-w-[220px]">
            <Hash className="h-4 w-4 text-muted-foreground shrink-0" />
            <Input
              type="search"
              value={filter.search}
              onChange={(e) => setFilter({ search: e.target.value })}
              placeholder={t('inspector.search_placeholder')}
              className="h-8"
            />
          </div>

          <div className="text-xs text-muted-foreground whitespace-nowrap">
            {t('inspector.entry_count', { shown: filtered.length, total: entries.length })}
          </div>
        </CardContent>
      </Card>

      <div className="flex flex-1 min-h-0 gap-3">
        <Card className="flex-1 min-h-0 flex flex-col overflow-hidden">
          <div
            ref={listRef}
            className="flex-1 overflow-y-auto font-mono text-xs"
            onWheel={() => {
              const el = listRef.current
              if (!el || !autoScroll) return
              const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 8
              if (!atBottom) onUserScrolledAway()
            }}
          >
            {filtered.length === 0 ? (
              <div className="p-8 text-center text-muted-foreground text-sm">
                {entries.length === 0 ? t('inspector.empty') : t('inspector.empty_filtered')}
              </div>
            ) : (
              filtered.map((e, i) => {
                const k = entryKey(e, i)
                const isSelected = k === selectedKey
                return (
                  <div
                    key={k}
                    onClick={() => setSelectedKey(isSelected ? null : k)}
                    className={`flex items-baseline gap-2 px-3 py-1 border-b border-border/40 cursor-pointer hover:bg-muted/40 ${
                      isSelected ? 'bg-muted/60' : ''
                    }`}
                  >
                    <span className="text-muted-foreground shrink-0 tabular-nums">
                      {formatTime(e.ts_ms)}
                    </span>
                    <KindRowBadge kind={e.kind} />
                    <RowSummary entry={e} />
                  </div>
                )
              })
            )}
          </div>
        </Card>

        {selectedEntry && (
          <Card className="w-[480px] shrink-0 flex flex-col overflow-hidden">
            <div className="flex items-center justify-between px-4 py-2 border-b">
              <span className="text-sm font-semibold">{t('inspector.detail_title')}</span>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setSelectedKey(null)}
                className="h-6 w-6 p-0"
              >
                <X className="h-4 w-4" />
              </Button>
            </div>
            <CardContent className="flex-1 overflow-y-auto py-3 space-y-3 text-xs">
              <DetailPanel entry={selectedEntry.entry} />
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  )
}

function KindToggle({
  kind,
  label,
  icon,
  active,
  onToggle,
}: {
  kind: InspectorKind
  label: string
  icon: React.ReactNode
  active: boolean
  onToggle: (kind: InspectorKind) => void
}) {
  return (
    <button
      type="button"
      onClick={() => onToggle(kind)}
      className={`px-2 py-0.5 text-xs font-mono rounded border transition-colors cursor-pointer flex items-center gap-1 ${
        active ? KIND_BADGE[kind] : 'border-muted text-muted-foreground hover:bg-muted/40'
      }`}
    >
      {icon}
      {label}
    </button>
  )
}

function KindRowBadge({ kind }: { kind: InspectorKind }) {
  const cls = KIND_BADGE[kind]
  const label =
    kind === 'ws_frame'
      ? 'WS'
      : kind === 'mjai_event'
        ? 'MJAI'
        : kind === 'http'
          ? 'HTTP'
          : 'BOT'
  return (
    <span
      className={`shrink-0 px-1.5 py-0.5 rounded border text-[10px] leading-none ${cls}`}
    >
      {label}
    </span>
  )
}

function RowSummary({ entry }: { entry: InspectorEntry }) {
  const { t } = useTranslation()
  if (entry.kind === 'ws_frame') {
    const arrow =
      entry.direction === 'down' ? <ArrowDown className="h-3 w-3 inline" /> : <ArrowUp className="h-3 w-3 inline" />
    const summary =
      entry.parsed != null
        ? entry.parsed.method
        : entry.raw.format === 'text'
          ? entry.raw.data.slice(0, 80)
          : entry.raw.format === 'withheld'
            ? t('inspector.withheld')
            : t('inspector.binary')
    return (
      <span className="flex-1 break-all whitespace-pre-wrap">
        <span className="text-muted-foreground">
          {arrow} {formatBytes(entry.size)}
          {entry.emitted > 0 && (
            <span className="ml-1">
              · <Zap className="h-3 w-3 inline" />
              {entry.emitted}
            </span>
          )}
        </span>
        <span className="ml-2">{summary}</span>
      </span>
    )
  }
  if (entry.kind === 'mjai_event') {
    const ev = entry.event as { type: string; actor?: number; pai?: string }
    const tail = ev.actor != null ? ` actor=${ev.actor}${ev.pai ? ` pai=${ev.pai}` : ''}` : ''
    return (
      <span className="flex-1 break-all whitespace-pre-wrap">
        <Badge variant="outline" className="font-mono text-[10px] py-0 mr-2">
          {ev.type}
        </Badge>
        <span className="text-muted-foreground">{tail}</span>
      </span>
    )
  }
  if (entry.kind === 'http') {
    // The path is what distinguishes one row from the next; the host
    // repeats. Show the path prominently and keep the host muted.
    const path = entry.url.replace(/^https?:\/\/[^/]+/, '') || '/'
    return (
      <span className="flex-1 break-all whitespace-pre-wrap">
        <span className="text-muted-foreground">
          {entry.phase === 'response'
            ? `← ${entry.status ?? '?'}`
            : `→ ${entry.method}`}
        </span>
        {entry.annotations?.map((a) => (
          <Badge key={a.kind} variant="outline" className="font-mono text-[10px] py-0 mx-1">
            {a.summary}
          </Badge>
        ))}
        <span className="ml-2">{path.slice(0, 100)}</span>
        <span className="text-muted-foreground ml-2">{entry.host}</span>
      </span>
    )
  }
  // bot_reaction
  const action = entry.action as { type: string; pai?: string }
  return (
    <span className="flex-1 break-all whitespace-pre-wrap">
      <span className="text-muted-foreground">{entry.bot} · actor={entry.actor_id}</span>
      <Badge variant="outline" className="font-mono text-[10px] py-0 mx-2">
        {action.type}
      </Badge>
      {action.pai && <span className="text-muted-foreground">pai={action.pai}</span>}
      <span className="text-muted-foreground ml-2">· {entry.reaction_ms}ms</span>
    </span>
  )
}

function DetailPanel({ entry }: { entry: InspectorEntry }) {
  const { t } = useTranslation()
  if (entry.kind === 'ws_frame') {
    return (
      <>
        <DetailRow label={t('inspector.detail_time')} value={new Date(entry.ts_ms).toISOString()} />
        <DetailRow label={t('inspector.detail_direction')} value={entry.direction} />
        <DetailRow label={t('inspector.detail_flow')} value={entry.flow_id} mono />
        <DetailRow label={t('inspector.detail_size')} value={formatBytes(entry.size)} />
        <DetailRow label={t('inspector.detail_emitted')} value={`${entry.emitted}`} />
        {entry.parsed && (
          <>
            <div>
              <div className="text-muted-foreground mb-1">{t('inspector.detail_method')}</div>
              <div className="font-mono break-all">{entry.parsed.method}</div>
            </div>
            <div>
              <div className="text-muted-foreground mb-1">{t('inspector.detail_args')}</div>
              <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2">
                {JSON.stringify(entry.parsed.args, null, 2)}
              </pre>
            </div>
          </>
        )}
        {entry.raw.format === 'withheld' ? (
          <div className="text-muted-foreground italic">{t('inspector.withheld')}</div>
        ) : (
          <div>
            <div className="text-muted-foreground mb-1">
              {entry.raw.format === 'text' ? t('inspector.detail_raw_text') : t('inspector.detail_raw_hex')}
            </div>
            <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2 text-[11px]">
              {entry.raw.format === 'text' ? entry.raw.data : hexDump(entry.raw.data, t)}
            </pre>
          </div>
        )}
      </>
    )
  }
  if (entry.kind === 'mjai_event') {
    return (
      <>
        <DetailRow label={t('inspector.detail_time')} value={new Date(entry.ts_ms).toISOString()} />
        <div>
          <div className="text-muted-foreground mb-1">{t('inspector.detail_event')}</div>
          <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2">
            {JSON.stringify(entry.event, null, 2)}
          </pre>
        </div>
      </>
    )
  }
  if (entry.kind === 'http') {
    return (
      <>
        <DetailRow label={t('inspector.detail_time')} value={new Date(entry.ts_ms).toISOString()} />
        <DetailRow label={t('inspector.detail_source')} value={entry.source} mono />
        <DetailRow label={t('inspector.detail_phase')} value={entry.phase} mono />
        <DetailRow
          label={t('inspector.detail_method')}
          value={entry.status != null ? `${entry.method || '—'} → ${entry.status}` : entry.method}
          mono
        />
        <DetailRow label={t('inspector.detail_host')} value={entry.host || '—'} mono />
        {/* Absent on an unpaired response — see the HTTP/2 note in the
            backend. Showing the gap beats implying a pairing we don't have. */}
        <DetailRow
          label={t('inspector.detail_exchange')}
          value={entry.exchange_id ?? t('inspector.unpaired')}
          mono
        />
        {entry.annotations?.map((a) => (
          <div key={a.kind}>
            <div className="text-muted-foreground mb-1">
              {t('inspector.detail_decoded')} · {a.kind}
            </div>
            <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2">
              {JSON.stringify(a.data, null, 2)}
            </pre>
          </div>
        ))}
        <div>
          <div className="text-muted-foreground mb-1">{t('inspector.detail_headers')}</div>
          <div className="rounded bg-muted/40 p-2 space-y-1">
            {entry.headers.length === 0 ? (
              <div className="text-muted-foreground">—</div>
            ) : (
              entry.headers.map((h, i) => (
                <div key={`${h.name}:${i}`} className="grid grid-cols-[9rem_1fr] gap-2">
                  <span className="font-mono text-muted-foreground break-all">{h.name}</span>
                  <span className="font-mono break-all">{h.value}</span>
                </div>
              ))
            )}
          </div>
        </div>
        {entry.body && (
          <div>
            <div className="text-muted-foreground mb-1">
              {t('inspector.detail_body')}
              {entry.body.bytes != null && ` · ${formatBytes(entry.body.bytes)}`}
            </div>
            <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2 text-[11px]">
              {entry.body.text ??
                t('inspector.body_skipped', { reason: entry.body.skipped ?? '' })}
            </pre>
          </div>
        )}
        <div>
          <div className="text-muted-foreground mb-1">{t('inspector.detail_url')}</div>
          <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2 text-[11px]">
            {entry.url}
          </pre>
        </div>
      </>
    )
  }
  // bot_reaction
  return (
    <>
      <DetailRow label={t('inspector.detail_time')} value={new Date(entry.ts_ms).toISOString()} />
      <DetailRow label={t('inspector.detail_bot')} value={entry.bot} />
      <DetailRow label={t('inspector.detail_actor')} value={`${entry.actor_id}`} />
      <DetailRow label={t('inspector.detail_reaction')} value={`${entry.reaction_ms} ms`} />
      <div>
        <div className="text-muted-foreground mb-1">{t('inspector.detail_trigger')}</div>
        <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2">
          {JSON.stringify(entry.trigger, null, 2)}
        </pre>
      </div>
      <div>
        <div className="text-muted-foreground mb-1">{t('inspector.detail_action')}</div>
        <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2">
          {JSON.stringify(entry.action, null, 2)}
        </pre>
      </div>
      {entry.meta && (
        <div>
          <div className="text-muted-foreground mb-1">{t('inspector.detail_meta')}</div>
          <pre className="font-mono whitespace-pre-wrap break-all bg-muted/40 rounded p-2">
            {JSON.stringify(entry.meta, null, 2)}
          </pre>
        </div>
      )}
    </>
  )
}

function DetailRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <div className="text-muted-foreground mb-0.5">{label}</div>
      <div className={mono ? 'font-mono break-all' : 'break-all'}>{value}</div>
    </div>
  )
}
