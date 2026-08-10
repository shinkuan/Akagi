import { create } from 'zustand'
import i18n from 'i18next'
import { invoke } from '@/lib/tauri'
import { openExternal } from '@/lib/external'
import { persistApiConfig } from '@/lib/nativeApi'
import { toast } from '@/components/ui/sonner'
import { useConfigStore } from '@/stores/configStore'
import type { Product } from '@/lib/products'
import type {
  CreatedOrder,
  CreatedSubscription,
  KeyStatus,
  OrderResult,
  RedeemResponse,
  SubscriptionResult,
} from '@/types'

// Drives the self-serve key purchase handshake (see the inference server's API documentation):
// create → open approve_url in the browser → poll *-result every few seconds
// → the key.
//
// How the key arrives depends on the purchase:
//  - subscription           → the poll carries it directly.
//  - one-time, new key      → `create-order` is sent `redeem: true`, so the
//                             SERVER redeems the prepaid code and the poll
//                             carries the key. The buyer's backup email then
//                             holds the key too, not a one-time code the app
//                             already spent (which reads like a credential and
//                             is not one).
//  - one-time, renewal      → `redeem: false`, because stacking time onto an
//                             existing key needs the raw code: only
//                             `/v3/redeem` accepts a `renew_key` target. This
//                             is the one path that still redeems client-side.
//
// This lives in a store (not dialog state) so a purchase survives the dialog
// — or the whole Settings page — unmounting while the buyer is off approving
// in their browser. The claim_secret exists only here, in memory: if the app
// exits mid-purchase the code/key is recovered from the buyer's email (the
// server's durable backstop), and the UI says so.
//
// A paid key must never be lost. When the key arrives, it is delivered to the
// mounted purchase dialog (which routes it through the same onNewKey path as
// the redeem dialog); if no dialog is mounted at that moment it is persisted
// straight into `bot.api.key` on disk as a fallback. Either way the API is
// switched on (`enabled: true`) — buying a key implies wanting to use it.

const POLL_MS = 3000
/** Transient-error backoff cap. 404 (wrong claim) is terminal, never retried. */
const POLL_BACKOFF_MAX_MS = 30_000
/** Client-side cap ≈ the server's 1h retrieval window, plus slack. */
const POLL_DEADLINE_MS = 65 * 60_000

export type PurchasePhase =
  | 'idle'
  | 'creating' // create-order / create-subscription in flight
  | 'approving' // buyer is on PayPal; polling *-result
  | 'redeeming' // client-side redeem in flight: code arrived, /v3/redeem running
  | 'redeem_failed' // /v3/redeem failed — code is safe and shown for retry
  | 'done' // key minted / key extended
  | 'delivered' // retrieval window passed — code/key was emailed
  | 'failed' // terminal: refunded / cancelled / timeout / request error

type StartOpts = {
  baseUrl: string
  /** Proxy URL for the inference server ('' = direct). */
  proxy: string
  product: Product
  /** One-time purchases: stack the bought time onto this existing key. */
  renewKey?: string
}

type PurchaseStore = {
  phase: PurchasePhase
  product: Product | null
  baseUrl: string
  proxy: string
  approveUrl: string | null
  /** One-time: the prepaid redeem code once `ready` (also emailed). */
  code: string | null
  /** The final API key (minted or subscription). Shown once, then saved. */
  key: string | null
  /** Set when a renewal stacked time onto an existing key instead. */
  extendedLast4: string | null
  plan: string | null
  /** New expiry (one-time) or next billing date (subscription). */
  until: string | null
  /** Machine-readable failure: a server status (`refunded`, `cancelled`, …)
   *  or `create` / `claim` / `timeout` / `redeem` for client-side stops. */
  failCode: string | null
  /** Raw error detail to show under the failure message. */
  error: string | null
  /** True when the key was auto-saved to config because no dialog was open. */
  persistedFallback: boolean
  /** True once a mounted dialog has routed the key into its editor. */
  deliveredToUi: boolean
  renewKey: string | null

  start: (opts: StartOpts) => void
  /** From `redeem_failed`: retry `/v3/redeem`, optionally dropping the
   *  renew target to mint a fresh key (a failed redeem consumes nothing). */
  retryRedeem: (renew: boolean) => void
  reopenApproveUrl: () => void
  /** Stop polling and clear the purchase. Only offered by the UI while
   *  approving (nothing paid yet) or from a terminal phase. */
  dismiss: () => void
  /** The mounted dialog's onNewKey sink; null when no dialog is open. */
  setClaimSink: (cb: ((key: string) => void) | null) => void
  markDeliveredToUi: () => void
}

let pollTimer: ReturnType<typeof setTimeout> | null = null
let pollDelay = POLL_MS
let startedAt = 0
/** Guards stale async work after dismiss()/start() resets the machine. */
let generation = 0
let claimSink: ((key: string) => void) | null = null

