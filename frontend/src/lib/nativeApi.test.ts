import { beforeEach, describe, expect, it, vi } from 'vitest'

import { apiProvider, checkApiBeforeSave } from './nativeApi'
import type { NativeApiConfig } from '@/types'

// The helper reaches the backend through `@/lib/tauri`'s `invoke`; mock it so
// the test controls whether the key-status endpoint accepts or rejects the key.
const invoke = vi.fn()
vi.mock('@/lib/tauri', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
}))

const api = (patch: Partial<NativeApiConfig> = {}): NativeApiConfig => ({
  enabled: true,
  provider: 'original',
  base_url: 'https://mjapi.example.test',
  key: 'sk-test-key',
  model_4p: '',
  model_3p: '',
  flya_base_url: 'https://api.nashout.com',
  flya_key: '',
  flya_model_4p: '',
  flya_model_3p: '',
  proxy_enabled: false,
  proxy: '',
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
      provider: 'original',
      baseUrl: cfg.base_url,
      proxy: '',
      key: cfg.key,
    })
  })

  it('checks the selected FlyA profile without overwriting the original profile', async () => {
    invoke.mockResolvedValueOnce({ status: 'active' })
    const cfg = api({
      provider: 'flya',
      flya_base_url: 'https://flya.example.test',
      flya_key: 'flyat_test',
    })
    expect(await checkApiBeforeSave(cfg)).toEqual({ ok: true })
    expect(invoke).toHaveBeenCalledWith('native_api_key_status', {
      provider: 'flya',
      baseUrl: 'https://flya.example.test',
      proxy: '',
      key: 'flyat_test',
    })
    expect(cfg.base_url).toBe('https://mjapi.example.test')
    expect(cfg.key).toBe('sk-test-key')
  })

  it('normalizes a legacy mixed-case FlyA provider value', () => {
    expect(apiProvider(api({ provider: 'FlyA' as NativeApiConfig['provider'] }))).toBe('flya')
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
