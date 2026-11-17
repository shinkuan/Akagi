import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { LayoutItem } from 'react-grid-layout'
import { ALL_TILES, DEFAULT_LAYOUTS, DEFAULT_LAYOUTS_3P } from '@/tiles/defaults'

// A fresh in-memory localStorage per test; the store reads it at module init.
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

// Re-import the store so it loads from whatever storage holds now.
async function freshStore() {
  vi.resetModules()
  return await import('./layoutStore')
}

const LAYOUTS_4P = 'akagi.dashboard.layouts.4p'

function seed(backing: Map<string, string>, lg: unknown[]) {
  backing.set(LAYOUTS_4P, JSON.stringify({ schema: 1, data: { lg, md: lg, sm: lg, xs: lg } }))
}

const savedDefaults = () => DEFAULT_LAYOUTS.lg.map((l) => ({ i: l.i, x: l.x, y: l.y, w: l.w, h: l.h }))

describe('layoutStore #300', () => {
  let backing: Map<string, string>
  beforeEach(() => {
    backing = stubLocalStorage()
  })

  // The load path drops saved entries with no default slot, so a tile missing
  // from a preset lost its saved position (or, if visible, got a 1x1 placeholder).
  it('gives every tile a slot in every preset', () => {
    for (const bp of ['lg', 'md', 'sm', 'xs'] as const) {
      const ids4p = DEFAULT_LAYOUTS[bp].map((l) => l.i)
      expect(ids4p, `4p ${bp}`).toEqual(expect.arrayContaining(ALL_TILES))
      const ids3p = DEFAULT_LAYOUTS_3P[bp].map((l) => l.i)
      const tiles3p = ALL_TILES.filter((id) => id !== 'player-3')
      expect(ids3p, `3p ${bp}`).toEqual(expect.arrayContaining(tiles3p))
    }
  })

  it('keeps where the user put a revealed tile across a restart', async () => {
    const first = await freshStore()
    first.useLayoutStore.getState().show('recommendations', 'lg')
    const layouts = structuredClone(first.useLayoutStore.getState().layouts)
    layouts.lg = layouts.lg.map((l) =>
      l.i === 'recommendations' ? { ...l, x: 2, y: 5, w: 6, h: 10 } : l,
    )
    first.useLayoutStore.getState().setLayouts(layouts)

    const restarted = await freshStore()
    const item = restarted.useLayoutStore.getState().layouts.lg.find((l) => l.i === 'recommendations')
    expect(item).toMatchObject({ x: 2, y: 5, w: 6, h: 10 })
  })

  it('grows the 1x1 placeholder older builds saved back to the minimum size', async () => {
    seed(backing, [...savedDefaults(), { i: 'recommendations', x: 0, y: 999, w: 1, h: 1 }])
    const { useLayoutStore } = await freshStore()
    const item = useLayoutStore.getState().layouts.lg.find((l) => l.i === 'recommendations') as LayoutItem
    expect(item.w).toBeGreaterThanOrEqual(item.minW ?? 1)
    expect(item.h).toBeGreaterThanOrEqual(item.minH ?? 1)
  })

  it('still drops saved tiles that no longer exist', async () => {
    seed(backing, [...savedDefaults(), { i: 'removed-tile', x: 0, y: 0, w: 4, h: 4 }])
    const { useLayoutStore } = await freshStore()
    expect(useLayoutStore.getState().layouts.lg.map((l) => l.i)).not.toContain('removed-tile')
  })
})
