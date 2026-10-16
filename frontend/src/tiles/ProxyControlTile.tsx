import { useTranslation } from 'react-i18next'
import { TileFrame } from '@/components/TileFrame'
import { Button } from '@/components/ui/button'
import { useCaptureStore } from '@/stores/captureStore'
import { invoke } from '@/lib/tauri'
import { captureTitleKey } from '@/lib/captureConfig'
import { Play, Square, RotateCw } from 'lucide-react'
import type { Breakpoint } from '@/tiles/defaults'
import { useState } from 'react'

const STATE_COLOR: Record<string, string> = {
  running:  'bg-emerald-500',
  starting: 'bg-amber-500',
  stopped:  'bg-zinc-500',
  error:    'bg-red-500',
}

export function ProxyControlTile({ bp }: { bp: Breakpoint }) {
  const { t } = useTranslation()
  const status = useCaptureStore((s) => s.status)
  const [busy, setBusy] = useState(false)

  const dot = STATE_COLOR[status.state] ?? 'bg-zinc-500'
  const descriptor = 'descriptor' in status && status.descriptor ? status.descriptor : '—'
  const kind = 'kind' in status ? status.kind : null
  const title = t(captureTitleKey(kind))

  const call = async (cmd: 'start_capture' | 'stop_capture' | 'restart_capture') => {
    setBusy(true)
    try {
      await invoke(cmd)
    } catch {
      /* surfaced via notify */
    } finally {
      setBusy(false)
    }
  }

  return (
    <TileFrame id="proxy-control" title={title} bp={bp} contentClassName="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <span className={`h-2 w-2 rounded-full ${dot}`} />
        <span className="text-sm font-medium capitalize">{status.state}</span>
        <span className="text-xs font-mono text-muted-foreground ml-auto truncate max-w-[60%]" title={descriptor}>
          {descriptor}
        </span>
      </div>

      <div className="flex gap-1.5">
        <Button
          variant="default"
          size="sm"
          className="flex-1 gap-1.5"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => call('start_capture')}
          disabled={busy || status.state === 'running' || status.state === 'starting'}
        >
          <Play className="h-3.5 w-3.5" />
          {t('common.start')}
        </Button>
        <Button
          variant="outline"
          size="sm"
          className="flex-1 gap-1.5"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => call('restart_capture')}
          disabled={busy}
        >
          <RotateCw className="h-3.5 w-3.5" />
          {t('common.restart')}
        </Button>
        <Button
          variant="destructive"
          size="sm"
          className="flex-1 gap-1.5"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => call('stop_capture')}
          disabled={busy || status.state === 'stopped'}
        >
          <Square className="h-3.5 w-3.5" />
          {t('common.stop')}
        </Button>
      </div>

      {status.state === 'error' && 'error' in status && (
        <span className="text-[11px] text-red-400 font-mono">{status.error}</span>
      )}
    </TileFrame>
  )
}
