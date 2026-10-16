import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { installPageUrl } from '@/lib/captureConfig'
import { copyText } from '@/lib/clipboard'
import { openExternal } from '@/lib/external'
import { invoke } from '@/lib/tauri'

// How long the address must stay unchanged before the preview moves to it,
// so typing in the address field does not bind every intermediate value.
const PREVIEW_DEBOUNCE_MS = 300

// How to get the Akagi userscript into the user's own browser. Shared by
// the Setup wizard and Settings → Capture.
//
// "Install" opens Akagi's install page in the default browser: it links the
// script (which a userscript manager intercepts) and tells a browser
// without a manager what is missing — opening the script directly would
// just show a page of code there. The running ingest backend serves that
// page — but it only runs once the configuration is saved, and the moment
// someone picks this mode (Setup, or an unsaved Settings change) is when
// they want to install. So until the backend is live on `addr`, the panel
// keeps a preview server there that serves only the page and the script; the backend
// takes the address over when it starts. "Copy script" renders the script
// locally and always works.
export function IngestUserscriptPanel({
  addr,
  endpointLive,
}: {
  addr: string
  /** The ingest backend is running and listening on `addr`. */
  endpointLive: boolean
}) {
  const { t, i18n } = useTranslation()
  const [copying, setCopying] = useState(false)
  // Which address the preview is serving, or why it could not.
  const [served, setServed] = useState<string | null>(null)
  const [failure, setFailure] = useState<{ addr: string; error: string } | null>(null)
  const target = addr.trim()
  const url = installPageUrl(target, i18n.language)

  useEffect(() => {
    if (endpointLive) return
    let cancelled = false
    const timer = setTimeout(() => {
      invoke('start_userscript_preview', { addr: target })
        .then(() => {
          if (!cancelled) setServed(target)
        })
        .catch((e) => {
          if (!cancelled) setFailure({ addr: target, error: String(e) })
        })
    }, PREVIEW_DEBOUNCE_MS)
    return () => {
      cancelled = true
      clearTimeout(timer)
      invoke('stop_userscript_preview', { addr: target }).catch(() => {})
    }
  }, [target, endpointLive])

  const installable = endpointLive || served === target
  const previewError = !endpointLive && failure?.addr === target ? failure.error : null

  const copy = async () => {
    setCopying(true)
    try {
      const script = await invoke<string>('ingest_userscript', { addr: target })
      if (await copyText(script)) toast.success(t('ingest.copied'))
      else toast.error(t('ingest.copy_failed'))
    } catch (e) {
      toast.error(t('ingest.copy_failed'), { description: String(e) })
    } finally {
      setCopying(false)
    }
  }

  return (
    <div className="grid gap-3 rounded-md border p-3 text-sm">
      <ol className="list-decimal space-y-1 pl-5 text-muted-foreground">
        <li>{t('ingest.step_manager')}</li>
        <li>{t('ingest.step_allow')}</li>
        <li>{t('ingest.step_install')}</li>
        <li>{t('ingest.step_permission')}</li>
      </ol>
      <code className="break-all rounded bg-muted px-2 py-1 font-mono text-xs">{url}</code>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" onClick={() => openExternal(url)} disabled={!installable}>
          {t('ingest.install')}
        </Button>
        <Button size="sm" variant="outline" onClick={copy} disabled={copying}>
          {t('ingest.copy')}
        </Button>
      </div>
      {previewError && (
        <p className="text-xs text-destructive">
          {t('ingest.preview_failed', { error: previewError })}
        </p>
      )}
      <p className="text-xs text-muted-foreground">{t('ingest.no_autoplay')}</p>
    </div>
  )
}
