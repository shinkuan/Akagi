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
    readyState = 0
    failSend = false
    sent: unknown[] = []
    onopen: (() => void) | null = null
    onclose: ((ev?: { code: number }) => void) | null = null
    onerror: (() => void) | null = null
    constructor(url: string) {
      super()
      if (url === ENDPOINT && FakeSocket.refuseEndpoint) throw new Error('refused')
      this.url = url
      FakeSocket.instances.push(this)
    }
    send(data: unknown) {
      if (this.failSend) throw new Error('InvalidStateError')
      this.sent.push(data)
    }
    open() {
      this.readyState = 1
      this.onopen?.()
    }
    receive(data: unknown) {
      this.dispatchEvent(new MessageEvent('message', { data }))
    }
    drop(code?: number) {
      this.readyState = 3
      this.dispatchEvent(new Event('close'))
      this.onclose?.(code === undefined ? undefined : { code })
    }
  }
  return FakeSocket
}

function makePage(overrides: Record<string, unknown> = {}) {
  const Socket = makeSocketClass()
  const timers: Array<() => void> = []
  const clock = { t: 0 }
  const logs: Array<{ level: string; msg: string }> = []
  const record = (level: string) => (msg: string) => logs.push({ level, msg })
  const win: Record<string, unknown> = {
    WebSocket: Socket,
    location: { origin: 'https://tenhou.net', pathname: '/4/' },
    performance: { now: () => clock.t },
    console: { info: record('info'), warn: record('warn'), error: record('error') },
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
  const lastAkagi = () => akagiSockets()[akagiSockets().length - 1]
  const messages = (sock = akagiSockets()[0]): Sent[] =>
    sock.sent.map((s) => JSON.parse(s as string) as Sent)
  const runLastTimer = () => timers[timers.length - 1]()
  const PageSocket = win.WebSocket as typeof Socket
  return {
    win,
    Socket,
    PageSocket,
    timers,
    clock,
    logs,
    run,
    akagiSockets,
    lastAkagi,
    messages,
    runLastTimer,
  }
}

// Let promise chains (and Blob reads) settle.
const settle = () => new Promise((r) => setTimeout(r, 20))

const frames = (msgs: Sent[]) =>
  msgs.filter((m) => m.t === 'frame').map(({ flow, dir, kind, data }) => ({ flow, dir, kind, data }))

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
    page.lastAkagi().open()
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
      { flow: 1, dir: 'up', kind: 'text', data: '<HELO/>' },
      { flow: 1, dir: 'down', kind: 'binary', data: 'AQID' },
      { flow: 1, dir: 'down', kind: 'binary', data: 'BAU=' },
      { flow: 1, dir: 'up', kind: 'binary', data: 'CAc=' },
    ])
    expect(msgs.at(-1)).toEqual({ t: 'close', flow: 1 })
  })

  it('keeps the page-visible shape of WebSocket', () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    expect(game.constructor).toBe(page.PageSocket)
    expect(page.PageSocket.prototype.constructor).toBe(page.PageSocket)
  })

  it('captures a page subclass that overrides addEventListener', async () => {
    const page = makePage()
    // The override runs before its own field exists if called from inside
    // `super()` — which is where the wrapper runs.
    class GameSocket extends page.PageSocket {
      field = { n: 1 }
      addEventListener(type: string, listener: EventListenerOrEventListenerObject | null) {
        void this.field.n
        super.addEventListener(type, listener)
      }
    }
    const game = new GameSocket('wss://game.example/')
    game.receive('hi')
    await settle()
    page.lastAkagi().open()
    expect(frames(page.messages())).toEqual([{ flow: 1, dir: 'down', kind: 'text', data: 'hi' }])
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

  it('forwards what was sent, not what the buffer became afterwards', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    const buf = new Uint8Array([1, 2, 3])
    game.send(buf)
    buf[0] = 9
    await settle()
    page.lastAkagi().open()
    expect(frames(page.messages())).toEqual([{ flow: 1, dir: 'up', kind: 'binary', data: 'AQID' }])
  })

  it('does not forward a send the socket refused', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    game.failSend = true
    expect(() => game.send('never sent')).toThrow('InvalidStateError')
    await settle()
    page.lastAkagi().open()
    expect(frames(page.messages())).toEqual([])
  })

  it('holds frames until Akagi is reachable, dropping the oldest past the cap', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    for (let i = 1; i <= 2005; i++) game.receive(`f${i}`)
    await settle()
    page.lastAkagi().open()

    const msgs = page.messages()
    const held = frames(msgs)
    expect(held).toHaveLength(2000)
    expect(held[0].data).toBe('f6')
    expect(held.at(-1)?.data).toBe('f2005')
    // `open` is never held, so the cap cannot push it out.
    expect(msgs).toContainEqual({ t: 'open', flow: 1, url: 'wss://game.example/' })
  })

  it('caps held frames by size too', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    const mib = 'x'.repeat(1024 * 1024)
    for (let i = 1; i <= 9; i++) game.receive(`${i}${mib}`)
    await settle()
    page.lastAkagi().open()
    const held = frames(page.messages()).map((f) => (f.data as string)[0])
    expect(held.length).toBeLessThan(9)
    expect(held.at(-1)).toBe('9')
  })

  it('only replays recent frames', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    game.receive('old')
    await settle()
    page.clock.t = 61_000
    game.receive('new')
    await settle()
    page.lastAkagi().open()
    expect(frames(page.messages()).map((f) => f.data)).toEqual(['new'])
  })

  // A game played while Akagi was closed must not be replayed into it as if
  // it were happening now.
  it('forgets held frames of a socket that closed before Akagi was reachable', async () => {
    const page = makePage()
    const finished = new page.PageSocket('wss://game.example/old')
    finished.receive('from a finished game')
    finished.drop()
    const current = new page.PageSocket('wss://game.example/new')
    current.receive('live')
    await settle()
    page.lastAkagi().open()

    const msgs = page.messages()
    expect(frames(msgs)).toEqual([{ flow: 2, dir: 'down', kind: 'text', data: 'live' }])
    expect(msgs.filter((m) => m.t === 'open')).toEqual([
      { t: 'open', flow: 2, url: 'wss://game.example/new' },
    ])
    expect(msgs.some((m) => m.t === 'close')).toBe(false)
  })

  it('stops once Akagi turns the page away', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    await settle()
    page.lastAkagi().open()
    page.lastAkagi().drop(1008)

    expect(page.timers).toHaveLength(0)
    game.receive('after refusal')
    new page.PageSocket('wss://game.example/again')
    await settle()
    expect(page.akagiSockets()).toHaveLength(1)
  })

  it('goes dormant while Akagi stays unreachable, and wakes on connect', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    game.receive('early')
    await settle()
    // Six failed connects run the backoff up to its ceiling.
    for (let i = 0; i < 6; i++) {
      page.lastAkagi().drop()
      if (i < 5) page.runLastTimer()
    }
    game.receive('while dormant')
    await settle()
    page.runLastTimer()
    page.lastAkagi().open()
    game.receive('awake')
    await settle()
    expect(frames(page.messages(page.lastAkagi())).map((f) => f.data)).toEqual(['early', 'awake'])
  })

  it('reconnects after Akagi goes away and re-announces live sockets', async () => {
    const page = makePage()
    new page.PageSocket('wss://game.example/')
    await settle()
    const first = page.lastAkagi()
    first.open()
    first.drop()

    expect(page.timers).toHaveLength(1)
    page.runLastTimer()
    const second = page.lastAkagi()
    expect(second).not.toBe(first)
    second.open()

    const msgs = page.messages(second)
    expect(msgs[0]).toMatchObject({ t: 'hello' })
    expect(msgs).toContainEqual({ t: 'open', flow: 1, url: 'wss://game.example/' })
  })

  it('does not forward its own connection to Akagi', async () => {
    const page = makePage()
    const game = new page.PageSocket('wss://game.example/')
    await settle()
    page.lastAkagi().open()
    game.receive('x')
    await settle()
    const msgs = page.messages()
    expect(msgs.filter((m) => m.t === 'open')).toEqual([
      { t: 'open', flow: 1, url: 'wss://game.example/' },
    ])
    expect(frames(msgs)).toEqual([{ flow: 1, dir: 'down', kind: 'text', data: 'x' }])
  })

  it('installs once even if loaded twice', async () => {
    const page = makePage()
    page.run()
    const game = new page.PageSocket('wss://game.example/')
    await settle()
    page.lastAkagi().open()
    game.receive('once')
    await settle()
    expect(frames(page.messages())).toEqual([{ flow: 1, dir: 'down', kind: 'text', data: 'once' }])
  })

  describe('console status', () => {
    const lines = (page: ReturnType<typeof makePage>, level: string) =>
      page.logs.filter((l) => l.level === level).map((l) => l.msg)

    it('reports starting, watching a socket and connecting', async () => {
      const page = makePage()
      new page.PageSocket('wss://game.example/')
      await settle()
      page.lastAkagi().open()
      const info = lines(page, 'info')
      expect(info[0]).toMatch(/^\[Akagi\] capture userscript active/)
      expect(info).toContain('[Akagi] watching game socket wss://game.example/')
      expect(info).toContain('[Akagi] connected to Akagi')
    })

    it('warns once when Akagi is unreachable, not on every retry', async () => {
      const page = makePage()
      new page.PageSocket('wss://game.example/')
      await settle()
      page.lastAkagi().drop()
      page.runLastTimer()
      page.lastAkagi().drop()
      const warns = lines(page, 'warn')
      expect(warns).toHaveLength(1)
      expect(warns[0]).toMatch(/cannot reach Akagi at ws:\/\/127\.0\.0\.1:23411\/ingest/)
    })

    // A blocked local-network permission fails the connection exactly like
    // Akagi not running; the console has to say which it is.
    it('names a blocked local network permission', async () => {
      const page = makePage({
        navigator: {
          permissions: {
            query: ({ name }: { name: string }) =>
              name === 'local-network-access'
                ? Promise.resolve({ state: 'denied' })
                : Promise.reject(new TypeError('unknown permission')),
          },
        },
      })
      new page.PageSocket('wss://game.example/')
      await settle()
      page.lastAkagi().drop()
      await settle()
      const errors = lines(page, 'error')
      expect(errors).toHaveLength(1)
      expect(errors[0]).toMatch(/not allowed to access devices on your local network/)
    })

    it('stays quiet about permissions it is not denied', async () => {
      const page = makePage({
        navigator: { permissions: { query: () => Promise.resolve({ state: 'granted' }) } },
      })
      new page.PageSocket('wss://game.example/')
      await settle()
      page.lastAkagi().drop()
      await settle()
      expect(lines(page, 'error')).toEqual([])
    })

    it('explains a refusal', async () => {
      const page = makePage()
      new page.PageSocket('wss://game.example/')
      await settle()
      page.lastAkagi().open()
      page.lastAkagi().drop(1008)
      expect(lines(page, 'warn').at(-1)).toMatch(/Akagi turned this page away/)
    })
  })
})
