import type { LayoutItem } from 'react-grid-layout'

export type TileId =
  | 'header'
  | 'player-0'
  | 'player-1'
  | 'player-2'
  | 'player-3'
  | 'self-hand'
  | 'recommendations'
  | 'expected-draws'
  | 'risk-chart'
  | 'opponents'
  | 'events'
  | 'notifications'
  | 'bot-responses'
  | 'bot-action'
  | 'bot-show'
  | 'proxy-control'

export type Breakpoint = 'lg' | 'md' | 'sm' | 'xs'

export const BREAKPOINTS: Record<Breakpoint, number> = { lg: 1200, md: 996, sm: 768, xs: 0 }
export const COLS: Record<Breakpoint, number> = { lg: 12, md: 10, sm: 6, xs: 4 }

// All tile ids in canonical order.
export const ALL_TILES: TileId[] = [
  'header',
  'player-0',
  'player-1',
  'player-2',
  'player-3',
  'self-hand',
  'recommendations',
  'expected-draws',
  'risk-chart',
  'opponents',
  'events',
  'notifications',
  'bot-responses',
  'bot-action',
  'bot-show',
  'proxy-control',
]

export const DEFAULT_HIDDEN: TileId[] = ['recommendations']

// Default layout for the lg (12-col) breakpoint. RGL Responsive copies
// layouts from lg → md → sm → xs when missing entries; for now we provide
// only lg + xs and let the responsive layout handle md / sm dynamically.
const LG_LAYOUT: LayoutItem[] = [
  { i: 'header',          x: 0, y: 0,  w: 6,  h: 3, minW: 6, minH: 2, maxH: 5 },
  { i: 'self-hand',       x: 6, y: 0,  w: 6,  h: 3, minW: 6, minH: 2, maxH: 6 },
  { i: 'bot-action',      x: 0, y: 3,  w: 4,  h: 4, minW: 3, minH: 2 },
  { i: 'notifications',   x: 0, y: 7,  w: 4,  h: 4, minW: 2, minH: 3 },
  { i: 'bot-show',        x: 4, y: 3,  w: 5,  h: 8, minW: 2, minH: 4 },
//{ i: 'recommendations', x: 4, y: 3,  w: 5,  h: 8, minW: 2, minH: 4 },
  { i: 'bot-responses',   x: 9, y: 3,  w: 3,  h: 4, minW: 2, minH: 3 },
  { i: 'events',          x: 9, y: 7,  w: 3,  h: 4, minW: 2, minH: 3 },
  { i: 'player-0',        x: 0, y: 11, w: 3,  h: 8, minW: 2, minH: 6 },
  { i: 'player-1',        x: 3, y: 11, w: 3,  h: 8, minW: 2, minH: 6 },
  { i: 'player-2',        x: 6, y: 11, w: 3,  h: 8, minW: 2, minH: 6 },
  { i: 'player-3',        x: 9, y: 11, w: 3,  h: 8, minW: 2, minH: 6 },
  { i: 'risk-chart',      x: 0, y: 19, w: 3,  h: 6, minW: 2, minH: 4 },
  { i: 'opponents',       x: 3, y: 19, w: 3,  h: 6, minW: 2, minH: 4 },
  { i: 'proxy-control',   x: 6, y: 19, w: 6,  h: 6, minW: 2, minH: 2, maxH: 6 },
  { i: 'expected-draws',  x: 0, y: 25, w: 6,  h: 8, minW: 4, minH: 5 },
]

const MD_LAYOUT: LayoutItem[] = [
  { i: 'header',          x: 0, y: 0,  w: 5,  h: 3, minW: 5, minH: 2, maxH: 5 },
  { i: 'self-hand',       x: 5, y: 0,  w: 5,  h: 3, minW: 5, minH: 2, maxH: 6 },
  { i: 'bot-action',      x: 0, y: 3,  w: 5,  h: 4, minW: 3, minH: 2 },
  { i: 'bot-show',        x: 5, y: 3,  w: 5,  h: 8, minW: 2, minH: 4 },
//{ i: 'recommendations', x: 5, y: 3,  w: 5,  h: 8, minW: 2, minH: 4 },
  { i: 'opponents',       x: 0, y: 7,  w: 5,  h: 5, minW: 2, minH: 4 },
  { i: 'events',          x: 0, y: 12, w: 5,  h: 6, minW: 2, minH: 3 },
  { i: 'bot-responses',   x: 5, y: 11, w: 5,  h: 4, minW: 2, minH: 3 },
  { i: 'notifications',   x: 5, y: 15, w: 5,  h: 3, minW: 2, minH: 3 },
  { i: 'risk-chart',      x: 0, y: 18, w: 2,  h: 8, minW: 2, minH: 4 },
  { i: 'player-0',        x: 2, y: 18, w: 2,  h: 8, minW: 2, minH: 6 },
  { i: 'player-1',        x: 4, y: 18, w: 2,  h: 8, minW: 2, minH: 6 },
  { i: 'player-2',        x: 6, y: 18, w: 2,  h: 8, minW: 2, minH: 6 },
  { i: 'player-3',        x: 8, y: 18, w: 2,  h: 8, minW: 2, minH: 6 },
  { i: 'proxy-control',   x: 0, y: 26, w: 10, h: 4, minW: 2, minH: 2, maxH: 6 },
  { i: 'expected-draws',  x: 0, y: 30, w: 10, h: 8, minW: 4, minH: 5 },
]

