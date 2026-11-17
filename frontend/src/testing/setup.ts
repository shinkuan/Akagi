import { afterEach } from 'vitest'
import { cleanup } from '@testing-library/react'

// Vitest runs with `globals: false`, so Testing Library's auto-cleanup (which
// hooks a global `afterEach`) never registers. Do it explicitly.
afterEach(cleanup)

// Node ≥25's own Web Storage globals shadow jsdom's and are undefined without
// `--localstorage-file`; put jsdom's back.
for (const name of ['localStorage', 'sessionStorage'] as const) {
  if (globalThis[name] === undefined) {
    const { window } = (globalThis as unknown as { jsdom: { window: Window } }).jsdom
    Object.defineProperty(globalThis, name, { value: window[name], configurable: true })
  }
}

// jsdom ships no ResizeObserver, which Radix's ScrollArea constructs on mount.
if (!('ResizeObserver' in globalThis)) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
}

/**
 * jsdom implements no `matchMedia`. Install a stub whose match state the test
 * controls, so components using `useIsNarrow` can be driven across the `lg`
 * breakpoint.
 *
 * Returns a `setMatches` that flips the result *and* notifies subscribers, the
 * way a real resize would.
 */
export function mockMatchMedia(initialMatches: boolean) {
  const listeners = new Set<() => void>()
  let matches = initialMatches

  window.matchMedia = ((query: string) => ({
    matches,
    media: query,
    onchange: null,
    addEventListener: (_: string, cb: () => void) => void listeners.add(cb),
    removeEventListener: (_: string, cb: () => void) => void listeners.delete(cb),
    addListener: (cb: () => void) => void listeners.add(cb),
    removeListener: (cb: () => void) => void listeners.delete(cb),
    dispatchEvent: () => false,
  })) as unknown as typeof window.matchMedia

  return function setMatches(next: boolean) {
    matches = next
    for (const cb of listeners) cb()
  }
}
