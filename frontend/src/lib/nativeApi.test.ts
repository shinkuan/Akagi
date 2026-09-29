import { beforeEach, describe, expect, it, vi } from 'vitest'

import { checkApiBeforeSave } from './nativeApi'
import type { NativeApiConfig } from '@/types'

// The helper reaches the backend through `@/lib/tauri`'s `invoke`; mock it so
// the test controls whether the key-status endpoint accepts or rejects the key.
const invoke = vi.fn()
vi.mock('@/lib/tauri', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
}))

const api = (patch: Partial<NativeApiConfig> = {}): NativeApiConfig => ({
  enabled: true,
  base_url: 'https://mjapi.example.test',
  key: 'sk-test-key',
  model_4p: '',
  model_3p: '',
  proxy_enabled: false,
  proxy: '',
  react_timeout_ms: 3000,
  ...patch,
})

describe('checkApiBeforeSave', () => {
  beforeEach(() => {
    invoke.mockReset()
  })

  it('passes without hitting the server when the API is disabled', async () => {
    // A disabled API has nothing to verify — Save must proceed even with a
    // blank/garbage key, and we must not make a network call to check it.
    const res = await checkApiBeforeSave(api({ enabled: false, key: '' }))
    expect(res).toEqual({ ok: true })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('blocks as `missing` when enabled but the URL or key is blank', async () => {
    // Enabled with an empty key can't possibly work; report it as `missing`
    // (a distinct, friendlier reason) instead of firing a doomed request.
    expect(await checkApiBeforeSave(api({ key: '   ' }))).toEqual({
      ok: false,
      kind: 'missing',
    })
    expect(await checkApiBeforeSave(api({ base_url: '' }))).toEqual({
      ok: false,
      kind: 'missing',
    })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('passes when the server accepts the key', async () => {
    invoke.mockResolvedValueOnce({ plan: 'pro' })
    const cfg = api()
    expect(await checkApiBeforeSave(cfg)).toEqual({ ok: true })
    expect(invoke).toHaveBeenCalledWith('native_api_key_status', {
      baseUrl: cfg.base_url,
      proxy: '',
      key: cfg.key,
    })
  })

  // Regression: the pre-save check used to omit `proxy`, so it went direct.
  // On a network where the inference server is only reachable through the
  // proxy, Save could never pass — and the proxy setting never persisted.
  it('routes the pre-save check through an enabled proxy', async () => {
    invoke.mockResolvedValueOnce({ plan: 'pro' })
    const cfg = api({ proxy_enabled: true, proxy: '  socks5://127.0.0.1:1080  ' })
    expect(await checkApiBeforeSave(cfg)).toEqual({ ok: true })
    expect(invoke).toHaveBeenCalledWith('native_api_key_status', {
      baseUrl: cfg.base_url,
      // Trimmed, and only because the toggle is on (see `effectiveProxy`).
      proxy: 'socks5://127.0.0.1:1080',
      key: cfg.key,
    })
  })

  it('blocks as `error` and forwards the reason when the server rejects the key', async () => {
    invoke.mockRejectedValueOnce('401 Unauthorized: key expired')
    expect(await checkApiBeforeSave(api())).toEqual({
      ok: false,
      kind: 'error',
      message: '401 Unauthorized: key expired',
    })
  })
})
