import { useTranslation } from 'react-i18next'
import { TileFrame } from '@/components/TileFrame'
import { Button } from '@/components/ui/button'
import { useNotifyStore } from '@/stores/notifyStore'
import { notifyTitle, notifyBody } from '@/lib/notifyText'
import { fmtTime } from '@/lib/format'
import type { Breakpoint } from '@/tiles/defaults'

const LEVEL_COLOR: Record<string, string> = {
  info:    'text-sky-400',
  success: 'text-emerald-400',
  warn:    'text-amber-400',
  error:   'text-red-400',
}

export function NotificationsTile({ bp }: { bp: Breakpoint }) {
  const { t } = useTranslation()
  const notifications = useNotifyStore((s) => s.notifications)
  const clear = useNotifyStore((s) => s.clearToasts)
  const recent = notifications.slice().reverse()

  return (
    <TileFrame
      id="notifications"
      title={t('tile.notifications')}
      bp={bp}
      rightSlot={
        <Button
          variant="ghost"
          size="sm"
          className="h-6 text-[10px] uppercase"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={clear}
        >
          {t('common.clear')}
        </Button>
      }
      contentClassName="p-0"
    >
      <ul className="flex flex-col text-xs divide-y divide-border">
        {recent.length === 0 ? (
          <li className="px-3 py-2 text-muted-foreground">{t('tile.notifications_empty')}</li>
        ) : recent.map((n) => {
          const body = notifyBody(n)
          return (
          <li key={n._seq} className="px-3 py-1.5">
            <div className="flex items-center gap-2">
              <span className={`text-[10px] uppercase font-semibold ${LEVEL_COLOR[n.level] ?? ''}`}>{n.level}</span>
              <span className="text-muted-foreground text-[10px] font-mono">{fmtTime(new Date(n._ts))}</span>
              <span className="font-medium">{notifyTitle(n)}</span>
            </div>
            {body && <div className="text-muted-foreground text-[11px] mt-0.5">{body}</div>}
          </li>
          )
        })}
      </ul>
    </TileFrame>
  )
}