const SM_LAYOUT: LayoutItem[] = [
  { i: 'header',          x: 0, y: 0,  w: 6, h: 3, minW: 4, minH: 2, maxH: 5 },
  { i: 'player-0',        x: 0, y: 2,  w: 3, h: 8, minW: 2, minH: 6 },
  { i: 'player-1',        x: 3, y: 2,  w: 3, h: 8, minW: 2, minH: 6 },
  { i: 'player-3',        x: 0, y: 10, w: 3, h: 8, minW: 2, minH: 6 },
//{ i: 'recommendations', x: 3, y: 10, w: 3, h: 8, minW: 2, minH: 4 },
  { i: 'player-2',        x: 0, y: 18, w: 6, h: 6, minW: 2, minH: 6 },
  { i: 'risk-chart',      x: 0, y: 24, w: 3, h: 6, minW: 2, minH: 4 },
  { i: 'opponents',       x: 3, y: 24, w: 3, h: 6, minW: 2, minH: 4 },
  { i: 'self-hand',       x: 0, y: 30, w: 6, h: 4, minW: 4, minH: 2, maxH: 6 },
  { i: 'events',          x: 0, y: 33, w: 3, h: 5, minW: 2, minH: 3 },
  { i: 'notifications',   x: 3, y: 33, w: 3, h: 5, minW: 2, minH: 3 },
  { i: 'proxy-control',   x: 0, y: 38, w: 3, h: 5, minW: 2, minH: 2, maxH: 6 },
  { i: 'bot-responses',   x: 3, y: 38, w: 3, h: 5, minW: 2, minH: 3 },
  { i: 'bot-action',      x: 0, y: 43, w: 6, h: 3, minW: 3, minH: 2 },
  { i: 'bot-show',        x: 0, y: 46, w: 6, h: 5, minW: 2, minH: 3 },
  { i: 'expected-draws',  x: 0, y: 51, w: 6, h: 8, minW: 4, minH: 5 },
]

const XS_LAYOUT: LayoutItem[] = ALL_TILES.map((id, i) => ({
  i: id,
  x: 0,
  y: i * 6,
  w: 4,
  h: id === 'header' || id === 'self-hand' ? 4 : 6,
  minW: 4,
  minH: 2,
}))

export const DEFAULT_LAYOUTS: Record<Breakpoint, LayoutItem[]> = {
  lg: LG_LAYOUT,
  md: MD_LAYOUT,
  sm: SM_LAYOUT,
  xs: XS_LAYOUT,
}

// === 3p (sanma) layouts ===
//
// Sanma has only 3 player seats — `player-3` is suppressed and the freed
// grid space goes to the remaining player tiles. `DEFAULT_HIDDEN_3P`
// includes `player-3` so the registry skips rendering it; the layout
// entry itself is omitted (smaller saved layout footprint).

const LG_LAYOUT_3P: LayoutItem[] = [
  { i: 'header',          x: 0, y: 0,  w: 6,  h: 4, minW: 6, minH: 2, maxH: 5 },
  { i: 'self-hand',       x: 6, y: 0,  w: 6,  h: 4, minW: 6, minH: 2, maxH: 6 },
  { i: 'bot-action',      x: 0, y: 4,  w: 4,  h: 5, minW: 3, minH: 2 },
  { i: 'bot-show',        x: 4, y: 4,  w: 4,  h: 8, minW: 2, minH: 3 },
  { i: 'bot-responses',   x: 8, y: 4,  w: 4,  h: 8, minW: 2, minH: 3 },
  { i: 'notifications',   x: 0, y: 9,  w: 4,  h: 3, minW: 2, minH: 3 },
  { i: 'player-0',        x: 0, y: 12, w: 4,  h: 8, minW: 2, minH: 6 },
  { i: 'player-1',        x: 4, y: 12, w: 4,  h: 8, minW: 2, minH: 6 },
  { i: 'player-2',        x: 8, y: 12, w: 4,  h: 8, minW: 2, minH: 6 },
  { i: 'proxy-control',   x: 0, y: 20, w: 4,  h: 5, minW: 2, minH: 2, maxH: 6 },
  { i: 'opponents',       x: 4, y: 20, w: 4,  h: 5, minW: 2, minH: 4 },
  { i: 'events',          x: 8, y: 20, w: 4,  h: 5, minW: 2, minH: 3 },
  { i: 'recommendations', x: 0, y: 25, w: 6,  h: 8, minW: 2, minH: 4 },
  { i: 'risk-chart',      x: 6, y: 25, w: 6,  h: 8, minW: 2, minH: 4 },
  { i: 'expected-draws',  x: 0, y: 33, w: 12, h: 8, minW: 4, minH: 5 },
]

