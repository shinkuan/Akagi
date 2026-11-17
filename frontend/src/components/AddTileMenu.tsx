import { useTranslation } from 'react-i18next'
import { Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuLabel,
  DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu'
import { useLayoutStore } from '@/stores/layoutStore'
import { type Breakpoint, type TileId } from '@/tiles/defaults'

// Stable empty fallback — see OpponentsTile note on Zustand selector identity.
const EMPTY_HIDDEN: readonly TileId[] = []

// Maps each TileId to its localized i18n key. Kept here (not in defaults.ts)
// because defaults.ts is consumed in non-React contexts (layout calc) where
// the i18n hook isn't available.
const TILE_TITLE_KEYS: Record<TileId, string> = {
  'header':          'tile.header_full_title',
  'player-0':        'tile.player_0',
  'player-1':        'tile.player_1',
  'player-2':        'tile.player_2',
  'player-3':        'tile.player_3',
  'self-hand':       'tile.self_hand',
  'board':           'tile.board',
  'recommendations': 'tile.recommendations',
  'expected-draws':  'tile.expected_draws',
  'risk-chart':      'tile.risk_chart',
  'opponents':       'tile.opponents',
  'events':          'tile.events',
  'notifications':   'tile.notifications',
  'bot-responses':   'tile.bot_responses',
  'bot-action':      'tile.bot_action',
  'bot-show':        'tile.bot_show',
  'proxy-control':   'tile.proxy_control',
}

export function AddTileMenu({ bp }: { bp: Breakpoint }) {
  const { t } = useTranslation()
  const hidden = useLayoutStore((s) => s.hidden[bp] ?? EMPTY_HIDDEN)
  const mode = useLayoutStore((s) => s.mode)
  const show = useLayoutStore((s) => s.show)

  // 3p: never offer player-3 in the Add menu.
  const offered = mode === '3p' ? hidden.filter((id) => id !== 'player-3') : hidden

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="gap-1.5 text-xs">
          <Plus className="h-3.5 w-3.5" />
          {t('common.add_tile')}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuLabel className="text-xs">{t('common.hidden_tiles')}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {offered.length === 0 ? (
          <DropdownMenuItem disabled className="text-xs text-muted-foreground">
            {t('common.all_tiles_visible')}
          </DropdownMenuItem>
        ) : offered.map((id) => (
          <DropdownMenuItem key={id} onClick={() => show(id, bp)} className="text-xs">
            {t(TILE_TITLE_KEYS[id])}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
