/* eslint-disable react-refresh/only-export-components -- pure geometry/state helpers are exported for regression tests */
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X } from 'lucide-react'
import { BotShowList } from '@/components/BotShowList'
import { pickShow, visibleItems } from '@/lib/botShow'
import { invoke, listen } from '@/lib/tauri'
import type {
  AppConfig,
  BotStatus,
  BotResponse,
  GameStateSnapshot,
  MjaiEvent,
  OverlayConfig,
  ShowMeta,
} from '@/types'

// This window covers the browser's web-content viewport but remains a separate,
// transparent, click-through Tauri window. The small labels are positioned over
// the corresponding hand tiles, matching the familiar "打牌 56" presentation
// without inserting DOM/CSS/JS into the game page.

const FALLBACK: OverlayConfig = {
  enabled: true,
  top_n: 3,
  opacity: 0.95,
  always_on_top: true,
}

// If no game/bot event arrives for this long (e.g. the player left the table
// without the server sending end_game), the stale hints are cleared instead of
// lingering over the lobby/history screen.
const STALE_SHOW_MS = 60_000

const HONOR_ORDER: Record<string, number> = {
  E: 0,
  S: 1,
  W: 2,
  N: 3,
  P: 4,
  F: 5,
  C: 6,
}

function tileKey(tile: string): number {
  if (tile in HONOR_ORDER) return 300 + HONOR_ORDER[tile]
  const match = tile.match(/^([0-9])([mps])(r)?$/)
  if (!match) return 999
  const raw = Number(match[1])
  const number = raw === 0 ? 5 : raw
  const suit = match[2] === 'm' ? 0 : match[2] === 'p' ? 100 : 200
  // Red fives sort immediately before the ordinary five, as in the game hand.
  const red = raw === 0 || match[3] === 'r'
  return suit + number * 2 + (red ? 0 : 1)
}

export function sortHand(hand: string[]): string[] {
  return [...hand].sort((a, b) => tileKey(a) - tileKey(b))
}

function canonicalTile(tile: string): string {
  return tile.replace(/^0([mps])$/, '5$1r')
}

function sameTile(a: string, b: string): boolean {
  return canonicalTile(a) === canonicalTile(b)
}

/** Remove one physical tile while accepting both red-five wire spellings. */
function takeOne(hand: string[], tile: string): boolean {
  const index = hand.findIndex((candidate) => sameTile(candidate, tile))
  if (index < 0) return false
  hand.splice(index, 1)
  return true
}

export type HandState = {
  seat: number | null
  /** Sorted rack tiles, excluding an ordinary separated draw. */
  tiles: string[]
  drawn: string | null
  /** Majsoul displays the dealer's opening 14 tiles as one continuous sorted rack. */
  dealerOpening: boolean
}

export const INITIAL_HAND: HandState = {
  seat: null,
  tiles: [],
  drawn: null,
  dealerOpening: false,
}

/**
 * Remove tiles from the full concealed hand and fold any unconsumed draw back
 * into the sorted rack. This is important for ankan/kakan/kita: when the
 * declared tile came from the rack, the just-drawn *other* tile remains in the
 * hand instead of disappearing with the declaration.
 */
function consumeFromHand(state: HandState, consumed: readonly string[]): HandState {
  const remaining = [...state.tiles, ...(state.drawn ? [state.drawn] : [])]
  for (const tile of consumed) takeOne(remaining, tile)
  return {
    ...state,
    tiles: sortHand(remaining),
    drawn: null,
    dealerOpening: false,
  }
}

export function reduceHand(state: HandState, event: MjaiEvent): HandState {
  switch (event.type) {
    case 'start_game':
      return {
        seat: event.id ?? null,
        tiles: [],
        drawn: null,
        dealerOpening: false,
      }
    case 'start_kyoku': {
      const seat = state.seat
      const tiles = seat == null ? [] : (event.tehais[seat] ?? []).filter((p) => p !== '?')
      return {
        seat,
        tiles: sortHand(tiles),
        drawn: null,
        dealerOpening: seat != null && event.oya === seat,
      }
    }
    case 'tsumo': {
      if (event.actor !== state.seat) return state

      // ActionNewRound gives the dealer all 14 tiles at once. Majsoul sorts
      // those into a continuous rack (there is no tsumohai gap), which is also
      // the special case used by the long-standing autoplay coordinate code.
      if (state.dealerOpening) {
        // Hydration can fold the opening draw into the rack before the
        // opening `tsumo` event is replayed (start_kyoku -> get_game_snapshot
        // -> opening tsumo race). Adding the tile a second time would grow the
        // rack to 15 and shift every marker one slot right.
        const tiles = state.tiles.some((tile) => sameTile(tile, event.pai))
          ? state.tiles
          : [...state.tiles, event.pai]
        return {
          ...state,
          tiles: sortHand(tiles),
          drawn: null,
        }
      }

      // A second draw should normally follow a kan/kita whose reducer already
      // folded the previous draw. Retaining this guard keeps the rack count
      // stable if a platform emits the replacement draw without that event.
      const tiles = state.drawn
        ? sortHand([...state.tiles, state.drawn])
        : state.tiles
      return { ...state, tiles, drawn: event.pai }
    }
    case 'dahai': {
      if (event.actor !== state.seat) return state

      // Work on the complete 14/11/8/5/2-tile hand, but prefer the separated
      // draw only when the protocol explicitly says tsumogiri. If `moqie` is
      // false yet the tile exists only in the drawn slot (seen on real dealer
      // opening records), the ordinary first match still removes that slot and
      // prevents the rack from incorrectly growing by one tile.
      const remaining = [...state.tiles, ...(state.drawn ? [state.drawn] : [])]
      if (event.tsumogiri && state.drawn && sameTile(state.drawn, event.pai)) {
        remaining.splice(remaining.length - 1, 1)
      } else {
        takeOne(remaining, event.pai)
      }
      return {
        ...state,
        tiles: sortHand(remaining),
        drawn: null,
        dealerOpening: false,
      }
    }
    case 'chi':
    case 'pon':
    case 'daiminkan':
      return event.actor === state.seat
        ? consumeFromHand(state, event.consumed)
        : state
    case 'ankan':
      return event.actor === state.seat
        ? consumeFromHand(state, event.consumed)
        : state
    case 'kakan':
      return event.actor === state.seat
        ? consumeFromHand(state, [event.pai])
        : state
    case 'kita': {
      if (event.actor !== state.seat) return state
      const tile = event.pai ?? 'N'
      return consumeFromHand(state, [tile])
    }
    case 'end_kyoku':
    case 'end_game':
      return {
        ...state,
        tiles: [],
        drawn: null,
        dealerOpening: false,
      }
    default:
      return state
  }
}

