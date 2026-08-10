import { invoke } from '@/lib/tauri'
import { useConfigStore } from '@/stores/configStore'
import { effectiveProxy } from '@/lib/proxy'
import type { KeyStatus, NativeApiConfig } from '@/types'

export type ApiProfile = Pick<NativeApiConfig, 'base_url' | 'key' | 'model_4p' | 'model_3p'>

export const apiProvider = (api: NativeApiConfig) =>
  api.provider.toLowerCase() === 'flya' ? 'flya' : 'original'

export function selectedApiProfile(api: NativeApiConfig): ApiProfile {
  return apiProvider(api) === 'flya'
    ? {
        base_url: api.flya_base_url,
        key: api.flya_key,
        model_4p: api.flya_model_4p,
        model_3p: api.flya_model_3p,
      }
    : {
        base_url: api.base_url,
        key: api.key,
        model_4p: api.model_4p,
        model_3p: api.model_3p,
      }
}

export function withSelectedApiProfile(
  api: NativeApiConfig,
  patch: Partial<ApiProfile>,
): NativeApiConfig {
  if (apiProvider(api) === 'flya') {
    return {
      ...api,
      ...(patch.base_url === undefined ? {} : { flya_base_url: patch.base_url }),
      ...(patch.key === undefined ? {} : { flya_key: patch.key }),
      ...(patch.model_4p === undefined ? {} : { flya_model_4p: patch.model_4p }),
      ...(patch.model_3p === undefined ? {} : { flya_model_3p: patch.model_3p }),
    }
  }
  return { ...api, ...patch }
}

/**
 * Result of {@link checkApiBeforeSave}. `ok` gates whether the caller may
 * persist the config; `kind` distinguishes the two blocking reasons so the
 * caller can pick the right (localised) message:
 *  - `missing` — enabled but no server URL / key entered yet.
 *  - `error`   — the server rejected the key; `message` is the raw reason.
 */
export type ApiSaveCheck =
  | { ok: true }
  | { ok: false; kind: 'missing' }
  | { ok: false; kind: 'error'; message: string }

/**
 * Guard run before persisting `bot.api`: when cloud inference is **enabled**,
 * confirm the key actually works (via `GET /v3/key`) so a broken key can't be
 * saved in the enabled state — otherwise the built-in bot would silently fall
 * back to the local model every turn with no signal to the user. A disabled
 * API always passes; there is nothing to check.
 */
export async function checkApiBeforeSave(api: NativeApiConfig): Promise<ApiSaveCheck> {
  if (!api.enabled) return { ok: true }
  const profile = selectedApiProfile(api)
  if (profile.base_url.trim() === '' || profile.key.trim() === '') {
    return { ok: false, kind: 'missing' }
  }
  try {
    await invoke<KeyStatus>('native_api_key_status', {
      provider: apiProvider(api),
      baseUrl: profile.base_url,
      proxy: effectiveProxy(api),
      key: profile.key,
    })
    return { ok: true }
  } catch (e) {
    return { ok: false, kind: 'error', message: String(e) }
  }
}

/**
 * Persist a `bot.api` change immediately, layered on the *stored* (on-disk)
 * config so only `bot.api` differs from disk. That keeps `update_config` from
 * restarting capture (it only restarts on capture/proxy/platform changes) and
 * touches nothing else. Used for the one case that must not wait for an
 * explicit Save: a redeemed single-use code whose key the server shows once.
 */
export async function persistApiConfig(api: NativeApiConfig): Promise<void> {
  const store = useConfigStore.getState()
  const cfg = store.config
  if (!cfg) return
  const next = { ...cfg, bot: { ...cfg.bot, api } }
  await invoke('update_config', { newConfig: next })
  store.setConfig(next)
}
