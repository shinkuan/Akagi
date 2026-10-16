// The ingest userscript, run against a fake page.
//
// The script is served by the Rust backend (`src/capture/ingest/`), but it
// is plain browser JavaScript, so this is where it can actually be run. The
// committed template is valid JS as-is — the endpoint placeholder is a
// string literal — so it is loaded raw and the placeholder swapped here.
//
// What matters most is the script's one rule: the game behaves exactly as
// if it were not installed. Frames reach the game untouched, and nothing
// that goes wrong on the way to Akagi reaches the page.

import { describe, expect, it } from 'vitest'
import template from '../../../src/capture/ingest/akagi.user.js?raw'

const ENDPOINT = 'ws://127.0.0.1:23411/ingest'
const source = template.replace('__AKAGI_ENDPOINT__', ENDPOINT)

type Sent = Record<string, unknown>

// A fresh WebSocket class per page: the script patches the prototype's
// `send`, so pages must not share one.
function makeSocketClass() {
  class FakeSocket extends EventTarget {
    static instances: FakeSocket[] = []
    static refuseEndpoint = false
    url: string
    sent: unknown[] = []
    onopen: (() => void) | null = null
    onclose: (() => void) | null = null
    onerror: (() => void) | null = null
    constructor(url: string) {
      super()
      if (url === ENDPOINT && FakeSocket.refuseEndpoint) throw new Error('refused')
      this.url = url
      FakeSocket.instances.push(this)
    }
    send(data: unknown) {
      this.sent.push(data)
    }
    open() {
      this.onopen?.()
    }
    receive(data: unknown) {
      this.dispatchEvent(new MessageEvent('message', { data }))
    }
    drop() {
      this.dispatchEvent(new Event('close'))
      this.onclose?.()
    }
  }
  return FakeSocket
}

function makePage(overrides: Record<string, unknown> = {}) {
  const Socket = makeSocketClass()
  const timers: Array<() => void> = []
  const win: Record<string, unknown> = {
    WebSocket: Socket,
    location: { origin: 'https://tenhou.net', pathname: '/4/' },
    setTimeout: (fn: () => void) => {
      timers.push(fn)
      return timers.length
    },
    btoa: (s: string) => globalThis.btoa(s),
    Blob: globalThis.Blob,
    FileReader: globalThis.FileReader,
    ...overrides,
  }
  const run = () => new Function('window', source)(win)
  run()
  const akagiSockets = () => Socket.instances.filter((s) => s.url === ENDPOINT)
  const messages = (sock = akagiSockets()[0]): Sent[] =>
    sock.sent.map((s) => JSON.parse(s as string) as Sent)
  const PageSocket = win.WebSocket as typeof Socket
  return { win, Socket, PageSocket, timers, run, akagiSockets, messages }
}

// Let promise chains (and Blob reads) settle.
const settle = () => new Promise((r) => setTimeout(r, 20))

const frames = (msgs: Sent[]) =>
  msgs.filter((m) => m.t === 'frame').map(({ dir, kind, data }) => ({ dir, kind, data }))

describe('ingest userscript', () => {
  it('forwards both directions in order and leaves the game socket untouched', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    expect(game).toBeInstanceOf(page.Socket)
    expect(game).toBeInstanceOf(page.PageSocket)

    const view = new Uint8Array([9, 8, 7, 6]).subarray(1, 3)
    game.send('<HELO/>')
    game.receive(new Uint8Array([1, 2, 3]).buffer)
    game.receive(new Blob([new Uint8Array([4, 5])]))
    game.send(view)

    // The game got exactly what the page sent, as the same objects.
    expect(game.sent).toEqual(['<HELO/>', view])
    expect(game.sent[1]).toBe(view)

    await settle()
    page.akagiSockets()[0].open()
    game.drop()
    await settle()

    const msgs = page.messages()
    expect(msgs[0]).toMatchObject({
      t: 'hello',
      v: 1,
      origin: 'https://tenhou.net',
      url: 'https://tenhou.net/4/',
    })
    expect(msgs).toContainEqual({ t: 'open', flow: 1, url: 'wss://game.example/' })
    // The Blob is read asynchronously, yet nothing overtakes it.
    expect(frames(msgs)).toEqual([
      { dir: 'up', kind: 'text', data: '<HELO/>' },
      { dir: 'down', kind: 'binary', data: 'AQID' },
      { dir: 'down', kind: 'binary', data: 'BAU=' },
      { dir: 'up', kind: 'binary', data: 'CAc=' },
    ])
    expect(msgs.at(-1)).toEqual({ t: 'close', flow: 1 })
  })

  it('never lets a failure on the way to Akagi reach the page', async () => {
    const page = makePage({
      btoa: () => {
        throw new Error('boom')
      },
    })
    page.Socket.refuseEndpoint = true
    const game = new page.PageSocket('wss://game.example/')
    expect(() => game.send(new Uint8Array([1]))).not.toThrow()
    expect(() => game.receive(new Uint8Array([2]).buffer)).not.toThrow()
    await settle()
    expect(game.sent).toHaveLength(1)
    // Akagi was unreachable; a retry is scheduled rather than given up on.
    expect(page.timers.length).toBeGreaterThan(0)
  })

  it('holds frames until Akagi is reachable, dropping the oldest past the cap', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    for (let i = 1; i <= 2005; i++) game.receive(`f${i}`)
    await settle()
    page.akagiSockets()[0].open()

    const msgs = page.messages()
    const held = frames(msgs)
    expect(held).toHaveLength(2000)
    expect(held[0].data).toBe('f6')
    expect(held.at(-1)?.data).toBe('f2005')
    // `open` is never queued, so the cap cannot push it out.
    expect(msgs).toContainEqual({ t: 'open', flow: 1, url: 'wss://game.example/' })
  })

  it('reconnects after Akagi goes away and re-announces live sockets', async () => {
    const page = makePage()
    new page.PageSocket('wss://game.example/')
    await settle()
    const first = page.akagiSockets()[0]
    first.open()
    first.drop()

    expect(page.timers).toHaveLength(1)
    page.timers[0]()
    const second = page.akagiSockets()[1]
    expect(second).toBeDefined()
    second.open()

    const msgs = page.messages(second)
    expect(msgs[0]).toMatchObject({ t: 'hello' })
    expect(msgs).toContainEqual({ t: 'open', flow: 1, url: 'wss://game.example/' })
  })

  it('does not forward its own connection to Akagi', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    await settle()
    page.akagiSockets()[0].open()
    game.receive('x')
    await settle()
    const msgs = page.messages()
    expect(msgs.filter((m) => m.t === 'open')).toEqual([
      { t: 'open', flow: 1, url: 'wss://game.example/' },
    ])
    expect(frames(msgs)).toEqual([{ dir: 'down', kind: 'text', data: 'x' }])
  })

  it('installs once even if loaded twice', async () => {
    const page = makePage()
    page.run()
    const game = new page.PageSocket('wss://game.example/')
    await settle()
    page.akagiSockets()[0].open()
    game.receive('once')
    await settle()
    expect(frames(page.messages())).toEqual([{ dir: 'down', kind: 'text', data: 'once' }])
  })
})
