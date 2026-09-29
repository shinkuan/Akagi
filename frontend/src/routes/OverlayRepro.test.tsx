import { act, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { BotResponse, GameStateSnapshot, MjaiEvent, ShowMeta } from '@/types'

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (payload: unknown) => void>(),
  snapshot: null as GameStateSnapshot | null,
  // Queue of snapshots returned by successive get_game_snapshot invokes.
  snapshotQueue: [] as Array<GameStateSnapshot | null>,
}))

vi.mock('@/lib/tauri', () => ({
  invoke: vi.fn((command: string) => {
    if (command === 'get_game_snapshot') {
      const next = bridge.snapshotQueue.length
        ? bridge.snapshotQueue.shift()!
        : bridge.snapshot
      return Promise.resolve(next)
    }
    return new Promise(() => {})
  }),
  listen: vi.fn((name: string, cb: (payload: unknown) => void) => {
    bridge.listeners.set(name, cb)
    return Promise.resolve(() => {
      if (bridge.listeners.get(name) === cb) bridge.listeners.delete(name)
    })
  }),
}))

import { HAND_LAYOUT_SETTLE_MS, Overlay } from './Overlay'

function emit<T>(name: string, payload: T) {
  const listener = bridge.listeners.get(name)
  if (!listener) throw new Error(`listener not registered: ${name}`)
  act(() => listener(payload))
}

const basePlayer = {
  seat: 0,
  tehai: [] as string[],
  drawn_tile: null as string | null,
  melds: [],
  river: [] as Array<{ tile: string; tedashi: boolean; is_riichi: boolean }>,
  score: 25000,
  riichi_declared: false,
  riichi_stage: false,
  double_riichi: false,
  riichi_declaration_index: null,
  kita_tiles: [],
}

function snapshot(over: Partial<GameStateSnapshot>): GameStateSnapshot {
  return {
    bakaze: 'E',
    kyoku: 1,
    honba: 0,
    kyotaku: 0,
    oya: 0,
    current_player: 0,
    turn_count: 0,
    phase: 'wait_act',
    is_done: false,
    num_players: 4,
    players: [basePlayer],
    dora_markers: ['8s'],
    our_seat: 0,
    ...over,
  }
}

const startGame: MjaiEvent = { type: 'start_game', names: ['a', 'b', 'c', 'd'], id: 0 }
const startKyoku: MjaiEvent = {
  type: 'start_kyoku',
  bakaze: 'E',
  dora_marker: '8s',
  kyoku: 1,
  honba: 0,
  kyotaku: 0,
  oya: 0,
  scores: [25000, 25000, 25000, 25000],
  tehais: [
    ['3m', '6m', '8m', '9m', '4p', '6p', '6p', '7p', '8p', '8p', '4s', '5s', 'S'],
    ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?'],
    ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?'],
    ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?'],
  ],
}
const openingTsumo: MjaiEvent = { type: 'tsumo', actor: 0, pai: '5sr' }

// 22:37 hanchan E1 physical layout: 14 tiles continuous, 5sr sorted first
// (red five sorts before ordinary tiles).
const physical14 = ['5sr', '3m', '6m', '8m', '9m', '4p', '6p', '6p', '7p', '8p', '8p', '4s', '5s', 'S']
const preTsumo13 = ['3m', '6m', '8m', '9m', '4p', '6p', '6p', '7p', '8p', '8p', '4s', '5s', 'S']

function botResponse(pai: string): BotResponse {
  return {
    type: 'dahai',
    actor: 0,
    pai,
    tsumogiri: false,
    meta: {
      show: { items: [{ label: 'Discard', pais: [pai], value: '99%' }] },
    },
  }
}

describe('hanchan-start hydration race', () => {
  beforeEach(() => {
    bridge.listeners.clear()
    bridge.snapshot = null
    bridge.snapshotQueue = []
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('keeps the dealer opening marker on the correct tile when hydration sees the pre-tsumo 13-tile snapshot', async () => {
    // The tracker has consumed start_kyoku but not yet the opening tsumo when
    // the overlay hydrates; the bot response later hydrates from the current
    // 14-tile snapshot (production flow).
    bridge.snapshotQueue.push(
      snapshot({
        players: [{ ...basePlayer, tehai: preTsumo13 }],
        current_player: 255,
      }),
      snapshot({
        players: [{ ...basePlayer, tehai: physical14, drawn_tile: '5sr' }],
      }),
    )

    render(<Overlay />)
    emit('mjai-event', startGame)
    emit('mjai-event', startKyoku)
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    emit('mjai-event', openingTsumo)
    emit<BotResponse>('bot-response', botResponse('S'))
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    // The dealer opening rack settles with the longer 2000 ms window; the
    // 500 ms draw window would publish the hint over a still-animating rack.
    act(() => vi.advanceTimersByTime(HAND_LAYOUT_SETTLE_MS.dealerOpening))

    const marker = screen.getByText('打 99')
    // S is the 14th tile in the continuous opening rack. A separated 13+1
    // model would put it at index 12 (13th tile) instead.
    const left = Number((marker.closest('.tile-hint') as HTMLElement)?.style.left.replace('px', ''))
    expect(left).toBeGreaterThan(0)
  })

  it('reproduces the wrong opening layout when the pre-tsumo hydration is the only resync', async () => {
    // Simulate the overlay hydrating from the pre-tsumo snapshot and then no
    // later hydration happening before the hint renders (bot response without
    // a usable snapshot refresh, e.g. dropped by a racing event).
    bridge.snapshotQueue.push(
      snapshot({
        players: [{ ...basePlayer, tehai: preTsumo13 }],
        current_player: 255,
      }),
    )

    render(<Overlay />)
    emit('mjai-event', startGame)
    emit('mjai-event', startKyoku)
    await act(async () => { await Promise.resolve(); await Promise.resolve() })
    emit('mjai-event', openingTsumo)

    // Read the hand state through the exported reducer path: after hydration
    // the opening tsumo is treated as a separated draw instead of a
    // continuous rack.
    const { reduceHand, handFromSnapshot, markersFor } = await import('./Overlay')
    const hydrated = handFromSnapshot(snapshot({
      players: [{ ...basePlayer, tehai: preTsumo13 }],
      current_player: 255,
    }))
    const afterTsumo = reduceHand(hydrated, openingTsumo)
    expect(afterTsumo.dealerOpening).toBe(false)
    expect(afterTsumo.drawn).toBe('5sr')
    expect(afterTsumo.tiles).toHaveLength(13)

    const markers = markersFor(
      (botResponse('S').meta?.show as ShowMeta | undefined) ?? null,
      afterTsumo,
      3,
    )
    expect(markers[0].handIndex).toBe(12) // S sits at index 12 in the 13-rack
    expect(afterTsumo.tiles[markers[0].handIndex]).toBe('S')
    // But the physical opening rack has 14 continuous tiles; S is at index 13.
    expect(physical14[13]).toBe('S')
    expect(markers[0].handIndex).not.toBe(13)
  })
})
