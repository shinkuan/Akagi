import { useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { TileFrame } from '@/components/TileFrame'
import { Mahgen } from '@/components/Mahgen'
import { useNotifyStore } from '@/stores/notifyStore'
import { useBotHoraScore } from '@/lib/botHoraScore'
import { mjaiToMahgen } from '@/lib/tileIdx'
import type { Breakpoint } from '@/tiles/defaults'
import type { BotResponse } from '@/types'

import chiGlyph from '@/assets/glyphs/one_word/chi.png'
import ponGlyph from '@/assets/glyphs/one_word/pon.png'
import kanGlyph from '@/assets/glyphs/one_word/kan.png'
import ronGlyph from '@/assets/glyphs/one_word/ron.png'
import noneGlyph from '@/assets/glyphs/one_word/none.png'
import dahaiGlyph from '@/assets/glyphs/one_word/dahai.png'
import tsumoGlyph from '@/assets/glyphs/two_word/tsumo.png'
import reachGlyph from '@/assets/glyphs/two_word/reach.png'
import kitaGlyph from '@/assets/glyphs/two_word/kita.png'
import ryukyokuGlyph from '@/assets/glyphs/two_word/ryukyoku.png'

const DECISION_TYPES = new Set<BotResponse['type']>([
  'dahai', 'chi', 'pon', 'ankan', 'daiminkan', 'kakan',
  'reach', 'hora', 'ryukyoku', 'kita', 'none',
])

type Variant = {
  glyph: string
  /** Translucent background tint. Null disables the tinted backdrop. */
  color: string | null
  /** Tint for the left calligraphy glyph (a black-on-transparent PNG). Dahai = white. */
  glyphColor: string
  /** i18n key for the action label (under `mahjong.*`). */
  labelKey: string
  /** Pre-formatted info text. For hora this stays empty here — the score
   *  hook fills it in once the backend returns. */
  extra: string
  /** mahgen DSL string for the right-hand tile area. Empty = no tile column. */
  mahgen: string
}

// Action colours mirror reference/Akagi/akagi/client.tcss so the HUD matches
// the original Akagi TUI palette. Applied as a faint translucent fill.
function describe(r: BotResponse, t: (k: string, opts?: Record<string, unknown>) => string): Variant | null {
  switch (r.type) {
    case 'dahai':
      return {
        glyph: dahaiGlyph,
        color: null,
        glyphColor: '#ffffff',
        labelKey: 'mahjong.dahai',
        extra: '',
        mahgen: mjaiToMahgen([r.pai]),
      }
    case 'chi':
      return {
        glyph: chiGlyph,
        color: '#00ff80',
        glyphColor: '#00ff80',
        labelKey: 'mahjong.chi',
        extra: `${r.pai}|${r.consumed.join('')}`,
        mahgen: `${mjaiToMahgen([r.pai])}|${mjaiToMahgen([...r.consumed])}`,
      }
    case 'pon':
      return {
        glyph: ponGlyph,
        color: '#007fff',
        glyphColor: '#39a8ff',
        labelKey: 'mahjong.pon',
        extra: r.pai,
        mahgen: mjaiToMahgen([r.pai]),
      }
    case 'daiminkan':
      return {
        glyph: kanGlyph,
        color: '#9a1cbd',
        glyphColor: '#c859e6',
        labelKey: 'mahjong.daiminkan',
        extra: r.pai,
        mahgen: mjaiToMahgen([r.pai]),
      }
    case 'kakan':
      return {
        glyph: kanGlyph,
        color: '#9a1cbd',
        glyphColor: '#c859e6',
        labelKey: 'mahjong.kakan',
        extra: r.pai,
        mahgen: mjaiToMahgen([r.pai]),
      }
    case 'ankan':
      return {
        glyph: kanGlyph,
        color: '#9a1cbd',
        glyphColor: '#c859e6',
        labelKey: 'mahjong.ankan',
        extra: r.consumed[0],
        mahgen: mjaiToMahgen([r.consumed[0]]),
      }
    case 'reach':
      // The Mortal bot wrapper peeks the post-reach dahai by replaying
      // the event log into a throwaway Bot and reading off its next
      // action; when that succeeds, `pai` carries the predicted
      // riichi-discard tile so the HUD can render mahgen up-front
      // (Majsoul fuses declaring + discarding into one click). Bridge-
      // emitted reach events leave `pai` undefined and we fall back to
      // glyph + label only.
      return {
        glyph: reachGlyph,
        color: '#e06c20',
        glyphColor: '#e06c20',
        labelKey: 'mahjong.reach',
        extra: r.pai ?? '',
        mahgen: r.pai ? mjaiToMahgen([r.pai]) : '',
      }
    case 'hora':
      return r.actor === r.target
        ? {
            glyph: tsumoGlyph,
            color: '#ff1493',
            glyphColor: '#ff1493',
            labelKey: 'mahjong.tsumo',
            extra: '',
            mahgen: '',
          }
        : {
            glyph: ronGlyph,
            color: '#c13535',
            glyphColor: '#e05050',
            labelKey: 'mahjong.ron',
            extra: '',
            mahgen: '',
          }
    case 'ryukyoku':
      return {
        glyph: ryukyokuGlyph,
        color: '#8574a1',
        glyphColor: '#bab1ca',
        labelKey: 'mahjong.ryukyoku',
        extra: t('mahjong.kyuushukyuuhai'),
        mahgen: '',
      }
    case 'kita':
      return {
        glyph: kitaGlyph,
        color: '#d5508d',
        glyphColor: '#e9a2c3',
        labelKey: 'mahjong.kita',
        extra: t('mahjong.kita'),
        mahgen: 'N',
      }
    case 'none':
      return {
        glyph: noneGlyph,
        color: '#a0a0a0',
        glyphColor: '#d3d3d3',
        labelKey: 'mahjong.none',
        extra: t('mahjong.skip'),
        mahgen: '',
      }
    default:
      return null
  }
}

// "#aabbcc" → "rgba(170,187,204,a)". Returns null when input isn't a valid hex.
function hexToRgba(hex: string | null, alpha: number): string | undefined {
  if (!hex) return undefined
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return undefined
  const n = parseInt(m[1], 16)
  return `rgba(${(n >> 16) & 0xff}, ${(n >> 8) & 0xff}, ${n & 0xff}, ${alpha})`
}

export function BotActionTile({ bp }: { bp: Breakpoint }) {
  const { t } = useTranslation()
  const responses = useNotifyStore((s) => s.responses)
  const latest = useMemo(
    () => [...responses].reverse().find((r) => DECISION_TYPES.has(r.type)),
    [responses],
  )
  const variant = latest ? describe(latest, t) : null
  // Mahgen sizes off this ref (the full BotActionTile content row), so the
  // tile rescales when the user resizes the grid item rather than being
  // capped by the narrow right column.
  const rowRef = useRef<HTMLDivElement>(null)

  // Score lookup is keyed on the response's monotonic _seq so each new hora
  // triggers exactly one IPC round-trip. Hook is always called (rules of
  // hooks) but stays inert when the latest response isn't hora.
  const isHora = latest?.type === 'hora'
  const score = useBotHoraScore(
    isHora,
    isHora ? latest.actor : 0,
    isHora ? latest.actor === latest.target : false,
    latest?._seq ?? 0,
  )

  // Hora's right tile and extra info come from the score result rather than
  // the bot response, since the response itself doesn't carry a winning
  // tile or score.
  let extra = variant?.extra ?? ''
  let mahgen = variant?.mahgen ?? ''
  if (isHora && variant) {
    if (score) {
      extra = t('mahjong.points_value', { points: score.points.toLocaleString() })
      mahgen = mjaiToMahgen([score.win_tile])
    } else {
      // While the IPC is in flight or returned None, leave info blank so
      // the tile stays clean.
      extra = ''
      mahgen = ''
    }
  }

  // Two-layer setup so the drop-shadow filter sees the glyph silhouette as
  // its alpha source. If filter + mask are on the *same* element the filter
  // runs on the pre-mask solid rectangle (no edges → no shadow). The wrapper
  // owns the filter; the inner div owns the mask + tinted fill, and the
  // wrapper's filter then composites against the inner's masked output.
  const glyphFilter = variant
    ? {
        // Layered shadows: wide diffuse colour bloom → tighter colour halo →
        // soft directional drop. Together they emphasize the glyph without
        // hard edges, so it reads as glowing rather than stamped on.
        filter: [
          `drop-shadow(0 0 10px ${hexToRgba(variant.glyphColor, 0.55) ?? 'rgba(0,0,0,0.5)'})`,
          `drop-shadow(0 0 4px ${hexToRgba(variant.glyphColor, 0.45) ?? 'rgba(0,0,0,0.4)'})`,
          'drop-shadow(0 2px 5px rgba(0,0,0,0.55))',
        ].join(' '),
      }
    : undefined
  const glyphMask = variant
    ? {
        backgroundColor: variant.glyphColor,
        maskImage: `url(${variant.glyph})`,
        maskRepeat: 'no-repeat',
        maskSize: 'contain',
        maskPosition: 'center',
        WebkitMaskImage: `url(${variant.glyph})`,
        WebkitMaskRepeat: 'no-repeat',
        WebkitMaskSize: 'contain',
        WebkitMaskPosition: 'center',
      }
    : undefined

  return (
    <TileFrame id="bot-action" title={t('tile.bot_action')} bp={bp} contentClassName="p-0">
      <div
        ref={rowRef}
        className="flex h-full items-stretch gap-3 px-3 py-2 transition-colors"
        style={{ backgroundColor: hexToRgba(variant?.color ?? null, 0.18) }}
      >
        {variant ? (
          <>
            <div className="flex items-center justify-center w-1/4 min-w-[64px]">
              <div className="w-full h-full" style={glyphFilter}>
                <div
                  role="img"
                  aria-label={t(variant.labelKey)}
                  className="w-full h-full"
                  style={glyphMask}
                />
              </div>
            </div>
            <div className="flex flex-1 flex-col justify-center min-w-0">
              <div className="text-base font-semibold text-foreground truncate">
                {t(variant.labelKey)}
              </div>
              {extra && (
                <div className="text-sm font-medium tabular-nums text-foreground/90 truncate">
                  {extra}
                </div>
              )}
            </div>
            {mahgen && (
              <div className="flex items-center justify-center min-w-0 max-w-[40%]">
                <Mahgen seq={mahgen} kind="bot-action" containerRef={rowRef} />
              </div>
            )}
          </>
        ) : (
          <span className="self-center text-muted-foreground px-1">{t('tile.bot_action_empty')}</span>
        )}
      </div>
    </TileFrame>
  )
}