const MD_LAYOUT_3P: LayoutItem[] = [
  { i: 'header',          x: 0, y: 0,  w: 5,  h: 4, minW: 5, minH: 2, maxH: 5 },
  { i: 'self-hand',       x: 5, y: 0,  w: 5,  h: 4, minW: 5, minH: 2, maxH: 6 },
  { i: 'bot-action',      x: 0, y: 4,  w: 4,  h: 5, minW: 3, minH: 2 },
  { i: 'bot-show',        x: 4, y: 4,  w: 3,  h: 8, minW: 2, minH: 3 },
  { i: 'bot-responses',   x: 7, y: 4,  w: 3,  h: 8, minW: 2, minH: 3 },
  { i: 'notifications',   x: 0, y: 9,  w: 4,  h: 3, minW: 2, minH: 3 },
  { i: 'player-0',        x: 0, y: 12, w: 3,  h: 8, minW: 2, minH: 6 },
  { i: 'player-1',        x: 3, y: 12, w: 4,  h: 8, minW: 2, minH: 6 },
  { i: 'player-2',        x: 7, y: 12, w: 3,  h: 8, minW: 2, minH: 6 },
  { i: 'proxy-control',   x: 0, y: 20, w: 3,  h: 5, minW: 2, minH: 2, maxH: 6 },
  { i: 'opponents',       x: 3, y: 20, w: 4,  h: 5, minW: 2, minH: 4 },
  { i: 'events',          x: 7, y: 20, w: 3,  h: 5, minW: 2, minH: 3 },
  { i: 'recommendations', x: 0, y: 25, w: 5,  h: 8, minW: 2, minH: 4 },
  { i: 'risk-chart',      x: 5, y: 25, w: 5,  h: 8, minW: 2, minH: 4 },
  { i: 'expected-draws',  x: 0, y: 33, w: 10, h: 8, minW: 4, minH: 5 },
]

const SM_LAYOUT_3P: LayoutItem[] = [
  { i: 'header',          x: 0, y: 0,  w: 6, h: 3, minW: 4, minH: 2, maxH: 5 },
  { i: 'player-0',        x: 0, y: 2,  w: 3, h: 8, minW: 2, minH: 6 },
  { i: 'player-1',        x: 3, y: 2,  w: 3, h: 8, minW: 2, minH: 6 },
  { i: 'player-2',        x: 0, y: 18, w: 6, h: 6, minW: 2, minH: 6 },
  { i: 'risk-chart',      x: 0, y: 24, w: 3, h: 6, minW: 2, minH: 4 },
  { i: 'opponents',       x: 3, y: 24, w: 3, h: 6, minW: 2, minH: 4 },
  { i: 'self-hand',       x: 0, y: 30, w: 6, h: 4, minW: 4, minH: 2, maxH: 6 },
  { i: 'events',          x: 0, y: 33, w: 3, h: 5, minW: 2, minH: 3 },
  { i: 'notifications',   x: 3, y: 33, w: 3, h: 5, minW: 2, minH: 3 },
  { i: 'proxy-control',   x: 0, y: 38, w: 3, h: 5, minW: 2, minH: 2, maxH: 6 },
  { i: 'bot-responses',   x: 3, y: 38, w: 3, h: 5, minW: 2, minH: 3 },
  { i: 'bot-action',      x: 0, y: 43, w: 6, h: 3, minW: 3, minH: 2 },
  { i: 'bot-show',        x: 0, y: 46, w: 6, h: 5, minW: 2, minH: 3 },
//{ i: 'recommendations', x: 0, y: 10, w: 6, h: 8, minW: 2, minH: 4 },
  { i: 'expected-draws',  x: 0, y: 51, w: 6, h: 8, minW: 4, minH: 5 },
]

const ALL_TILES_3P: TileId[] = ALL_TILES.filter((id) => id !== 'player-3')

const XS_LAYOUT_3P: LayoutItem[] = ALL_TILES_3P.map((id, i) => ({
  i: id,
  x: 0,
  y: i * 6,
  w: 4,
  h: id === 'header' || id === 'self-hand' ? 4 : 6,
  minW: 4,
  minH: 2,
}))

export const DEFAULT_LAYOUTS_3P: Record<Breakpoint, LayoutItem[]> = {
  lg: LG_LAYOUT_3P,
  md: MD_LAYOUT_3P,
  sm: SM_LAYOUT_3P,
  xs: XS_LAYOUT_3P,
}

/** 3p hides player-3 (no fourth player). */
export const DEFAULT_HIDDEN_3P: TileId[] = ['player-3']

export const TILE_TITLES: Record<TileId, string> = {
  'header':          'Game Header',
  'player-0':        'Player 1',
  'player-1':        'Player 2',
  'player-2':        'Player 3 (Self)',
  'player-3':        'Player 4',
  'self-hand':       'Self Hand',
  'recommendations': 'Recommendations',
  'expected-draws':  'Expected Draws',
  'risk-chart':      'Mixed Risk',
  'opponents':       'Opponents',
  'events':          'Game Events',
  'notifications':   'Notifications',
  'bot-responses':   'Bot Responses',
  'bot-action':      'Bot Action',
  'bot-show':        'Bot Display',
  'proxy-control':   'Proxy',
}