/**
 * Rebuild the hand from the tracker's authoritative snapshot.
 *
 * A Mahjong Soul reconnect can deliver a whole GameRestore batch before the
 * overlay's independent bot-response stream catches up. If the overlay was
 * mounted during that batch, its incremental reducer may miss the draw even
 * though the tracker already has it. `drawn_tile` is the only reliable way to
 * preserve the visual gap between the rack and the just-drawn tile.
 */
export function handFromSnapshot(snapshot: GameStateSnapshot): HandState {
  const seat = snapshot.our_seat
  if (seat == null) return { ...INITIAL_HAND }

  const player = snapshot.players.find((candidate) => candidate.seat === seat)
  if (!player) return { ...INITIAL_HAND, seat }

  const fullHand = player.tehai.filter((tile) => tile !== '?')
  const rawDrawn =
    snapshot.current_player === seat && snapshot.phase === 'wait_act'
      ? player.drawn_tile
      : null
  const drawn = rawDrawn && rawDrawn !== '?' ? rawDrawn : null
  // Majsoul shows only the dealer's OPENING 14 tiles as one continuous rack;
  // every later draw is a separated tsumohai. The opening turn is the one
  // wait_act in which we hold 14 tiles and have not discarded yet this kyoku.
  // (`snapshot.turn_count` cannot be used here: the tracker's replay-mode
  // engine keeps it at 0 for the whole kyoku, which made a turn_count-based
  // guard fold the drawn tile into the rack on every dealer decision and
  // shifted every marker one slot right.)
  const dealerOpening =
    player.river.length === 0 &&
    snapshot.current_player === seat &&
    snapshot.phase === 'wait_act' &&
    snapshot.oya === seat &&
    fullHand.length >= 14

  if (dealerOpening) {
    return {
      seat,
      tiles: sortHand(fullHand),
      drawn: null,
      dealerOpening: true,
    }
  }

  const tiles = [...fullHand]
  if (drawn) takeOne(tiles, drawn)
  return {
    seat,
    tiles: sortHand(tiles),
    drawn,
    dealerOpening: false,
  }
}

/**
 * Minimum event-to-marker age for Majsoul hand-layout animations.
 *
 * The backend sees the WebSocket action before Laya has finished moving and
 * sorting the sprites. Cloud/local inference often returns in 200-300 ms, so
 * rendering immediately anchors an otherwise-correct marker over an
 * intermediate tile slot. The opening-deal value mirrors the existing,
 * empirically validated autoplay guard for the dealer's 14-tile sort.
 */
export const HAND_LAYOUT_SETTLE_MS = {
  draw: 500,
  meld: 900,
  kanOrKita: 1000,
  dealerOpening: 2000,
} as const

export function handLayoutSettleDelay(event: MjaiEvent, before: HandState): number {
  if (event.type === 'tsumo' && event.actor === before.seat) {
    return before.dealerOpening
      ? HAND_LAYOUT_SETTLE_MS.dealerOpening
      : HAND_LAYOUT_SETTLE_MS.draw
  }
  if (
    (event.type === 'chi' || event.type === 'pon') &&
    event.actor === before.seat
  ) {
    return HAND_LAYOUT_SETTLE_MS.meld
  }
  if (
    (
      event.type === 'daiminkan' ||
      event.type === 'ankan' ||
      event.type === 'kakan' ||
      event.type === 'kita'
    ) &&
    event.actor === before.seat
  ) {
    return HAND_LAYOUT_SETTLE_MS.kanOrKita
  }
  return 0
}

type Marker = {
  handIndex: number
  drawn: boolean
  text: string
  primary: boolean
}

export type ActionMarker = {
  op: ActionOp
  slot: number
  text: string
  primary: boolean
}

type ActionOp = 'pass' | 'chi' | 'pon' | 'kan' | 'reach' | 'hora' | 'ryukyoku' | 'kita'
export type MeldChoiceOp = Extract<ActionOp, 'chi' | 'pon' | 'kan'>

export type MeldChoiceMarker = {
  op: MeldChoiceOp
  slot: number
  choiceCount: number
  text: string
  primary: boolean
  pais: string[]
}

type OverlayPointer = {
  /** Position inside the Chromium renderer, normalized to 0..1. */
  x: number
  y: number
}

