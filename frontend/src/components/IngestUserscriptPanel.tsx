import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { userscriptUrl } from '@/lib/captureConfig'
import { copyText } from '@/lib/clipboard'
import { openExternal } from '@/lib/external'
import { invoke } from '@/lib/tauri'

// How to get the Akagi userscript into the user's own browser. Shared by
// the Setup wizard and Settings → Capture.
//
// "Install" opens the script's URL in the default browser, which a
// userscript manager intercepts — but that URL is served by the running
// ingest backend, so it only works once ingest capture is running on
// `addr`. "Copy script" renders the script locally and always works, for a
// browser that is not the default one or before the backend is up.
export function IngestUserscriptPanel({
  addr,
  endpointLive,
}: {
  addr: string
  /** The ingest backend is running and listening on `addr`. */
  endpointLive: boolean
}) {
  const { t } = useTranslation()
  const [copying, setCopying] = useState(false)
  const url = userscriptUrl(addr)

  const copy = async () => {
    setCopying(true)
    try {
      const script = await invoke<string>('ingest_userscript', { addr })
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
      </ol>
      <code className="break-all rounded bg-muted px-2 py-1 font-mono text-xs">{url}</code>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" onClick={() => openExternal(url)} disabled={!endpointLive}>
          {t('ingest.install')}
        </Button>
        <Button size="sm" variant="outline" onClick={copy} disabled={copying}>
          {t('ingest.copy')}
        </Button>
      </div>
      {!endpointLive && (
        <p className="text-xs text-muted-foreground">{t('ingest.install_needs_running')}</p>
      )}
      <p className="text-xs text-muted-foreground">{t('ingest.no_autoplay')}</p>
    </div>
  )
}
