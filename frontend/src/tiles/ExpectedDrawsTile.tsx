import { useTranslation } from 'react-i18next'
import { TileFrame } from '@/components/TileFrame'
import { Mahgen } from '@/components/Mahgen'
import { useAnalysisStore } from '@/stores/analysisStore'
import { useNotifyStore } from '@/stores/notifyStore'
import { fmtScore, pct } from '@/lib/format'
import { mjaiToMahgen } from '@/lib/tileIdx'
import type { Hand13Result, Hand14Result } from '@/types'
import type { Breakpoint } from '@/tiles/defaults'

type Row = {
  key: string
  label: string
  discard: string | null
  result: Hand13Result
}

export function ExpectedDrawsTile({ bp }: { bp: Breakpoint }) {
  const { t, i18n } = useTranslation()
  const result = useAnalysisStore((s) => s.result)
  const latestBotDiscard = useNotifyStore(latestDiscardFromResponses)
  const rows = rowsFromResult(
    result?.hand14,
    result?.hand13,
    latestBotDiscard,
    t('tile.expected_draws_ai'),
  )
  const shanten = result?.shanten
  const preferEnglish = i18n.resolvedLanguage?.startsWith('en') || i18n.language?.startsWith('en')

  return (
    <TileFrame
      id="expected-draws"
      title={t('tile.expected_draws')}
      bp={bp}
      rightSlot={
        shanten != null && (
          <span className="rounded-full border border-border px-2 py-0.5 text-[10px] tracking-wider uppercase">
            {t('mahjong.shanten_value', { n: shanten })}
          </span>
        )
      }
      contentClassName="flex flex-col gap-2"
    >
      {rows.length === 0 ? (
        <span className="text-muted-foreground text-sm">{t('tile.expected_draws_empty')}</span>
      ) : (
        rows.map((row) => (
          <section key={row.key} className="rounded-md border border-border bg-muted/20 p-2">
            <div className="flex items-center justify-between gap-2">
              <div className="flex min-w-0 items-center gap-2">
                <span className="w-5 shrink-0 text-xs font-mono text-muted-foreground">
                  {row.label}
                </span>
                {row.discard ? (
                  <>
                    <span className="text-[10px] uppercase tracking-wide text-muted-foreground">
                      {t('tile.expected_draws_after_discard')}
                    </span>
                    <Mahgen seq={mjaiToMahgen([row.discard])} kind="rec" />
                  </>
                ) : (
                  <span className="text-xs font-medium">{t('tile.expected_draws_current')}</span>
                )}
              </div>
              <span className="shrink-0 text-[10px] font-mono text-muted-foreground">
                {t('tile.expected_draws_ev')}: {row.result.mixed_round_point.toFixed(0)}
              </span>
            </div>

            <div className="mt-2 grid grid-cols-3 gap-2 text-[10px]">
              <Stat label={t('tile.expected_draws_waits')} value={`${row.result.waits_total}${t('tile.expected_draws_left_unit')}`} />
              <Stat label={t('tile.expected_draws_agari')} value={pct(row.result.avg_agari_rate)} />
              <Stat label={t('tile.expected_draws_score')} value={scoreLabel(row.result, t)} />
            </div>

            <WaitList waits={row.result.waits} />
            <YakuList result={row.result} preferEnglish={preferEnglish} empty={t('tile.expected_draws_no_yaku')} />
          </section>
        ))
      )}
    </TileFrame>
  )
}

function rowsFromResult(
  hand14: Hand14Result | null | undefined,
  hand13: Hand13Result | null | undefined,
  botDiscard: string | null,
  aiLabel: string,
): Row[] {
  if (hand14?.maintain.length) {
    const allCandidates = [...hand14.maintain, ...hand14.backwards]
    const botCandidate = botDiscard
      ? allCandidates.find((candidate) => candidate.discard === botDiscard)
      : null
    const candidates = botCandidate
      ? [
          botCandidate,
          ...hand14.maintain
            .filter((candidate) => candidate.discard !== botCandidate.discard)
            .slice(0, 2),
        ]
      : hand14.maintain.slice(0, 3)

    return candidates.map((candidate, index) => ({
      key: `${candidate.discard}-${index}`,
      label: botCandidate && index === 0 ? aiLabel : String(index + 1),
      discard: candidate.discard,
      result: candidate.result,
    }))
  }
  if (hand13) {
    return [{ key: 'current', label: '—', discard: null, result: hand13 }]
  }
  return []
}

function latestDiscardFromResponses(s: ReturnType<typeof useNotifyStore.getState>): string | null {
  for (let i = s.responses.length - 1; i >= 0; i--) {
    const response = s.responses[i]
    if (response.type === 'dahai') return response.pai
    if (response.type === 'reach' && response.pai) return response.pai
  }
  return null
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded border border-border bg-background/40 px-2 py-1">
      <div className="truncate text-muted-foreground">{label}</div>
      <div className="truncate font-mono text-xs">{value}</div>
    </div>
  )
}

function WaitList({ waits }: { waits: Hand13Result['waits'] }) {
  const topWaits = [...waits]
    .sort((a, b) => b.left - a.left || (b.agari_rate ?? 0) - (a.agari_rate ?? 0))
    .slice(0, 8)

  if (topWaits.length === 0) return null

  return (
    <div className="mt-2 flex flex-wrap gap-1.5">
      {topWaits.map((wait) => (
        <span key={wait.tile} className="inline-flex items-center gap-1 rounded border border-border bg-background/50 px-1.5 py-1">
          <Mahgen seq={mjaiToMahgen([wait.tile])} kind="bot-show" />
          <span className="text-[10px] font-mono text-muted-foreground">×{wait.left}</span>
          {wait.agari_rate != null && (
            <span className="text-[10px] font-mono">{pct(wait.agari_rate)}</span>
          )}
        </span>
      ))}
    </div>
  )
}

function YakuList({
  result,
  preferEnglish,
  empty,
}: {
  result: Hand13Result
  preferEnglish: boolean
  empty: string
}) {
  const names = preferEnglish ? result.yaku_names_en : result.yaku_names
  const shown = names.length ? names : result.yaku_ids.map((id) => `Yaku #${id}`)

  return (
    <div className="mt-2 text-[10px] leading-snug text-muted-foreground">
      {shown.length ? shown.join(' / ') : empty}
    </div>
  )
}

function scoreLabel(result: Hand13Result, t: (key: string, options?: Record<string, unknown>) => string): string {
  const dama = result.dama_point > 0 ? fmtScore(Math.round(result.dama_point)) : '—'
  const riichi = result.riichi_point > 0 ? fmtScore(Math.round(result.riichi_point)) : '—'
  return `${t('tile.expected_draws_dama')}${dama} / ${t('tile.expected_draws_riichi')}${riichi}`
}
