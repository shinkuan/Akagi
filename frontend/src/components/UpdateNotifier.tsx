import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { selectHasNotifiableUpdate, useUpdaterStore } from '@/stores/updaterStore'
import { HAS_TAURI } from '@/lib/tauri'
import { UpdateDialog } from '@/components/UpdateDialog'

/// Invisible coordinator. Mount it once near the root of the tree.
///
/// Responsibilities:
///   1. Kick off a (cached) `check_for_update` ~3 seconds after mount,
///      but only when the user has auto-check enabled and we aren't
///      already inside the 6-hour throttle window.
///   2. When a new, non-skipped update arrives, fire a single sonner
///      toast per session with a "View details" action that opens the
///      `<UpdateDialog />`.
///   3. Host the `<UpdateDialog />` so it can be opened from anywhere.
export function UpdateNotifier() {
  const { t } = useTranslation()
  const autoCheckEnabled = useUpdaterStore((s) => s.autoCheckEnabled)
  const checkNow = useUpdaterStore((s) => s.checkNow)
  const openDialog = useUpdaterStore((s) => s.openDialog)
  const markToastShown = useUpdaterStore((s) => s.markToastShown)
  const toastShown = useUpdaterStore((s) => s.toastShownThisSession)
  const pending = useUpdaterStore((s) => s.pendingUpdate)
  const hasNotifiable = useUpdaterStore(selectHasNotifiableUpdate)

  // 3s deferred auto-check on launch — keeps the startup path cold-cache-fast
  // and gives the rest of the app time to mount before we render a toast.
  useEffect(() => {
    if (!HAS_TAURI || !autoCheckEnabled) return
    const id = window.setTimeout(() => {
      void checkNow(false)
    }, 3000)
    return () => window.clearTimeout(id)
    // checkNow is stable (zustand action) — listing it would just churn.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoCheckEnabled])

  // Fire the toast exactly once per app session, when a notifiable
  // pending update appears. `markToastShown` guards against re-firing
  // after route changes that re-mount this component.
  useEffect(() => {
    if (!hasNotifiable || !pending || toastShown) return
    toast.info(t('updates.toast.title', { version: pending.latest_version }), {
      duration: 10_000,
      action: {
        label: t('updates.toast.action'),
        onClick: openDialog,
      },
    })
    markToastShown()
    // openDialog/markToastShown are stable zustand actions.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasNotifiable, pending, toastShown, t])

  return <UpdateDialog />
}