const ACTION_PRIORITY: Record<ActionOp, number> = {
  pass: 0,
  hora: 1,
  reach: 2,
  kan: 3,
  pon: 3,
  chi: 4,
  kita: 4,
  ryukyoku: 5,
}

/**
 * The cloud API uses the coarse label `Kan` for all three kan types.  When Pon
 * is offered in the same prompt, that coarse action is necessarily Daiminkan:
 * both actions are reacting to the same opponent discard.  Majsoul gives
 * Daiminkan priority 2 and Pon priority 3, so the kan button must be placed
 * before the pon button regardless of the model's probability/rank order.
 *
 * Without this contextual distinction both actions had priority 3.  The rank
 * tie-break then put a higher-ranked Pon row in Daiminkan's physical slot.
 * On our own turn Pon is absent, so Ankan/Kakan retain their normal priority 3
 * relative to Reach (priority 2).
 */
function actionPriority(op: ActionOp, actions: ReadonlySet<ActionOp>): number {
    if (op === 'kan' && actions.has('pon')) return 2
    return ACTION_PRIORITY[op]
}

// Same 16x9-normalised button centres used by Majsoul autoplay. Majsoul lays
// the buttons right-to-left, then wraps into a second row if necessary.
const ACTION_POSITIONS: ReadonlyArray<readonly [number, number]> = [
  [10.875, 7.0],
  [8.6375, 7.0],
  [6.4, 7.0],
  [10.875, 5.9],
  [8.6375, 5.9],
  [6.4, 5.9],
  [10.875, 4.8],
  [8.6375, 4.8],
  [6.4, 4.8],
]

function normalizedFive(tile: string): string {
  return tile.replace(/^0([mps])$/, '5$1').replace(/^5([mps])r$/, '5$1')
}

function exactTilesKey(tiles: readonly string[]): string {
  return sortHand(tiles.map(canonicalTile)).join('|')
}

function suitedTile(tile: string): { number: number; suit: string } | null {
  const match = normalizedFive(tile).match(/^([1-9])([mps])$/)
  return match ? { number: Number(match[1]), suit: match[2] } : null
}

function actionOp(label: string | undefined): ActionOp | 'discard' | null {
  const value = (label ?? '').trim().toLowerCase()
  if (!value) return null
  if (/discard|dahai|打牌|切牌/.test(value)) return 'discard'
  if (/pass|none|skip|dama|no riichi|跳过|スキップ|取消|默听|不立直/.test(value)) return 'pass'
  if (/riichi|reach|立直/.test(value)) return 'reach'
  if (/ankan|kakan|daiminkan|kan|杠|槓/.test(value)) return 'kan'
  if (/pon|碰/.test(value)) return 'pon'
  if (/chi|吃/.test(value)) return 'chi'
  if (/hora|ron|tsumo|zimo|和了|荣和|榮和|自摸/.test(value)) return 'hora'
  if (/ryukyoku|kyushu|流局|九种|九種/.test(value)) return 'ryukyoku'
  if (/kita|nukidora|拔北|抜き北/.test(value)) return 'kita'
  return null
}

function markerValue(value: string | undefined): string {
  return (value ?? '').replace('%', '').trim()
}

function actionText(op: ActionOp, value: string | undefined, sourceLabel?: string): string {
  const labels: Record<ActionOp, string> = {
    pass: '过',
    chi: '吃',
    pon: '碰',
    kan: '杠',
    reach: '立',
    hora: '和',
    ryukyoku: '流',
    kita: '北',
  }
  if (
    op === 'pass' &&
    /dama|no riichi|默听|不立直/i.test(sourceLabel ?? '')
  ) {
    labels.pass = '不立'
  }
  const probability = markerValue(value)
  return probability ? `${labels[op]} ${probability}` : labels[op]
}

/**
 * Convert the ranked candidate rows into markers for Majsoul's operation
 * buttons. Multiple chi/kan variants share one physical button, so only the
 * best-ranked row for each operation is drawn.
 *
 * On our own turn the engine ranks "ordinary discard" rather than an explicit
 * Pass action. Majsoul nevertheless shows a Skip button beside Riichi/Kan/etc.;
 * in that case the best ordinary-discard probability is the useful score for
 * declining the optional operation.
 */
export function actionMarkersFor(show: ShowMeta | null): ActionMarker[] {
  if (!show) return []

  type Candidate = { op: ActionOp; value?: string; label?: string; rank: number }
  const actions = new Map<ActionOp, Candidate>()
  let discard: { value?: string; rank: number } | null = null

  const items = visibleItems(show)
  for (let rank = 0; rank < items.length; rank += 1) {
    const item = items[rank]
    const op = actionOp(item.label)
    if (op === 'discard') {
      discard ??= { value: item.value, rank }
    } else if (op && !actions.has(op)) {
      actions.set(op, { op, value: item.value, label: item.label, rank })
    }
  }

  // A show containing only discards has no operation-button prompt.
  if (actions.size === 0) return []
  if (!actions.has('pass') && discard) {
    actions.set('pass', { op: 'pass', value: discard.value, rank: discard.rank })
  }

  // `items` is policy Top-N and may omit a legal button. Reserve slots from
  // the engine's complete legal set, but draw only candidates with real scores.
  const layoutOps = new Set<ActionOp>(actions.keys())
  for (const legal of show.legal_ops ?? []) {
    const op = actionOp(legal)
    if (op && op !== 'discard') layoutOps.add(op)
  }
  const layout = [...layoutOps].sort(
    (a, b) => actionPriority(a, layoutOps) - actionPriority(b, layoutOps),
  )
  const slots = new Map(layout.map((op, slot) => [op, slot]))
  const sorted = [...actions.values()].sort(
    (a, b) => (slots.get(a.op) ?? 99) - (slots.get(b.op) ?? 99) || a.rank - b.rank,
  )
  const bestRank = Math.min(...sorted.map((candidate) => candidate.rank))

  return sorted.flatMap((candidate) => {
    const slot = slots.get(candidate.op)
    return slot == null || slot >= ACTION_POSITIONS.length
      ? []
      : [{
          op: candidate.op,
          slot,
          text: actionText(candidate.op, candidate.value, candidate.label),
          primary: candidate.rank === bestRank,
        }]
  })
}