function clearTimer() {
  if (pollTimer !== null) {
    clearTimeout(pollTimer)
    pollTimer = null
  }
}

const initial = {
  phase: 'idle' as PurchasePhase,
  product: null,
  baseUrl: '',
  proxy: '',
  approveUrl: null,
  code: null,
  key: null,
  extendedLast4: null,
  plan: null,
  until: null,
  failCode: null,
  error: null,
  persistedFallback: false,
  deliveredToUi: false,
  renewKey: null,
}

export const usePurchaseStore = create<PurchaseStore>((set, get) => {
  const fail = (failCode: string, error: string | null) => {
    clearTimer()
    set({ phase: 'failed', failCode, error })
  }

  /** A key exists now — hand it to the open dialog, or save it directly. */
  const deliverKey = (key: string, plan: string | null, until: string | null) => {
    clearTimer()
    set({ phase: 'done', key, plan, until })
    // Toast from the store, not the dialog: it fires exactly once per
    // purchase, even when the buyer closed the dialog during checkout.
    toast.success(i18n.t('bots.api.buy_ok'))
    if (claimSink) {
      claimSink(key)
      set({ deliveredToUi: true })
    } else {
      const cfg = useConfigStore.getState().config
      if (cfg) {
        // `enabled: true` matches the dialog path (adoptNewKey): a bought key
        // should start serving inference without a manual toggle.
        void persistApiConfig({ ...cfg.bot.api, key, enabled: true }).then(
          () => set({ persistedFallback: true }),
          () => {
            /* key stays visible in the store; email is the backstop */
          },
        )
      }
    }
  }

  const redeemCode = async (gen: number) => {
    const s = get()
    const code = s.code
    if (!code) return
    set({ phase: 'redeeming', error: null })
    try {
      // No `email`: the server already emails the purchase to the PayPal
      // payer's address, so there is no separate account email to link here.
      const resp = await invoke<RedeemResponse>('native_api_redeem', {
        baseUrl: s.baseUrl,
        proxy: s.proxy,
        code,
        renewKey: s.renewKey ?? undefined,
      })
      if (gen !== generation) return
      if (resp.key) {
        deliverKey(resp.key, resp.plan || s.plan, resp.expires_at || null)
      } else {
        // Time stacked onto the existing key — nothing new to save.
        clearTimer()
        set({
          phase: 'done',
          extendedLast4: resp.key_last4 || null,
          plan: resp.plan || s.plan,
          until: resp.expires_at || null,
        })
        toast.success(i18n.t('bots.api.buy_ok'))
      }
    } catch (e) {
      if (gen !== generation) return
      // The code was NOT consumed (redeem errors never burn a code) and is
      // still shown in the dialog + emailed; the user can retry from here.
      set({ phase: 'redeem_failed', error: String(e) })
    }
  }

  /** `redeem: true` orders resolve server-side, so the poll has no expiry to
   *  report (unlike `/v3/redeem`, which returns `expires_at`). Ask the server
   *  for it after the fact. Strictly cosmetic: the key is already delivered
   *  and saved by the time this runs, so a failure costs only the date line. */
  const fillExpiry = async (gen: number, key: string) => {
    try {
      const st = await invoke<KeyStatus>('native_api_key_status', {
        provider: 'original',
        baseUrl: get().baseUrl,
        proxy: get().proxy,
        key,
      })
      if (gen !== generation) return
      if (st.expires_at) set({ until: st.expires_at })
    } catch {
      /* the key is in hand; the "valid until" line is optional */
    }
  }

  const handleOrderResult = (gen: number, r: OrderResult) => {
    switch (r.status) {
      case 'pending':
        return schedulePoll(gen)
      case 'ready': {
        set({ plan: r.plan ?? get().plan })
        // A key means the server already redeemed the code (`redeem: true`).
        // Take it and stop: that code is spent, and handing it to /v3/redeem
        // would fail as already-used. Checked before `code` so a server that
        // ever returned both can't make us burn a key we already hold.
        if (r.key) {
          const key = r.key
          deliverKey(key, r.plan ?? get().plan, null)
          void fillExpiry(gen, key)
          return
        }
        // Otherwise the classic flow: a renewal (we asked for the code so we
        // can aim /v3/redeem at `renewKey`), or an older server that ignored
        // the `redeem` flag — both redeem client-side from here.
        if (r.code) {
          set({ code: r.code })
          return void redeemCode(gen)
        }
        return fail('claim', 'server returned ready without a key or code')
      }
      case 'delivered':
        clearTimer()
        return set({ phase: 'delivered' })
      default:
        // refunded — or an unknown future status; both stop the purchase.
        return fail(r.status, null)
    }
  }

  const handleSubscriptionResult = (gen: number, r: SubscriptionResult) => {
    switch (r.status) {
      case 'pending':
        return schedulePoll(gen)
      case 'ready':
        if (!r.key) return fail('claim', 'server returned ready without a key')
        return deliverKey(r.key, r.plan ?? null, r.next_billing ?? null)
      case 'delivered':
        clearTimer()
        return set({ phase: 'delivered' })
      default:
        // cancelled / expired / suspended — or an unknown future status.
        return fail(r.status, null)
    }
  }

  const poll = async (gen: number, ids: { orderId?: string; subscriptionId?: string; claim: string }) => {
    if (gen !== generation || get().phase !== 'approving') return
    if (Date.now() - startedAt > POLL_DEADLINE_MS) {
      // Not paid within the retrieval window. If the buyer DID pay at the
      // last minute, the code/key still reaches them by email.
      return fail('timeout', null)
    }
    try {
      if (ids.orderId) {
        const r = await invoke<OrderResult>('native_api_order_result', {
          baseUrl: get().baseUrl,
          proxy: get().proxy,
          orderId: ids.orderId,
          claim: ids.claim,
        })
        if (gen !== generation) return
        pollDelay = POLL_MS
        handleOrderResult(gen, r)
      } else {
        const r = await invoke<SubscriptionResult>('native_api_subscription_result', {
          baseUrl: get().baseUrl,
          proxy: get().proxy,
          subscriptionId: ids.subscriptionId,
          claim: ids.claim,
        })
        if (gen !== generation) return
        pollDelay = POLL_MS
        handleSubscriptionResult(gen, r)
      }
    } catch (e) {
      if (gen !== generation) return
      const msg = String(e)
      // Wrong/expired claim is 404 and counts toward the server's per-IP
      // failure guard — retrying with the same claim would only trip it.
      if (msg.includes('HTTP 404')) return fail('claim', msg)
      // Anything else (network blip, 429, 503) is transient: poll slower.
      pollDelay = Math.min(pollDelay * 2, POLL_BACKOFF_MAX_MS)
      schedulePoll(gen)
    }
  }

  let activeIds: { orderId?: string; subscriptionId?: string; claim: string } | null = null

  const schedulePoll = (gen: number) => {
    clearTimer()
    pollTimer = setTimeout(() => {
      pollTimer = null
      if (activeIds) void poll(gen, activeIds)
    }, pollDelay)
  }

  return {
    ...initial,

    start: (opts) => {
      const { phase } = get()
      if (phase === 'creating' || phase === 'approving' || phase === 'redeeming') return
      clearTimer()
      generation += 1
      const gen = generation
      pollDelay = POLL_MS
      startedAt = Date.now()
      activeIds = null
      const renewKey = opts.product.kind === 'onetime' ? (opts.renewKey?.trim() || null) : null
      set({
        ...initial,
        phase: 'creating',
        product: opts.product,
        baseUrl: opts.baseUrl,
        proxy: opts.proxy,
        renewKey,
      })
      void (async () => {
        try {
          if (opts.product.kind === 'onetime') {
            const o = await invoke<CreatedOrder>('native_api_create_order', {
              baseUrl: opts.baseUrl,
              proxy: opts.proxy,
              product: opts.product.id,
              // Let the server redeem, unless we need the code itself to aim
              // /v3/redeem at an existing key. Server-side redeem is what puts
              // the KEY in the buyer's backup email instead of a spent code.
              redeem: renewKey === null,
            })
            if (gen !== generation) return
            activeIds = { orderId: o.order_id, claim: o.claim_secret }
            set({ phase: 'approving', approveUrl: o.approve_url })
            openExternal(o.approve_url)
          } else {
            const o = await invoke<CreatedSubscription>('native_api_create_subscription', {
              baseUrl: opts.baseUrl,
              proxy: opts.proxy,
              product: opts.product.id,
            })
            if (gen !== generation) return
            activeIds = { subscriptionId: o.subscription_id, claim: o.claim_secret }
            set({ phase: 'approving', approveUrl: o.approve_url })
            openExternal(o.approve_url)
          }
          schedulePoll(gen)
        } catch (e) {
          if (gen !== generation) return
          fail('create', String(e))
        }
      })()
    },

    retryRedeem: (renew) => {
      if (get().phase !== 'redeem_failed') return
      if (!renew) set({ renewKey: null })
      void redeemCode(generation)
    },

    reopenApproveUrl: () => {
      const url = get().approveUrl
      if (url) openExternal(url)
    },

    dismiss: () => {
      clearTimer()
      generation += 1
      activeIds = null
      set({ ...initial })
    },

    setClaimSink: (cb) => {
      claimSink = cb
    },

    markDeliveredToUi: () => set({ deliveredToUi: true }),
  }
})
