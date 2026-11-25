// Game-history page. Filter bar + PT-rule selector at the top; rank
// pie + cumulative-PT line chart in the middle; detailed stats and the
// game list at the bottom.
//
// All filtering happens in-memory off the records cached in
// `useHistoryStore`. The store is hydrated by `useTauriBridge` on
// startup and kept current by the `history-recorded` Tauri event.

import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { CumulativePtChart } from '@/components/history/CumulativePtChart'
import { GameList } from '@/components/history/GameList'
import { PtRuleSelector } from '@/components/history/PtRuleSelector'
import { RankPieChart } from '@/components/history/RankPieChart'
import { StatsTable } from '@/components/history/StatsTable'
import { aggregateStats } from '@/lib/historyStats'
import { cumulativePtSeries, rankDistribution } from '@/lib/ptCalc'
import { useHistoryStore } from '@/stores/historyStore'
import type { GameRecord, KyokuMode, Platform } from '@/types'

const ANY = '__any__'

export function History() {
  const { t } = useTranslation()
  const records = useHistoryStore((s) => s.records)
  const filter = useHistoryStore((s) => s.filter)
  const setFilter = useHistoryStore((s) => s.setFilter)
  const rule = useHistoryStore((s) => s.rule)

  // 3p / 4p are separate analysis modes — never mix. Defaults to 4p.
  const numPlayers: 3 | 4 = filter.num_players === 3 ? 3 : 4
  const setNumPlayers = (np: 3 | 4) => setFilter({ ...filter, num_players: np })

  const filtered = useMemo(
    () => records.filter((r) => matchFilter(r, filter)),
    [records, filter],
  )

  const series = useMemo(
    () => cumulativePtSeries(filtered, rule),
    [filtered, rule],
  )
  const stats = useMemo(() => aggregateStats(filtered), [filtered])

  const pieCounts = useMemo(
    () => rankDistribution(filtered, numPlayers),
    [filtered, numPlayers],
  )

  return (
    <div className="p-6 flex flex-col gap-6 w-full">
      <header className="flex items-center justify-between gap-4 flex-wrap">
        <div>
          <h1 className="text-2xl font-semibold">{t('history.title')}</h1>
          <p className="text-sm text-muted-foreground">
            {t('history.description')}
          </p>
        </div>
        <Tabs
          value={String(numPlayers)}
          onValueChange={(v) => setNumPlayers(v === '3' ? 3 : 4)}
        >
          <TabsList>
            <TabsTrigger value="4">
              {t('history.filter.num_players_4')}
            </TabsTrigger>
            <TabsTrigger value="3">
              {t('history.filter.num_players_3')}
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </header>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <FilterCard
          filter={filter}
          onChange={setFilter}
        />
        <div className="lg:col-span-2">
          <PtRuleSelector />
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm uppercase tracking-wider">
              {t('history.rank_distribution')}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <RankPieChart counts={pieCounts} />
          </CardContent>
        </Card>
        <Card className="lg:col-span-2">
          <CardHeader>
            <CardTitle className="text-sm uppercase tracking-wider">
              {t('history.cumulative_pt')}
            </CardTitle>
          </CardHeader>
          <CardContent>
            <CumulativePtChart series={series} />
          </CardContent>
        </Card>
      </div>

      <StatsTable stats={stats} />

      <GameList records={filtered} rule={rule} />
    </div>
  )
}

function matchFilter(
  r: GameRecord,
  f: ReturnType<typeof useHistoryStore.getState>['filter'],
): boolean {
  if (f.platform && r.platform !== f.platform) return false
  if (f.num_players && r.num_players !== f.num_players) return false
  if (f.kyoku_mode && r.kyoku_mode !== f.kyoku_mode) return false
  if (f.started_after && new Date(r.started_at) < new Date(f.started_after))
    return false
  if (f.started_before && new Date(r.started_at) >= new Date(f.started_before))
    return false
  return true
}

function FilterCard({
  filter,
  onChange,
}: {
  filter: ReturnType<typeof useHistoryStore.getState>['filter']
  onChange: (f: ReturnType<typeof useHistoryStore.getState>['filter']) => void
}) {
  const { t } = useTranslation()

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm uppercase tracking-wider">{t('history.filters_card_title')}</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          <Field label={t('history.filter.platform')}>
            <Select
              value={filter.platform ?? ANY}
              onValueChange={(v) =>
                onChange({
                  ...filter,
                  platform: v === ANY ? undefined : (v as Platform),
                })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ANY}>{t('history.filter.any')}</SelectItem>
                <SelectItem value="majsoul">{t('platform.majsoul')}</SelectItem>
                <SelectItem value="tenhou">{t('platform.tenhou')}</SelectItem>
                <SelectItem value="riichi_city">{t('platform.riichi_city')}</SelectItem>
                <SelectItem value="holo_mahjong">{t('platform.holomahjong')}</SelectItem>
                <SelectItem value="mjai">{t('platform.mjai')}</SelectItem>
              </SelectContent>
            </Select>
          </Field>

          <Field label={t('history.filter.kyoku_mode')}>
            <Select
              value={filter.kyoku_mode ?? ANY}
              onValueChange={(v) =>
                onChange({
                  ...filter,
                  kyoku_mode: v === ANY ? undefined : (v as KyokuMode),
                })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={ANY}>{t('history.filter.any')}</SelectItem>
                <SelectItem value="east_only">{t('history.filter.east_only')}</SelectItem>
                <SelectItem value="east_south">{t('history.filter.east_south')}</SelectItem>
                <SelectItem value="other">{t('history.filter.other')}</SelectItem>
              </SelectContent>
            </Select>
          </Field>

          <Field label={t('history.date_label')}>
            <div className="flex gap-1">
              <Input
                type="date"
                value={dateOnly(filter.started_after)}
                onChange={(e) =>
                  onChange({
                    ...filter,
                    started_after: e.target.value
                      ? new Date(e.target.value).toISOString()
                      : undefined,
                  })
                }
              />
              <Input
                type="date"
                value={dateOnly(filter.started_before)}
                onChange={(e) =>
                  onChange({
                    ...filter,
                    started_before: e.target.value
                      ? new Date(e.target.value).toISOString()
                      : undefined,
                  })
                }
              />
            </div>
          </Field>
        </div>
      </CardContent>
    </Card>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <Label className="text-xs">{label}</Label>
      {children}
    </div>
  )
}

function dateOnly(iso?: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.valueOf())) return ''
  return d.toISOString().slice(0, 10)
}