/**
 * Enumerate Majsoul's legal Chi combinations in the same left-to-right order
 * as its second-stage chooser: lower sequence first, then middle, then upper.
 * Red/non-red five variants remain separate because Majsoul presents them as
 * separate choices.
 */
export function legalChiChoices(hand: HandState, calledTile: string | null): string[][] {
  if (!calledTile) return []
  const called = suitedTile(calledTile)
  if (!called) return []

  const available = [...hand.tiles, ...(hand.drawn ? [hand.drawn] : [])]
  const choices: string[][] = []
  const seen = new Set<string>()

  for (let start = called.number - 2; start <= called.number; start += 1) {
    if (start < 1 || start > 7) continue
    const needed = [start, start + 1, start + 2].filter((number) => number !== called.number)
    const first = available.filter((tile) => {
      const parsed = suitedTile(tile)
      return parsed?.suit === called.suit && parsed.number === needed[0]
    })
    const second = available.filter((tile) => {
      const parsed = suitedTile(tile)
      return parsed?.suit === called.suit && parsed.number === needed[1]
    })

    for (const a of first) {
      for (const b of second) {
        const option = [calledTile, a, b]
        const key = exactTilesKey(option)
        if (!seen.has(key)) {
          seen.add(key)
          choices.push(option)
        }
      }
    }
  }

  return choices
}

function legalPonChoices(hand: HandState, calledTile: string | null): string[][] {
  if (!calledTile) return []
  const available = [...hand.tiles, ...(hand.drawn ? [hand.drawn] : [])].filter(
    (tile) => normalizedFive(tile) === normalizedFive(calledTile),
  )
  const choices: string[][] = []
  const seen = new Set<string>()
  for (let a = 0; a < available.length; a += 1) {
    for (let b = a + 1; b < available.length; b += 1) {
      const option = [calledTile, available[a], available[b]]
      const key = exactTilesKey(option)
      if (!seen.has(key)) {
        seen.add(key)
        choices.push(option)
      }
    }
  }
  return choices
}

function exactMeldItems(show: ShowMeta | null, op: MeldChoiceOp) {
  return visibleItems(show)
    .map((item, rank) => ({ item, rank, op: actionOp(item.label) }))
    .filter(
      ({ item, op: itemOp }) =>
        itemOp === op && (item.pais?.filter(Boolean).length ?? 0) >= 3,
    )
}

function calledTileFor(
  show: ShowMeta | null,
  op: MeldChoiceOp,
  lastDiscard: string | null,
): string | null {
  return lastDiscard ?? exactMeldItems(show, op)[0]?.item.pais?.[0] ?? null
}

function legalMeldChoices(
  show: ShowMeta | null,
  op: MeldChoiceOp,
  hand: HandState,
  lastDiscard: string | null,
): string[][] {
  const calledTile = calledTileFor(show, op, lastDiscard)
  if (op === 'chi') return legalChiChoices(hand, calledTile)
  if (op === 'pon') return legalPonChoices(hand, calledTile)

  // Kan choices are already exact in the local response. Keeping their source
  // order is more reliable than guessing whether the game is offering ankan,
  // kakan or daiminkan in this particular prompt.
  const choices: string[][] = []
  const seen = new Set<string>()
  for (const { item } of exactMeldItems(show, op)) {
    const pais = item.pais ?? []
    const key = exactTilesKey(pais)
    if (!seen.has(key)) {
      seen.add(key)
      choices.push(pais)
    }
  }
  return choices
}

/** Mark only the model-ranked combinations, but place them in their real UI slots. */
export function meldChoiceMarkersFor(
  show: ShowMeta | null,
  op: MeldChoiceOp,
  hand: HandState,
  lastDiscard: string | null,
): MeldChoiceMarker[] {
  const exact = exactMeldItems(show, op)
  if (exact.length === 0) return []

  const legal = legalMeldChoices(show, op, hand, lastDiscard)
  const legalKeys = legal.map(exactTilesKey)
  const choiceCount = Math.max(legal.length, exact.length)
  const bestRank = Math.min(...exact.map(({ rank }) => rank))
  const usedSlots = new Set<number>()

  return exact.flatMap(({ item, rank }, fallbackSlot) => {
    const pais = item.pais ?? []
    let slot = legalKeys.indexOf(exactTilesKey(pais))
    if (slot < 0 || usedSlots.has(slot)) slot = fallbackSlot
    if (usedSlots.has(slot)) return []
    usedSlots.add(slot)
    return [{
      op,
      slot,
      choiceCount,
      text: actionText(op, item.value),
      primary: rank === bestRank,
      pais,
    }]
  })
}

export function hasMultipleMeldChoices(
  show: ShowMeta | null,
  op: MeldChoiceOp,
  hand: HandState,
  lastDiscard: string | null,
): boolean {
  const rankedCount = visibleItems(show).filter((item) => actionOp(item.label) === op).length
  return rankedCount > 1 || legalMeldChoices(show, op, hand, lastDiscard).length > 1
}

