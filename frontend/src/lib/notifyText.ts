import i18n from '@/i18n'
import type { Notification } from '@/types'

// Notifications carry both a resolved English `title`/`body` (backend fallback)
// and optional i18n keys. When the frontend i18n knows the key, render the
// localized string with the backend-supplied interpolation `args`; otherwise
// fall back to the English text. Resolving at render time (not on arrival) means
// switching languages re-translates history notifications too.

export function notifyTitle(n: Notification): string {
  const key = n.title_key
  if (key && i18n.exists(key, n.args)) return i18n.t(key, n.args)
  return n.title
}

export function notifyBody(n: Notification): string | undefined {
  const key = n.body_key
  if (key && i18n.exists(key, n.args)) return i18n.t(key, n.args)
  return n.body
}
