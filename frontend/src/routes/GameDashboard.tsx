import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Responsive,
  useContainerWidth,
  verticalCompactor,
  type Layout,
  type LayoutItem,
  type ResponsiveLayouts,
} from 'react-grid-layout'
import 'react-grid-layout/css/styles.css'
import 'react-resizable/css/styles.css'

import { HelpCircle } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { toast } from '@/components/ui/sonner'
import { useLayoutStore, visibleTilesFor } from '@/stores/layoutStore'
import { useNumPlayers } from '@/stores/gameStore'
import { useConfigStore } from '@/stores/configStore'
import { useUiPrefsStore } from '@/stores/uiPrefsStore'
import {
  BREAKPOINTS,
  COLS,
  type Breakpoint,
  type TileId,
} from '@/tiles/defaults'
import { renderTile } from '@/tiles/registry'
import { AddTileMenu } from '@/components/AddTileMenu'
import { AutoStartControlBar } from '@/components/AutoStartControlBar'
import { DashboardOnboardingDialog } from '@/components/DashboardOnboardingDialog'
import { OverlayToggle } from '@/components/OverlayToggle'

const BOT_DISABLED_TOAST_ID = 'bot-disabled-warning'

export function GameDashboard() {
  const { t } = useTranslation()
  const layouts = useLayoutStore((s) => s.layouts)
  const hidden = useLayoutStore((s) => s.hidden)
  const mode = useLayoutStore((s) => s.mode)
  const setLayouts = useLayoutStore((s) => s.setLayouts)
  const setMode = useLayoutStore((s) => s.setMode)
  const reset = useLayoutStore((s) => s.reset)
  const numPlayers = useNumPlayers()
  const botEnabled = useConfigStore((s) => s.config?.bot.enabled)
  const markOnboarded = useUiPrefsStore((s) => s.markDashboardOnboarded)
  const { width, containerRef, mounted } = useContainerWidth()
  // Auto-open once on the very first visit. Derived from the persisted flag at
  // mount (read synchronously from localStorage at store init), so no effect is
  // needed and it never reappears after being dismissed.
  const [helpOpen, setHelpOpen] = useState(
    () => !useUiPrefsStore.getState().dashboardOnboarded,
  )

  // Sync layout mode with active game's player count.
  useEffect(() => {
    setMode(numPlayers === 3 ? '3p' : '4p')
  }, [numPlayers, setMode])

  // Warn when viewing Game page while bots are disabled.
  // botEnabled is undefined while config is still loading — wait for a real value.
  useEffect(() => {
    if (botEnabled === false) {
      toast.warning(t('game.bot_disabled_title'), {
        id: BOT_DISABLED_TOAST_ID,
        description: t('game.bot_disabled_desc'),
        duration: Infinity,
      })
    } else if (botEnabled === true) {
      toast.dismiss(BOT_DISABLED_TOAST_ID)
    }
    return () => {
      toast.dismiss(BOT_DISABLED_TOAST_ID)
    }
  }, [botEnabled, t])

  const handleHelpOpenChange = (open: boolean) => {
    setHelpOpen(open)
    if (!open) markOnboarded()
  }

  const [bp, setBp] = useState<Breakpoint>('lg')
  const visibleIds = visibleTilesFor(bp, hidden, mode)

  // RGL filters layouts to only visible items so missing entries don't crash.
  const filteredLayouts: ResponsiveLayouts = {
    lg: layouts.lg.filter((l) => visibleIds.includes(l.i as TileId)),
    md: layouts.md.filter((l) => visibleIds.includes(l.i as TileId)),
    sm: layouts.sm.filter((l) => visibleIds.includes(l.i as TileId)),
    xs: layouts.xs.filter((l) => visibleIds.includes(l.i as TileId)),
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 px-4 py-2 border-b border-border bg-muted/20">
        <h1 className="text-sm font-semibold tracking-wide uppercase text-muted-foreground">{t('nav.game')}</h1>
        <div className="ml-auto flex items-center gap-2">
          <OverlayToggle />
          <AddTileMenu bp={bp} />
          <Button variant="ghost" size="sm" onClick={reset} className="text-xs">
            {t('common.reset_layout')}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => setHelpOpen(true)}
            aria-label={t('game.onboarding.help_aria')}
            title={t('game.onboarding.help_aria')}
          >
            <HelpCircle className="size-4" />
          </Button>
        </div>
      </div>

      <AutoStartControlBar />

      <DashboardOnboardingDialog open={helpOpen} onOpenChange={handleHelpOpenChange} />

      <div ref={containerRef} className="flex-1 overflow-auto">
        {mounted && (
          <Responsive
            width={width}
            breakpoints={BREAKPOINTS}
            cols={COLS}
            rowHeight={30}
            margin={[12, 12]}
            containerPadding={[16, 16]}
            layouts={filteredLayouts}
            dragConfig={{ handle: '.tile-drag-handle' }}
            resizeConfig={{ handles: ['se'] }}
            compactor={verticalCompactor}
            onBreakpointChange={(b: string) => setBp(b as Breakpoint)}
            onLayoutChange={(_current: Layout, all: ResponsiveLayouts) => {
              const merged = mergeLayouts(layouts, all)
              setLayouts(merged)
            }}
          >
            {visibleIds.map((id) => (
              <div key={id} className="overflow-hidden">
                {renderTile(id, bp)}
              </div>
            ))}
          </Responsive>
        )}
      </div>
    </div>
  )
}

// RGL only emits layouts for currently rendered (visible) tiles. Merge with
// the existing store so hidden entries keep their last known position.
function mergeLayouts(prev: Record<Breakpoint, LayoutItem[]>, next: ResponsiveLayouts): Record<Breakpoint, LayoutItem[]> {
  const out: Record<Breakpoint, LayoutItem[]> = { ...prev }
  for (const bp of ['lg', 'md', 'sm', 'xs'] as const) {
    const incoming = next[bp]
    if (!incoming) continue
    const incomingIds = new Set(incoming.map((l) => l.i))
    const kept = (prev[bp] ?? []).filter((l) => !incomingIds.has(l.i))
    out[bp] = [...incoming, ...kept]
  }
  return out
}