export function markersFor(show: ShowMeta | null, hand: HandState, limit: number): Marker[] {
  if (!show || (hand.tiles.length === 0 && !hand.drawn)) return []
  const used = new Set<number>()
  const markers: Marker[] = []
  let drawnUsed = false

  for (const item of visibleItems(show, limit)) {
    const tile = item.pais?.[0]
    if (
      !tile ||
      !/discard|riichi|dama|no riichi|打牌|立直|默听|不立直/i.test(item.label ?? '')
    ) continue

    // The reference implementation deliberately prefers the separated drawn
    // tile when its value matches a candidate; this avoids putting the marker
    // on an identical tile inside the sorted thirteen-tile block.
    // Preserve red-five identity here. `5m` and `5mr` are equivalent for
    // meld legality, but they are different physical sprites and can occupy
    // different slots in the sorted rack.
    const isDrawn = !drawnUsed && hand.drawn != null && sameTile(hand.drawn, tile)
    const handIndex = isDrawn
      ? hand.tiles.length
      : hand.tiles.findIndex(
          (candidate, index) =>
            sameTile(candidate, tile) && !used.has(index),
        )
    if (handIndex < 0) continue

    if (isDrawn) drawnUsed = true
    else used.add(handIndex)
    const value = markerValue(item.value)
    markers.push({
      handIndex,
      drawn: isDrawn,
      text: value ? `打 ${value}` : '打',
      primary: markers.length === 0,
    })
  }

  return markers
}

export function fittedCanvas(width: number, height: number) {
  const gameWidth = 1287
  const gameHeight = 724
  const scale = Math.min(width / gameWidth, height / gameHeight)
  const canvasWidth = Math.floor(gameWidth * scale)
  const canvasHeight = Math.floor(gameHeight * scale)
  return {
    canvasWidth,
    canvasHeight,
    canvasLeft: (width - canvasWidth) / 2,
    canvasTop: (height - canvasHeight) / 2,
  }
}

export function markerPosition(
  index: number,
  drawn: boolean,
  concealedCount: number,
  width: number,
  height: number,
) {
  // MahjongMaster lays its hints out in a fixed 1287x724 design space, then
  // aspect-fits that space into the actual renderer rectangle. Use the same
  // model instead of assuming that the whole browser viewport is always 16:9.
  const { canvasWidth, canvasHeight, canvasLeft, canvasTop } = fittedCanvas(width, height)

  // Percentages measured from the reference implementation's normalized
  // 16:9 canvas. `left` is the tile's left edge, not its center.
  const firstLeft = 0.115625
  const tileWidthRatio = 0.0484375
  const tileGapRatio = 0.0009765625
  const drawnGapRatio = 0.01640625
  const leftRatio = drawn
    ? firstLeft +
      concealedCount * tileWidthRatio +
      Math.max(0, concealedCount - 1) * tileGapRatio +
      drawnGapRatio
    : firstLeft + index * (tileWidthRatio + tileGapRatio)

  return {
    left: canvasLeft + canvasWidth * leftRatio,
    // Exact reference anchor: FirstPaiPos.bottom = 13.61111111111111%.
    bottom: height - (canvasTop + canvasHeight * (1 - 0.1361111111111111)),
    // Measured from MAKA screenshots: the plate body is ~9% wider than the
    // tile it labels (131px vs 120px), so it slightly overhangs the tile.
    width: canvasWidth * tileWidthRatio * 1.09,
    // Maka's plate body is approximately one third of a tile's height. The
    // cat-ear tab protrudes above this box via CSS and does not affect anchoring.
    height: canvasHeight * 0.0345,
    fontSize: Math.max(7, canvasHeight * 0.0235),
  }
}

export function actionMarkerPosition(slot: number, width: number, height: number) {
  const { canvasWidth, canvasHeight, canvasLeft, canvasTop } = fittedCanvas(width, height)
  const [x, y] = ACTION_POSITIONS[slot] ?? ACTION_POSITIONS[0]

  return {
    left: canvasLeft + canvasWidth * (x / 16),
    // MAKA's meld prompts sit directly ON the button's top edge. A Majsoul
    // operation button is 1.0/9 of the canvas tall, so its top edge is 5.56%
    // above the centre; offset 5.2% makes the plate bottom overlap the button
    // top by ~1.5% so the hint reads as glued to it.
    bottom: height - (canvasTop + canvasHeight * (y / 9)) + canvasHeight * 0.052,
    // MAKA draws its meld prompts as large slanted banners (~1.0x the button
    // width), so action labels are substantially bigger than discard labels.
    width: canvasWidth * 0.115,
    height: canvasHeight * 0.075,
    fontSize: Math.max(10, canvasHeight * 0.034),
  }
}

/**
 * Hit-test a real mouse press against Majsoul's operation buttons. The event is
 * observed by the native overlay tracker, while all geometry remains in this
 * same 1287x724 aspect-fitted coordinate system used to draw the markers.
 */
