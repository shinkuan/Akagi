import i18n from 'i18next'
import { invoke } from '@/lib/tauri'
import { toast } from '@/components/ui/sonner'
import { NATIVE_3P, NATIVE_4P } from '@/lib/nativeBots'
import { effectiveProxy } from '@/lib/proxy'
import { useConfigStore } from '@/stores/configStore'
import type { AppConfig, KeyStatus, NativeApiConfig } from '@/types'

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
 * back to the local model every turn with no signal to the user. The check
 * travels through the config's proxy ([`effectiveProxy`]): it must succeed on
 * the same route live inference will use, or a server only reachable via
 * proxy could never be saved. A disabled API always passes; there is nothing
 * to check.
 */
export async function checkApiBeforeSave(api: NativeApiConfig): Promise<ApiSaveCheck> {
  if (!api.enabled) return { ok: true }
  if (api.base_url.trim() === '' || api.key.trim() === '') {
    return { ok: false, kind: 'missing' }
  }
  try {
    await invoke<KeyStatus>('native_api_key_status', {
      baseUrl: api.base_url,
      proxy: effectiveProxy(api),
      key: api.key,
    })
    return { ok: true }
  } catch (e) {
    return { ok: false, kind: 'error', message: String(e) }
  }
}

/**
 * Fold an edited `bot.api` into a config — and, when the API is **enabled**,
 * make the built-in native bots the active ones for both modes. MJOT cloud
 * inference only ever applies to the built-in bot: with an author bot active,
 * an enabled API is dead config the user thinks is working. `switched` tells
 * the caller whether the active-bot selection actually changed, so it can say
 * so (the user may have picked another bot deliberately).
 */
export function withNativeBotForApi(
  cfg: AppConfig,
  api: NativeApiConfig,
): { next: AppConfig; switched: boolean } {
  const bot = { ...cfg.bot, api }
  const switched = api.enabled && (bot.active_4p !== NATIVE_4P || bot.active_3p !== NATIVE_3P)
  if (switched) {
    bot.active_4p = NATIVE_4P
    bot.active_3p = NATIVE_3P
  }
  return { next: { ...cfg, bot }, switched }
}

/**
 * Persist a `bot.api` change immediately, layered on the *stored* (on-disk)
 * config so nothing capture-relevant differs from disk — that keeps
 * `update_config` from restarting capture (it only restarts on
 * capture/proxy/platform changes). Used for the one case that must not wait
 * for an explicit Save: a redeemed single-use code whose key the server shows
 * once. An enabled API also selects the built-in bots (see
 * [`withNativeBotForApi`]); active-bot changes don't restart anything either.
 */
export async function persistApiConfig(api: NativeApiConfig): Promise<void> {
  const store = useConfigStore.getState()
  const cfg = store.config
  if (!cfg) return
  const { next, switched } = withNativeBotForApi(cfg, api)
  await invoke('update_config', { newConfig: next })
  store.setConfig(next)
  if (switched) toast.info(i18n.t('bots.api.native_selected'))
}
