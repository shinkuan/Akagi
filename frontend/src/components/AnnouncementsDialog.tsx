import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Megaphone } from 'lucide-react'

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { type AnnouncementEntry } from '@/announcements/entries'
import { useAnnouncementStore } from '@/stores/announcementStore'
import { AKAGI_GITHUB_URL, openExternal } from '@/lib/external'

// Delay so the announcements don't pop over the very first paint; keeps
// them clear of the UpdateNotifier toast (3s), which stacks fine next to
// a dialog.
const OPEN_DELAY_MS = 1500

/**
 * In-app announcements dialog: one collapsible row per entry (release
 * or product news), newest first, the newest expanded. Two ways in:
 *  - launch: after an update (or on a fresh install) it lists every
 *    bundled entry the user hasn't seen yet — skip-level updates replay
 *    all the versions in between. Any close records the newest shown
 *    entry as seen, so it shows once per new announcement.
 *  - history: Settings → Updates → "Announcements" re-opens the full list.
 */
export function AnnouncementsDialog() {
  const { t } = useTranslation()
  const open = useAnnouncementStore((s) => s.open)
  const entries = useAnnouncementStore((s) => s.entries)
  const close = useAnnouncementStore((s) => s.close)
  // One-shot per app launch (the component remounts on route changes but
  // the store's armed/seen state is global, so the ref is just belt and
  // braces against double-arming the timer under StrictMode).
  const fired = useRef(false)
  const gotIt = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (fired.current) return
    fired.current = true
    if (!useAnnouncementStore.getState().prepareLaunch()) return
    const timer = window.setTimeout(() => {
      useAnnouncementStore.getState().showLaunch()
    }, OPEN_DELAY_MS)
    return () => window.clearTimeout(timer)
  }, [])

  return (
    <Dialog open={open} onOpenChange={(v) => !v && close()}>
      {/* Wider than the standard dialogs to fit screenshots and two-column
          feature grids; the middle grid row scrolls so the dialog itself
          never grows past the window. */}
      <DialogContent
        className="sm:max-w-4xl max-h-[85vh] grid-rows-[auto_minmax(0,1fr)_auto]"
        // Focus "Got it" rather than the first row: a row focused on open
        // shows a focus frame, as if it had been selected.
        onOpenAutoFocus={(e) => {
          e.preventDefault()
          gotIt.current?.focus()
        }}
      >
        <DialogHeader>
          <DialogTitle>{t('announcements.dialog.title')}</DialogTitle>
          <DialogDescription>{t('announcements.dialog.intro')}</DialogDescription>
        </DialogHeader>

        {/* Keyed by the shown id set: a different selection (launch unseen
            vs full history) remounts the list, resetting expansion to
            "newest only" without effect-driven state syncing. */}
        <AnnouncementList key={entries.map((e) => e.id).join('|')} entries={entries} />

        <DialogFooter className="bg-transparent p-0 border-0 mx-0 mb-0 flex-wrap gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => openExternal(`${AKAGI_GITHUB_URL}/releases`)}
          >
            {t('announcements.dialog.all_releases')}
          </Button>
          <Button ref={gotIt} size="sm" onClick={close}>
            {t('announcements.dialog.got_it')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function AnnouncementList({ entries }: { entries: AnnouncementEntry[] }) {
  // Explicit toggles; the newest shown entry starts expanded.
  const [expanded, setExpanded] = useState<Record<string, boolean>>(() =>
    entries[0] ? { [entries[0].id]: true } : {},
  )
  return (
    <ul className="grid gap-2 content-start overflow-y-auto min-h-0 pr-1">
      {entries.map((entry) => (
        <AnnouncementRow
          key={entry.id}
          entry={entry}
          expanded={!!expanded[entry.id]}
          onToggle={() =>
            setExpanded((prev) => ({ ...prev, [entry.id]: !prev[entry.id] }))
          }
        />
      ))}
    </ul>
  )
}

function AnnouncementRow({
  entry,
  expanded,
  onToggle,
}: {
  entry: AnnouncementEntry
  expanded: boolean
  onToggle: () => void
}) {
  const { t, i18n } = useTranslation()
  // The ISO date renders in the viewer's locale; parse as local midnight
  // (plain YYYY-MM-DD strings would otherwise be treated as UTC and can
  // display one day off west of Greenwich).
  const date = new Date(`${entry.date}T00:00:00`).toLocaleDateString(i18n.language)
  return (
    // Surfaces are mixed from the foreground rather than taken from the
    // theme's muted / border colours: in the dark themes those sit within a
    // few percent of the dialog's own background, so rows, badges and icon
    // tiles would not stand out from it.
    <li className="rounded-lg border border-foreground/15 bg-foreground/[0.05]">
      <button
        type="button"
        aria-expanded={expanded}
        onClick={onToggle}
        className="flex w-full items-center gap-3 rounded-lg p-3 text-left outline-none hover:bg-foreground/[0.06] focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
      >
        {entry.version ? (
          <span className="shrink-0 rounded-md bg-foreground/10 px-1.5 py-0.5 font-mono text-xs font-semibold">
            v{entry.version}
          </span>
        ) : (
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-foreground/10">
            <Megaphone className="size-3.5" />
          </span>
        )}
        <span className="min-w-0 flex-1 truncate text-sm font-medium">
          {t(`announcements.entries.${entry.id}.title`)}
        </span>
        <span className="shrink-0 text-xs text-muted-foreground">{date}</span>
        <ChevronDown
          className={cn(
            'size-4 shrink-0 text-muted-foreground transition-transform',
            expanded && 'rotate-180',
          )}
        />
      </button>

      {expanded && (
        <div className="grid gap-4 border-t border-foreground/10 p-3">
          {entry.image && (
            <img
              src={entry.image}
              alt={t(`announcements.entries.${entry.id}.image_alt`)}
              className="w-full rounded-md border border-foreground/10"
            />
          )}
          <ul className="grid gap-3 sm:grid-cols-2">
            {entry.features.map((f) => {
              const Icon = f.icon
              return (
                <li key={f.key} className="flex items-start gap-3">
                  <span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-md bg-foreground/10 text-foreground">
                    <Icon className="size-4" />
                  </span>
                  <div className="min-w-0">
                    <div className="text-sm font-medium">
                      {t(`announcements.entries.${entry.id}.${f.key}_title`)}
                    </div>
                    <div className="text-xs text-muted-foreground">
                      {t(`announcements.entries.${entry.id}.${f.key}_desc`)}
                    </div>
                  </div>
                </li>
              )
            })}
          </ul>
          {entry.link && (
            <Button
              size="sm"
              className="justify-self-start"
              onClick={() => openExternal(entry.link!)}
            >
              {t(`announcements.entries.${entry.id}.link_label`)}
            </Button>
          )}
        </div>
      )}
    </li>
  )
}