export function actionOpAtPoint(
  show: ShowMeta | null,
  x: number,
  y: number,
  width: number,
  height: number,
): ActionOp | null {
  const { canvasWidth, canvasHeight, canvasLeft, canvasTop } = fittedCanvas(width, height)
  if (
    x < canvasLeft ||
    y < canvasTop ||
    x > canvasLeft + canvasWidth ||
    y > canvasTop + canvasHeight
  ) return null

  const designX = ((x - canvasLeft) / canvasWidth) * 16
  const designY = ((y - canvasTop) / canvasHeight) * 9
  for (const marker of actionMarkersFor(show)) {
    const [buttonX, buttonY] = ACTION_POSITIONS[marker.slot] ?? ACTION_POSITIONS[0]
    // A Majsoul operation button is roughly 1.8 x 1.0 units in the normalized
    // 16x9 canvas. The slightly inset box avoids treating the gap as a click.
    if (Math.abs(designX - buttonX) <= 0.88 && Math.abs(designY - buttonY) <= 0.48) {
      return marker.op
    }
  }
  return null
}

/**
 * Majsoul's second-stage meld chooser is centred above the hand. Captured
 * reference frames place two choices at x=44.2% and 55.8%; the same 11.6%
 * spacing extends naturally to one or three choices.
 */
export function meldChoiceMarkerPosition(
  slot: number,
  choiceCount: number,
  width: number,
  height: number,
) {
  const { canvasWidth, canvasHeight, canvasLeft, canvasTop } = fittedCanvas(width, height)
  const count = Math.max(1, choiceCount)
  const centerRatio = 0.5 + (slot - (count - 1) / 2) * 0.116

  return {
    left: canvasLeft + canvasWidth * centerRatio,
    // The choice panel begins at about y=60% of the 16:9 game canvas. Keep the
    // plate immediately above its decorative top bar and clear of the table.
    bottom: height - (canvasTop + canvasHeight * 0.594),
    // Three choices are spaced 11.6% of the canvas apart; keep the plate
    // comfortably narrower than that spacing so adjacent hints never touch.
    width: canvasWidth * 0.105,
    height: canvasHeight * 0.075,
    fontSize: Math.max(10, canvasHeight * 0.034),
  }
}

