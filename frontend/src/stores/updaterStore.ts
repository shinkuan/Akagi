import { create } from 'zustand'
import { persist, createJSONStorage } from 'zustand/middleware'
import { invoke } from '@/lib/tauri'

/// Mirrors `crate::updater::check::UpdateInfo` on the Rust side. The
/// shape is fixed by the IPC contract — keep both ends in sync.
export type UpdateInfo = {
  current: string
  latest_tag: string
  latest_version: string
  body: string
  html_url: string
  asset_name: string
  asset_url: string
  asset_size: number
  asset_digest_sha256: string | null
}

/// Mirrors `crate::updater::error::UpdateError`. The `kind` tag is what
/// frontend code switches on to decide between a generic "Update failed"
/// toast and a fallback that opens the release page.
export type UpdateError =
  | { kind: 'unsupported_platform' }
  | { kind: 'read_only_install'; path: string }
  | { kind: 'digest_mismatch' }
  | { kind: 'no_matching_asset' }
  | { kind: 'other'; message: string }

const CHECK_CACHE_MS = 6 * 60 * 60 * 1000

/// Frontend-only persisted state. The Rust side intentionally has no
/// knowledge of `skippedVersion` / `autoCheckEnabled` — they're user
/// preferences and don't gate any backend behavior.
type Persisted = {
  lastChecked: number | null
  skippedVersion: string | null
  autoCheckEnabled: boolean
}

type Ephemeral = {
  /// Result of the most recent `check_for_update`. NOT persisted — we
  /// always re-fetch on launch (subject to the 6h cache) so a release
  /// that was deleted / unpublished doesn't keep nagging.
  pendingUpdate: UpdateInfo | null
  /// `true` while `apply_update` is in flight. Used to disable the
  /// "Update now" button and show a spinner.
  applying: boolean
  /// `true` after the auto-check toast has fired this session. Stops
  /// the toast from re-firing when `<UpdateNotifier />` re-mounts on
  /// a route change.
  toastShownThisSession: boolean
  /// Open state for the `<UpdateDialog />`. The sidebar red-dot and the
  /// toast's "View details" action both flip this to true.
  dialogOpen: boolean
  /// `true` while a `check_for_update` call is in flight. Used by the
  /// Settings page to disable the button + show a spinner.
  checking: boolean
}

type Actions = {
  checkNow: (force?: boolean) => Promise<void>
  applyUpdate: () => Promise<UpdateError | null>
  skip: (version: string) => void
  clearSkipped: () => void
  setAutoCheckEnabled: (enabled: boolean) => void
  openDialog: () => void
  closeDialog: () => void
  markToastShown: () => void
}

export type UpdaterStore = Persisted & Ephemeral & Actions

const initialEphemeral: Ephemeral = {
  pendingUpdate: null,
  applying: false,
  toastShownThisSession: false,
  dialogOpen: false,
  checking: false,
}

export const useUpdaterStore = create<UpdaterStore>()(
  persist(
    (set, get) => ({
      lastChecked: null,
      skippedVersion: null,
      autoCheckEnabled: true,
      ...initialEphemeral,

      checkNow: async (force = false) => {
        if (get().checking) return
        if (!force) {
          const last = get().lastChecked ?? 0
          if (Date.now() - last < CHECK_CACHE_MS) return
        }
        set({ checking: true })
        try {
          const info = await invoke<UpdateInfo | null>('check_for_update')
          set({ pendingUpdate: info, lastChecked: Date.now() })
        } catch (e) {
          // Network / parse failures: just log; the toast layer will
          // see `pendingUpdate === null` and stay quiet. Settings card
          // shows the error via its own try/catch wrapper.
          console.warn('check_for_update failed:', e)
        } finally {
          set({ checking: false })
        }
      },

      applyUpdate: async () => {
        const info = get().pendingUpdate
        if (!info || get().applying) return null
        set({ applying: true })
        try {
          await invoke<void>('apply_update', { info })
          // Unreachable on success — the backend calls
          // `AppHandle::restart` after a successful swap, which exits
          // the current process.
          return null
        } catch (e) {
          set({ applying: false })
          // Tauri serialises the `Result<_, UpdateError>` Err variant as
          // a JS object — but very old failure paths (e.g. command not
          // registered) might come back as a string. Normalise.
          if (typeof e === 'object' && e !== null && 'kind' in e) {
            return e as UpdateError
          }
          return { kind: 'other', message: String(e) }
        }
      },

      skip: (version: string) =>
        set({ skippedVersion: version, dialogOpen: false }),
      clearSkipped: () => set({ skippedVersion: null }),
      setAutoCheckEnabled: (enabled: boolean) =>
        set({ autoCheckEnabled: enabled }),
      openDialog: () => set({ dialogOpen: true }),
      closeDialog: () => set({ dialogOpen: false }),
      markToastShown: () => set({ toastShownThisSession: true }),
    }),
    {
      name: 'akagi.updater',
      storage: createJSONStorage(() => localStorage),
      partialize: (state): Persisted => ({
        lastChecked: state.lastChecked,
        skippedVersion: state.skippedVersion,
        autoCheckEnabled: state.autoCheckEnabled,
      }),
    },
  ),
)

/// Derived: is there a notifiable update right now? Notifiable means
/// "we have a pending update AND it isn't the one the user explicitly
/// skipped." Used by the sidebar red-dot and the toast firing logic.
export function selectHasNotifiableUpdate(s: UpdaterStore): boolean {
  if (!s.pendingUpdate) return false
  return s.skippedVersion !== s.pendingUpdate.latest_tag
}
