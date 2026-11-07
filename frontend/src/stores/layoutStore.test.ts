import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { LayoutItem } from 'react-grid-layout'
import { ALL_TILES, DEFAULT_LAYOUTS } from '@/tiles/defaults'

// Node 22+ ships an experimental `localStorage` global that shadows jsdom's, so
// install an explicit in-memory stand-in the store and the assertions both see.
function stubLocalStorage() {
  const backing = new Map<string, string>()
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => backing.get(k) ?? null,
    setItem: (k: string, v: string) => void backing.set(k, String(v)),
    removeItem: (k: string) => void backing.delete(k),
    clear: () => backing.clear(),
  })
  return backing
}

// The store reads localStorage at module-init, so each test re-imports a fresh
// copy after seeding storage to exercise the load path.
async function freshStore() {
  vi.resetModules()
  return await import('./layoutStore')
}

const LAYOUTS_4P = 'akagi.dashboard.layouts.4p'

function seed(backing: Map<string, string>, lg: unknown[]) {
  backing.set(
    LAYOUTS_4P,
    JSON.stringify({ schema: 1, data: { lg, md: lg, sm: lg, xs: lg } }),
  )
}

describe('layoutStore #300 — tiles revealed from the Add menu', () => {
  beforeEach(() => {
    stubLocalStorage()
  })

  it('gives a revealed tile a slot at the breakpoint that shows it', async () => {
    const { useLayoutStore } = await freshStore()
    useLayoutStore.getState().show('recommendations', 'lg')

    const s = useLayoutStore.getState()
    expect(s.hidden.lg).not.toContain('recommendations')
    expect(s.layouts.lg.find((l) => l.i === 'recommendations')).toBeDefined()
    // Visibility stays per-breakpoint: the tile is still hidden at the other
    // widths, so it is not rendered there and needs no slot.
    expect(s.hidden.md).toContain('recommendations')
  })

  it('keeps the user position and size of a tile that has no default slot', async () => {
    const first = await freshStore()
    first.useLayoutStore.getState().show('recommendations', 'lg')

    // What react-grid-layout reports once the user drags and resizes the tile.
    const layouts = structuredClone(first.useLayoutStore.getState().layouts)
    layouts.lg = layouts.lg.map((l) =>
      l.i === 'recommendations' ? { ...l, x: 2, y: 5, w: 6, h: 10 } : l,
    )
    first.useLayoutStore.getState().setLayouts(layouts)

    const restarted = await freshStore()
    const item = restarted.useLayoutStore.getState().layouts.lg.find((l) => l.i === 'recommendations')
    expect(item).toBeDefined()
    expect(item).toMatchObject({ x: 2, y: 5, w: 6, h: 10 })
  })

  it('heals a 1x1 bottom-of-grid placeholder those tiles used to be saved as', async () => {
    const backing = stubLocalStorage()
    seed(backing, [
      ...DEFAULT_LAYOUTS.lg.map((l) => ({ i: l.i, x: l.x, y: l.y, w: l.w, h: l.h })),
      // `synchronizeLayoutWithChildren` synthesises exactly this when a visible
      // child has no layout entry, and the next layout change persists it.
      { i: 'recommendations', x: 0, y: 999, w: 1, h: 1 },
    ])

    const { useLayoutStore } = await freshStore()
    const item = useLayoutStore.getState().layouts.lg.find((l) => l.i === 'recommendations') as LayoutItem
    expect(item).toBeDefined()
    expect(item.w).toBeGreaterThanOrEqual(item.minW ?? 1)
    expect(item.h).toBeGreaterThanOrEqual(item.minH ?? 1)
  })

  it('adds a slot for a tile the defaults show but do not lay out', async () => {
    // `DEFAULT_LAYOUTS.lg` has no `recommendations` entry, and the default is to
    // hide it — but if a save says it is visible, it must get a real slot.
    const backing = stubLocalStorage()
    backing.set('akagi.dashboard.hidden.4p', JSON.stringify({ schema: 1, data: { lg: [], md: [], sm: [], xs: [] } }))
    seed(backing, DEFAULT_LAYOUTS.lg.map((l) => ({ i: l.i, x: l.x, y: l.y, w: l.w, h: l.h })))

    const { useLayoutStore } = await freshStore()
    const item = useLayoutStore.getState().layouts.lg.find((l) => l.i === 'recommendations') as LayoutItem
    expect(item).toBeDefined()
    expect(item.w).toBe(4)
    expect(item.h).toBe(4)
  })
  it('still drops saved tiles that no longer exist in the registry', async () => {
    const backing = stubLocalStorage()
    seed(backing, [
      ...DEFAULT_LAYOUTS.lg.map((l) => ({ i: l.i, x: l.x, y: l.y, w: l.w, h: l.h })),
      { i: 'removed-tile', x: 0, y: 0, w: 4, h: 4 },
    ])

    const { useLayoutStore } = await freshStore()
    expect(useLayoutStore.getState().layouts.lg.map((l) => l.i)).not.toContain('removed-tile')
  })

  it('exposes every registry tile to the Add menu as a known tile', () => {
    // Guards the invariant the fix relies on: `recommendations` is renderable
    // even though the default 4p layouts give it no slot.
    expect(ALL_TILES).toContain('recommendations')
    expect(DEFAULT_LAYOUTS.lg.map((l) => l.i)).not.toContain('recommendations')
  })
})
