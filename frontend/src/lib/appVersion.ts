import { getVersion } from '@tauri-apps/api/app'
import { HAS_TAURI } from '@/lib/tauri'

// Fallback used in the browser dev preview (no Tauri runtime → no
// app.getVersion). Lines up with Cargo.toml so devs see a sensible
// string until the real version resolves. The release tagging script
// rewrites this line on every release — keep the exact
// `const VERSION_FALLBACK = '…'` shape it greps for.
export const VERSION_FALLBACK = '3.7.1'

/** Resolve the running app version, falling back for browser previews. */
export async function getAppVersion(): Promise<string> {
  if (!HAS_TAURI) return VERSION_FALLBACK
  try {
    return await getVersion()
  } catch {
    return VERSION_FALLBACK
  }
}

/**
 * Compare two Akagi version strings (`3.5.0`, and the legacy `3.0.0-8`
 * beta shape) segment-wise: split on `.` and `-`, compare numerically,
 * treat missing segments as 0. Returns <0 / 0 / >0 like a comparator.
 * Non-numeric segments (never produced by tag_release.sh) fall back to
 * string comparison so the order is still total.
 */
export function compareVersions(a: string, b: string): number {
  const as = a.split(/[.-]/)
  const bs = b.split(/[.-]/)
  const len = Math.max(as.length, bs.length)
  for (let i = 0; i < len; i++) {
    const ra = as[i] ?? '0'
    const rb = bs[i] ?? '0'
    const na = Number(ra)
    const nb = Number(rb)
    if (Number.isFinite(na) && Number.isFinite(nb)) {
      if (na !== nb) return na < nb ? -1 : 1
    } else if (ra !== rb) {
      return ra < rb ? -1 : 1
    }
  }
  return 0
}