function DockedOverlay() {
  const [show, setShow] = useState<ShowMeta | null>(null)
  const [cfg, setCfg] = useState<OverlayConfig>(FALLBACK)
  const handRef = useRef<HandState>(INITIAL_HAND)
  const showRef = useRef<ShowMeta | null>(null)
  const lastDiscardRef = useRef<string | null>(null)
  const choiceTimerRef = useRef<number | null>(null)
  const showTimerRef = useRef<number | null>(null)
  const staleTimerRef = useRef<number | null>(null)
  const showEpochRef = useRef(0)
  const handSettleUntilRef = useRef(0)
  const handRevisionRef = useRef(0)
  const [hand, setHand] = useState<HandState>(INITIAL_HAND)
  const [lastDiscard, setLastDiscard] = useState<string | null>(null)
  const [viewport, setViewport] = useState({ width: window.innerWidth, height: window.innerHeight })
  const viewportRef = useRef(viewport)
  const [choiceOp, setChoiceOp] = useState<MeldChoiceOp | null>(null)

  const clearStaleTimer = () => {
    if (staleTimerRef.current != null) {
      window.clearTimeout(staleTimerRef.current)
      staleTimerRef.current = null
    }
  }

  const armStaleGuard = () => {
    clearStaleTimer()
    staleTimerRef.current = window.setTimeout(() => {
      staleTimerRef.current = null
      showEpochRef.current += 1
      showRef.current = null
      setShow(null)
      setChoiceOp(null)
    }, STALE_SHOW_MS)
  }

  useEffect(() => {
    const unlistens: Array<() => void> = []
    let cancelled = false
    let handHydrationRequest = 0

    const refreshHandFromSnapshot = async (revision: number) => {
      const request = ++handHydrationRequest
      try {
        const snapshot = await invoke<GameStateSnapshot | null>('get_game_snapshot')
        if (
          cancelled ||
          request !== handHydrationRequest ||
          revision !== handRevisionRef.current ||
          snapshot == null
        ) {
          return
        }
        const next = handFromSnapshot(snapshot)
        // A snapshot without a perspective cannot repair the overlay and
        // should not erase a hand that was already reconstructed from events.
        if (next.seat == null) return
        handRef.current = next
        setHand(next)
      } catch {
        /* ignore: backend may not be ready */
      }
    }

    // The overlay can be opened/reloaded in the middle of a match. Seed it
    // from the tracker so it does not depend on having seen start_game.
    void refreshHandFromSnapshot(handRevisionRef.current)

    invoke<AppConfig>('get_config')
      .then((c) => {
        if (!cancelled) setCfg(c.overlay)
      })
      .catch(() => {})

    listen<MjaiEvent>('mjai-event', (event) => {
      const revision = ++handRevisionRef.current
      const before = handRef.current
      const seatBefore = before.seat
      const settleDelay = handLayoutSettleDelay(event, before)
      if (
        event.type === 'start_game' ||
        event.type === 'start_kyoku' ||
        event.type === 'end_kyoku' ||
        event.type === 'end_game'
      ) {
        handSettleUntilRef.current = 0
      }
      if (settleDelay > 0) {
        handSettleUntilRef.current = Math.max(
          handSettleUntilRef.current,
          Date.now() + settleDelay,
        )
      }

      const next = reduceHand(before, event)
      handRef.current = next
      setHand(next)

      if (event.type === 'start_game' || event.type === 'start_kyoku') {
        void refreshHandFromSnapshot(revision)
      }

      // The exact discarded tile is enough to reconstruct every legal Chi
      // sequence from our already-maintained concealed hand. No page state or
      // DOM inspection is involved.
      if (event.type === 'dahai' && event.actor !== seatBefore) {
        lastDiscardRef.current = event.pai
        setLastDiscard(event.pai)
      } else if (event.type !== 'none') {
        lastDiscardRef.current = null
        setLastDiscard(null)
      }

      // Every incoming game event advances (or opens) a decision. Its matching
      // bot response arrives afterwards and installs the new markers. Clearing
      // here prevents call/pass labels from surviving after the player has
      // clicked an operation or after the prompt has timed out.
      if (
        event.type === 'start_game' ||
        event.type === 'start_kyoku' ||
        event.type === 'tsumo' ||
        event.type === 'dahai' ||
        event.type === 'chi' ||
        event.type === 'pon' ||
        event.type === 'daiminkan' ||
        event.type === 'ankan' ||
        event.type === 'kakan' ||
        event.type === 'reach' ||
        event.type === 'reach_accepted' ||
        event.type === 'hora' ||
        event.type === 'ryukyoku' ||
        event.type === 'kita' ||
        event.type === 'end_kyoku' ||
        event.type === 'end_game'
      ) {
        // Invalidate both a visible response and one waiting behind the hand
        // animation gate. A later game event always makes that response stale.
        showEpochRef.current += 1
        clearStaleTimer()
        if (showTimerRef.current != null) {
          window.clearTimeout(showTimerRef.current)
          showTimerRef.current = null
        }
        showRef.current = null
        setShow(null)
        setChoiceOp(null)
        if (choiceTimerRef.current != null) {
          window.clearTimeout(choiceTimerRef.current)
          choiceTimerRef.current = null
        }
      }
    }).then((u) => unlistens.push(u))

    listen<BotResponse>('bot-response', (response) => {
      // Bot responses and raw MJAI events use separate IPC forwarders. A
      // response can therefore arrive after a restore burst while the
      // incremental hand reducer is still missing one of the draw events.
      // The tracker has already applied the triggering batch at this point;
      // use its snapshot as a cheap, authoritative resynchronisation point.
      void refreshHandFromSnapshot(handRevisionRef.current)

      const nextShow = pickShow(response.meta)
      setChoiceOp(null)

      if (showTimerRef.current != null) {
        window.clearTimeout(showTimerRef.current)
        showTimerRef.current = null
      }
      showRef.current = null
      setShow(null)
      if (!nextShow) return

      const epoch = showEpochRef.current
      const publish = () => {
        showTimerRef.current = null
        if (cancelled || showEpochRef.current !== epoch) return
        showRef.current = nextShow
        setShow(nextShow)
        armStaleGuard()
      }
      const remaining = handSettleUntilRef.current - Date.now()
      if (remaining > 0) {
        showTimerRef.current = window.setTimeout(publish, remaining)
      } else {
        publish()
      }
    }).then((u) => unlistens.push(u))

    listen<BotStatus>('bot-status', (s) => {
      // Game ended, bot stopped or the environment broke: nothing new will
      // arrive, so drop whatever hints are still on screen right away.
      if (s.state === 'idle' || s.state === 'stopped' || s.state === 'error') {
        showEpochRef.current += 1
        clearStaleTimer()
        if (showTimerRef.current != null) {
          window.clearTimeout(showTimerRef.current)
          showTimerRef.current = null
        }
        showRef.current = null
        setShow(null)
        setChoiceOp(null)
        if (choiceTimerRef.current != null) {
          window.clearTimeout(choiceTimerRef.current)
          choiceTimerRef.current = null
        }
      }
    }).then((u) => unlistens.push(u))

    listen<OverlayPointer>('overlay-pointer-down', (pointer) => {
      const currentShow = showRef.current
      const currentViewport = viewportRef.current
      const op = actionOpAtPoint(
        currentShow,
        pointer.x * currentViewport.width,
        pointer.y * currentViewport.height,
        currentViewport.width,
        currentViewport.height,
      )
      if (op !== 'chi' && op !== 'pon' && op !== 'kan') return
      if (!hasMultipleMeldChoices(
        currentShow,
        op,
        handRef.current,
        lastDiscardRef.current,
      )) return

      // Majsoul changes to the combination chooser immediately after this
      // real user click. Mirror that visual phase for a bounded interval; the
      // resulting chi/pon/kan game event clears it earlier in normal play.
      setChoiceOp(op)
      if (choiceTimerRef.current != null) window.clearTimeout(choiceTimerRef.current)
      choiceTimerRef.current = window.setTimeout(() => {
        setChoiceOp(null)
        choiceTimerRef.current = null
      }, 6000)
    }).then((u) => unlistens.push(u))

    listen<OverlayConfig>('overlay-config', (c) => setCfg(c)).then((u) => unlistens.push(u))

    const root = document.documentElement
    const updateViewport = () => {
      const nextViewport = {
        width: root.clientWidth || window.innerWidth,
        height: root.clientHeight || window.innerHeight,
      }
      viewportRef.current = nextViewport
      setViewport(nextViewport)
    }
    updateViewport()
    // The native overlay HWND is resized independently of the page. Observing
    // the document element catches the resulting WebView layout change even
    // when a conventional window resize event is coalesced during live drag.
    const resizeObserver = new ResizeObserver(updateViewport)
    resizeObserver.observe(root)
    window.addEventListener('resize', updateViewport)

    return () => {
      cancelled = true
      clearStaleTimer()
      resizeObserver.disconnect()
      window.removeEventListener('resize', updateViewport)
      if (choiceTimerRef.current != null) window.clearTimeout(choiceTimerRef.current)
      if (showTimerRef.current != null) window.clearTimeout(showTimerRef.current)
      showEpochRef.current += 1
      unlistens.forEach((u) => u())
    }
    // The listener graph is intentionally installed once for this window;
    // refs carry the live hand/show state without re-subscribing every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const markers = useMemo(
    () => markersFor(show, hand, cfg.top_n),
    [show, hand, cfg.top_n],
  )
  const actionMarkers = useMemo(() => actionMarkersFor(show), [show])
  const choiceMarkers = useMemo(
    () => choiceOp == null
      ? []
      : meldChoiceMarkersFor(show, choiceOp, hand, lastDiscard),
    [show, choiceOp, hand, lastDiscard],
  )

  return (
    <div className="relative h-screen w-screen overflow-hidden bg-transparent select-none pointer-events-none">
      {markers.map((marker) => {
        const position = markerPosition(
          marker.handIndex,
          marker.drawn,
          hand.tiles.length,
          viewport.width,
          viewport.height,
        )
        return (
          <div
            key={`${marker.drawn ? 'drawn' : marker.handIndex}-${marker.text}`}
            className={`tile-hint ${marker.primary ? 'tile-hint--primary' : 'tile-hint--secondary'}`}
            style={{
              left: position.left,
              bottom: position.bottom,
              width: position.width,
              height: position.height,
              opacity: cfg.opacity,
              fontSize: position.fontSize,
            }}
          >
            <span>{marker.text}</span>
          </div>
        )
      })}
      {choiceOp == null && actionMarkers.map((marker) => {
        const position = actionMarkerPosition(
          marker.slot,
          viewport.width,
          viewport.height,
        )
        return (
          <div
            key={`action-${marker.op}`}
            className={`tile-hint action-hint ${marker.primary ? 'tile-hint--primary' : 'tile-hint--secondary'}`}
            style={{
              left: position.left,
              bottom: position.bottom,
              width: position.width,
              height: position.height,
              opacity: cfg.opacity,
              fontSize: position.fontSize,
            }}
          >
            <span>{marker.text}</span>
          </div>
        )
      })}
      {choiceMarkers.map((marker) => {
        const position = meldChoiceMarkerPosition(
          marker.slot,
          marker.choiceCount,
          viewport.width,
          viewport.height,
        )
        return (
          <div
            key={`choice-${marker.op}-${exactTilesKey(marker.pais)}`}
            className={`tile-hint meld-choice-hint ${marker.primary ? 'tile-hint--primary' : 'tile-hint--secondary'}`}
            style={{
              left: position.left,
              bottom: position.bottom,
              width: position.width,
              height: position.height,
              opacity: cfg.opacity,
              fontSize: position.fontSize,
            }}
          >
            <span>{marker.text}</span>
          </div>
        )
      })}
    </div>
  )
}

function FloatingOverlay() {
  const { t } = useTranslation()
  const [show, setShow] = useState<ShowMeta | null>(null)
  const [cfg, setCfg] = useState<OverlayConfig>(FALLBACK)

  useEffect(() => {
    const unlistens: Array<() => void> = []
    let cancelled = false

    invoke<AppConfig>('get_config')
      .then((config) => {
        if (!cancelled) setCfg(config.overlay)
      })
      .catch(() => {})

    listen<BotResponse>('bot-response', (response) => {
      const next = pickShow(response.meta)
      if (next) setShow(next)
    }).then((unlisten) => unlistens.push(unlisten))

    listen<OverlayConfig>('overlay-config', (config) => setCfg(config))
      .then((unlisten) => unlistens.push(unlisten))

    return () => {
      cancelled = true
      unlistens.forEach((unlisten) => unlisten())
    }
  }, [])

  const items = visibleItems(show, cfg.top_n)
  const close = () => {
    void invoke('set_overlay_enabled', { enabled: false }).catch(() => {})
  }

  return (
    <div
      data-tauri-drag-region
      className="h-screen w-screen overflow-hidden p-1 select-none cursor-grab active:cursor-grabbing"
    >
      <div
        data-tauri-drag-region
        className="flex h-full w-full flex-col overflow-hidden rounded-lg border border-border bg-background shadow-lg [&_*]:pointer-events-none"
        style={{ opacity: cfg.opacity }}
      >
        <header
          data-tauri-drag-region
          className="flex shrink-0 items-center gap-1 px-2 pt-1.5 pb-1"
        >
          <span className="flex-1 truncate text-[11px] font-medium text-muted-foreground">
            {show?.title ?? t('tile.bot_show_default_title')}
          </span>
          <button
            type="button"
            onClick={close}
            aria-label={t('overlay.close')}
            title={t('overlay.close')}
            className="shrink-0 cursor-pointer text-muted-foreground hover:text-foreground"
            style={{ pointerEvents: 'auto' }}
          >
            <X className="size-3.5" />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-hidden px-1.5 pb-1.5">
          {items.length === 0 ? (
            <span className="px-1 text-xs text-muted-foreground">{t('overlay.empty')}</span>
          ) : (
            <BotShowList items={items} variant="overlay" />
          )}
        </div>
      </div>
    </div>
  )
}

/**
 * Windows uses the page-aligned docked overlay. Other packaged targets keep
 * the existing draggable card because their backends do not dock to Chromium.
 * Browser/JSDOM development defaults to the docked view for regression tests.
 */
export function Overlay() {
  const packaged = '__TAURI_INTERNALS__' in window
  const windows = /Windows/i.test(navigator.userAgent)
  return packaged && !windows ? <FloatingOverlay /> : <DockedOverlay />
}
