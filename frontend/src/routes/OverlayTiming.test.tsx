import { act, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { BotResponse, GameStateSnapshot, MjaiEvent } from '@/types'

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (payload: unknown) => void>(),
  snapshot: null as GameStateSnapshot | null,
}))

vi.mock('@/lib/tauri', () => ({
  // Leave config pending; Overlay's fallback is sufficient for this test and
  // avoids an unrelated promise-driven render while fake timers are active.
  invoke: vi.fn((command: string) => {
    if (command === 'get_game_snapshot') return Promise.resolve(bridge.snapshot)
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

const startKyoku: MjaiEvent = {
  type: 'start_kyoku',
  bakaze: 'E',
  dora_marker: '1m',
  kyoku: 1,
  honba: 0,
  kyotaku: 0,
  oya: 1,
  scores: [25000, 25000, 25000, 25000],
  tehais: [
    ['1m', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', '1p', '2p', '3p', '4p'],
    [],
    [],
    [],
  ],
}

describe('overlay hand-animation gate', () => {
  beforeEach(() => {
    bridge.listeners.clear()
    bridge.snapshot = null
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('publishes a draw recommendation only after the rack has settled', () => {
    render(<Overlay />)
    emit<MjaiEvent>('mjai-event', { type: 'start_game', names: [], id: 0 })
    emit('mjai-event', startKyoku)
    emit<MjaiEvent>('mjai-event', { type: 'tsumo', actor: 0, pai: 'C' })
    emit<BotResponse>('bot-response', {
      type: 'dahai',
      actor: 0,
      pai: 'C',
      tsumogiri: true,
      meta: {
        show: { items: [{ label: 'Discard', pais: ['C'], value: '99%' }] },
      },
    })

    expect(screen.queryByText('打 99')).toBeNull()
    act(() => vi.advanceTimersByTime(HAND_LAYOUT_SETTLE_MS.draw - 1))
    expect(screen.queryByText('打 99')).toBeNull()
    act(() => vi.advanceTimersByTime(1))
    expect(screen.getByText('打 99')).toBeTruthy()
  })

  it('cancels a queued marker when a newer game event arrives', () => {
    render(<Overlay />)
    emit<MjaiEvent>('mjai-event', { type: 'start_game', names: [], id: 0 })
    emit('mjai-event', startKyoku)
    emit<MjaiEvent>('mjai-event', { type: 'tsumo', actor: 0, pai: 'C' })
    emit<BotResponse>('bot-response', {
      type: 'dahai',
      actor: 0,
      pai: 'C',
      tsumogiri: true,
      meta: {
        show: { items: [{ label: 'Discard', pais: ['C'], value: '99%' }] },
      },
    })

    emit<MjaiEvent>('mjai-event', {
      type: 'dahai',
      actor: 0,
      pai: 'C',
      tsumogiri: true,
    })
    act(() => vi.advanceTimersByTime(HAND_LAYOUT_SETTLE_MS.draw))
    expect(screen.queryByText('打 99')).toBeNull()
  })

  it('repairs a missed restore draw from the tracker snapshot before showing the prompt', async () => {
    render(<Overlay />)
    bridge.snapshot = {
      bakaze: 'S',
      kyoku: 2,
      honba: 0,
      kyotaku: 0,
      oya: 1,
      current_player: 0,
      turn_count: 8,
      phase: 'wait_act',
      is_done: false,
      num_players: 4,
      players: [{
        seat: 0,
        tehai: ['1m', '8m', 'C'],
        drawn_tile: 'C',
        melds: [],
        river: [],
        score: 25000,
        riichi_declared: false,
        riichi_stage: false,
        double_riichi: false,
        riichi_declaration_index: null,
        kita_tiles: [],
      }],
      dora_markers: ['2p'],
      our_seat: 0,
    }

    // Simulate the prompt arriving after the restore events were missed by
    // this webview. The snapshot refresh must restore C as the separated draw.
    emit<BotResponse>('bot-response', {
      type: 'dahai',
      actor: 0,
      pai: 'C',
      tsumogiri: true,
      meta: {
        show: { items: [{ label: 'Discard', pais: ['C'], value: '99%' }] },
      },
    })

    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(screen.getByText('打 99')).toBeTruthy()
  })
})
