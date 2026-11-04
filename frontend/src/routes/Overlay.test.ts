import { describe, expect, it } from 'vitest'
import {
  actionOpAtPoint,
  actionMarkerPosition,
  actionMarkersFor,
  fittedCanvas,
  HAND_LAYOUT_SETTLE_MS,
  hasMultipleMeldChoices,
  handFromSnapshot,
  handLayoutSettleDelay,
  INITIAL_HAND,
  legalChiChoices,
  markerPosition,
  markersFor,
  meldChoiceMarkerPosition,
  meldChoiceMarkersFor,
  reduceHand,
  sortHand,
  type HandState,
} from './Overlay'
import type { GameStateSnapshot, MjaiEvent, ShowMeta } from '@/types'

const apply = (state: HandState, event: MjaiEvent) => reduceHand(state, event)

describe('tile-aligned overlay state', () => {
  it('aspect-fits the 1287x724 design space into an arbitrarily resized game viewport', () => {
    // Live CDP measurement from the controlled Chromium (dpr=1.5, window
    // manually resized by the player): CSS viewport 772x728, game canvas
    // 772x434 at y=147 — width-filling with a centered 16:9 fit.
    const fitted = fittedCanvas(772, 728)
    expect(fitted.canvasWidth).toBe(772)
    expect(fitted.canvasHeight).toBe(434)
    expect(fitted.canvasLeft).toBe(0)
    expect(fitted.canvasTop).toBe(147)

    // A portrait-ish window must letterbox on the sides instead of stretching.
    const narrow = fittedCanvas(600, 800)
    expect(narrow.canvasWidth).toBe(600)
    expect(narrow.canvasHeight).toBe(337)
    expect(narrow.canvasLeft).toBe(0)
    expect(narrow.canvasTop).toBe(231.5)

    // A window taller than 16:9 letterboxes top and bottom, keeping the hand
    // rack anchored to the same canvas-relative line.
    const wide = fittedCanvas(1287, 1000)
    expect(wide.canvasWidth).toBe(1287)
    expect(wide.canvasHeight).toBe(724)
    expect(wide.canvasLeft).toBe(0)
    expect(wide.canvasTop).toBe(138)
  })

  it('keeps a later dealer draw separate so a 1s prompt stays on the physical 1s', () => {
    const snapshot: GameStateSnapshot = {
      bakaze: 'E',
      kyoku: 3,
      honba: 0,
      kyotaku: 0,
      oya: 0,
      current_player: 0,
      // The tracker's replay-mode engine keeps turn_count at 0 for the whole
      // kyoku; the opening/later-draw distinction must come from the river.
      turn_count: 0,
      phase: 'wait_act',
      is_done: false,
      num_players: 4,
      players: [{
        seat: 0,
        tehai: ['2m', '8m', '4p', '4p', '6p', '6p', '8p', '8p', '9p', '1s', '6s', '6s', '7s', '8s'],
        drawn_tile: '4p',
        melds: [],
        // A non-empty river proves the opening discard already happened, so
        // this 14-tile wait_act is a later draw, not the opening rack.
        river: [{ tile: 'W', tedashi: true, is_riichi: false }],
        score: 23900,
        riichi_declared: false,
        riichi_stage: false,
        double_riichi: false,
        riichi_declaration_index: null,
        kita_tiles: [],
      }],
      dora_markers: ['3s'],
      our_seat: 0,
    }

    const hand = handFromSnapshot(snapshot)
    const marker = markersFor({
      items: [
        { label: 'Discard', pais: ['1s'], value: '89.33%' },
        { label: 'Discard', pais: ['9p'], value: '10.67%' },
      ],
    }, hand, 3)[0]

    expect(hand.dealerOpening).toBe(false)
    expect(hand.drawn).toBe('4p')
    expect(hand.tiles).toHaveLength(13)
    expect(marker.handIndex).toBe(8)
    expect(hand.tiles[marker.handIndex]).toBe('1s')
    expect(hand.tiles[marker.handIndex]).not.toBe('6s')

    const opening = handFromSnapshot({
      ...snapshot,
      players: [{ ...snapshot.players[0], river: [] }],
    })
    expect(opening.dealerOpening).toBe(true)
    expect(opening.drawn).toBeNull()
    expect(opening.tiles).toHaveLength(14)
  })

  it('does not double-fold the dealer opening draw after a hydration race', () => {
    // start_kyoku (13 tiles) -> hydration folds 5sr into the continuous rack
    // -> the opening tsumo event replays 5sr. The rack must stay at 14.
    const kyoku: MjaiEvent = {
      type: 'start_kyoku',
      bakaze: 'E',
      dora_marker: '8s',
      kyoku: 1,
      honba: 0,
      kyotaku: 0,
      oya: 0,
      scores: [25000, 25000, 25000, 25000],
      tehais: [
        ['3m', '6m', '8m', '9m', '4p', '6p', '6p', '7p', '8p', '8p', '4s', '5s', 'S'],
        ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?'],
        ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?'],
        ['?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?', '?'],
      ],
    }
    let state = apply(INITIAL_HAND, { type: 'start_game', names: ['a', 'b', 'c', 'd'], id: 0 })
    state = apply(state, kyoku)
    expect(state.dealerOpening).toBe(true)
    expect(state.tiles).toHaveLength(13)

    // Hydration: the tracker snapshot already contains the drawn 5sr.
    state = { ...state, tiles: sortHand([...state.tiles, '5sr']), drawn: null }
    expect(state.tiles).toHaveLength(14)

    // The replayed opening tsumo must not add it again.
    state = apply(state, { type: 'tsumo', actor: 0, pai: '5sr' })
    expect(state.tiles).toHaveLength(14)
    expect(state.tiles.filter((tile) => tile === '5sr' || tile === '0s')).toHaveLength(1)
  })

  it('keeps the drawn tile separate until the discard', () => {
    let state: HandState = {
      seat: null,
      tiles: [],
      drawn: null,
      dealerOpening: false,
    }
    state = apply(state, { type: 'start_game', names: [], id: 0 })
    state = apply(state, {
      type: 'start_kyoku',
      bakaze: 'E',
      dora_marker: '1m',
      kyoku: 1,
      honba: 0,
      kyotaku: 0,
      oya: 1,
      scores: [25000, 25000, 25000, 25000],
      tehais: [
        ['9s', '1m', '2m', '3m', '4m', '5mr', '5m', '6p', '7p', '8p', 'E', 'S', 'W'],
        [],
        [],
        [],
      ],
    })
    state = apply(state, { type: 'tsumo', actor: 0, pai: 'C' })

    expect(state.drawn).toBe('C')
    expect(state.tiles).toHaveLength(13)
    expect(state.tiles.indexOf('5mr')).toBeLessThan(state.tiles.indexOf('5m'))
  })

  it('models the dealer opening as one continuous sorted 14-tile rack', () => {
    let state: HandState = {
      seat: null,
      tiles: [],
      drawn: null,
      dealerOpening: false,
    }
    state = apply(state, { type: 'start_game', names: [], id: 0 })
    state = apply(state, {
      type: 'start_kyoku',
      bakaze: 'E',
      dora_marker: '1m',
      kyoku: 1,
      honba: 0,
      kyotaku: 0,
      oya: 0,
      scores: [25000, 25000, 25000, 25000],
      tehais: [
        ['1m', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', '1p', '2p', '3p', '4p'],
        [],
        [],
        [],
      ],
    })
    state = apply(state, { type: 'tsumo', actor: 0, pai: '5p' })

    expect(state.tiles).toHaveLength(14)
    expect(state.drawn).toBeNull()
    expect(state.dealerOpening).toBe(true)
    expect(markersFor({
      items: [{ label: 'Discard', pais: ['5p'], value: '99%' }],
    }, state, 3)).toEqual([{
      handIndex: state.tiles.indexOf('5p'),
      drawn: false,
      text: '打 99',
      primary: true,
    }])

    // Majsoul reports the dealer's first discard with moqie=false even when
    // it is the 14th dealt tile. The rack must still shrink to 13.
    state = apply(state, { type: 'dahai', actor: 0, pai: '5p', tsumogiri: false })
    expect(state.tiles).toHaveLength(13)
    expect(state.tiles).not.toContain('5p')
    expect(state.dealerOpening).toBe(false)
  })

  it('folds the drawn tile into the sorted hand after a hand discard', () => {
    const initial: HandState = {
      seat: 0,
      tiles: sortHand(['1m', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', '1p', '2p', '3p', '4p']),
      drawn: 'C',
      dealerOpening: false,
    }
    const state = apply(initial, { type: 'dahai', actor: 0, pai: '1m', tsumogiri: false })

    expect(state.drawn).toBeNull()
    expect(state.tiles).toHaveLength(13)
    expect(state.tiles).not.toContain('1m')
    expect(state.tiles.at(-1)).toBe('C')
  })

  it('places a matching recommendation on the separated drawn tile', () => {
    const hand: HandState = {
      seat: 0,
      tiles: sortHand(['1m', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', '1p', '2p', '3p', '4p']),
      drawn: '5m',
      dealerOpening: false,
    }
    const show: ShowMeta = {
      items: [{ label: 'Discard', pais: ['5m'], value: '98%' }],
    }

    expect(markersFor(show, hand, 3)).toEqual([
      {
        handIndex: 13,
        drawn: true,
        text: '打 98',
        primary: true,
      },
    ])
  })

  it('keeps red and ordinary five markers on their physical tile instances', () => {
    const hand: HandState = {
      seat: 0,
      tiles: sortHand(['1m', '2m', '3m', '4m', '5mr', '6m', '7m', '8m', '9m', '1p', '2p', '3p', '4p']),
      drawn: '5m',
      dealerOpening: false,
    }
    const show: ShowMeta = {
      items: [{ label: 'Discard', pais: ['5mr'], value: '88%' }],
    }

    expect(markersFor(show, hand, 3)).toEqual([{
      handIndex: hand.tiles.indexOf('5mr'),
      drawn: false,
      text: '打 88',
      primary: true,
    }])
  })

  it('preserves an unrelated drawn tile when kan or kita consumes from the rack', () => {
    const ankan: HandState = {
      seat: 0,
      tiles: sortHand(['1m', '1m', '1m', '1m', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', 'E']),
      drawn: 'C',
      dealerOpening: false,
    }
    const afterAnkan = apply(ankan, {
      type: 'ankan',
      actor: 0,
      consumed: ['1m', '1m', '1m', '1m'],
    })
    expect(afterAnkan.tiles).toHaveLength(10)
    expect(afterAnkan.tiles).toContain('C')
    expect(afterAnkan.drawn).toBeNull()

    const kakan: HandState = {
      seat: 0,
      tiles: sortHand(['S', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', 'E']),
      drawn: 'C',
      dealerOpening: false,
    }
    const afterKakan = apply(kakan, {
      type: 'kakan',
      actor: 0,
      pai: 'S',
      consumed: ['S', 'S', 'S'],
    })
    expect(afterKakan.tiles).toHaveLength(10)
    expect(afterKakan.tiles).toContain('C')

    const kita: HandState = {
      seat: 0,
      tiles: sortHand(['N', '1m', '2m', '3m', '4m', '5m', '6m', '7m', '8m', '9m', '1p', '2p', '3p']),
      drawn: 'C',
      dealerOpening: false,
    }
    const afterKita = apply(kita, { type: 'kita', actor: 0, pai: 'N' })
    expect(afterKita.tiles).toHaveLength(13)
    expect(afterKita.tiles).toContain('C')
  })

  it('gates markers until the corresponding hand animation has settled', () => {
    const ordinary: HandState = {
      seat: 0,
      tiles: [],
      drawn: null,
      dealerOpening: false,
    }
    const dealer = { ...ordinary, dealerOpening: true }

    expect(handLayoutSettleDelay(
      { type: 'tsumo', actor: 0, pai: '1m' },
      ordinary,
    )).toBe(HAND_LAYOUT_SETTLE_MS.draw)
    expect(handLayoutSettleDelay(
      { type: 'tsumo', actor: 0, pai: '1m' },
      dealer,
    )).toBe(HAND_LAYOUT_SETTLE_MS.dealerOpening)
    expect(handLayoutSettleDelay(
      { type: 'pon', actor: 0, target: 1, pai: '2m', consumed: ['2m', '2m'] },
      ordinary,
    )).toBe(HAND_LAYOUT_SETTLE_MS.meld)
    expect(handLayoutSettleDelay(
      { type: 'tsumo', actor: 1, pai: '?' },
      ordinary,
    )).toBe(0)
  })

  it('clears retained hand data at every game lifecycle boundary', () => {
    const active: HandState = {
      seat: 0,
      tiles: sortHand(['1m', '2m', '3m']),
      drawn: '4m',
      dealerOpening: false,
    }

    expect(apply(active, { type: 'end_game' })).toEqual({
      seat: 0,
      tiles: [],
      drawn: null,
      dealerOpening: false,
    })
    expect(apply(active, { type: 'end_kyoku' })).toEqual({
      seat: 0,
      tiles: [],
      drawn: null,
      dealerOpening: false,
    })
  })

  it('uses the reference 1287x724 tile anchors at native size', () => {
    const position = markerPosition(0, false, 13, 1287, 724)

    expect(position.left).toBeCloseTo(1287 * 0.115625, 5)
    expect(position.bottom).toBeCloseTo(724 * 0.1361111111111111, 5)
    expect(position.width).toBeCloseTo(1287 * 0.0484375 * 1.09, 5)
  })

  it('aspect-fits and remains aligned in a short wide renderer', () => {
    const position = markerPosition(0, false, 13, 502, 129)
    const fittedWidth = Math.floor(1287 * (129 / 724))
    const expectedLeft = (502 - fittedWidth) / 2 + fittedWidth * 0.115625

    expect(position.left).toBeCloseTo(expectedLeft, 5)
    expect(position.bottom).toBeCloseTo(129 * 0.1361111111111111, 5)
    expect(position.width).toBeGreaterThan(0)
    expect(position.height).toBeGreaterThan(0)
  })
})

describe('operation-button overlay markers', () => {
  it('puts pass and pon over the two rightmost Majsoul action slots', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Pass', value: '83%' },
        { label: 'Pon', pais: ['2p', '2p', '2p'], value: '17%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 83', primary: true },
      { op: 'pon', slot: 1, text: '碰 17', primary: false },
    ])
  })

  it('places Daiminkan before Pon even when the cloud ranks Pon higher', () => {
    // This is the exact coarse response shape produced by the cloud API when
    // three matching tiles make both Pon and Daiminkan legal.  The probability
    // order must not be reused as Majsoul's physical button order.
    const show: ShowMeta = {
      items: [
        { label: 'Pass', value: '45%' },
        { label: 'Pon', value: '28%' },
        { label: 'Kan', value: '27%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 45', primary: true },
      { op: 'kan', slot: 1, text: '杠 27', primary: false },
      { op: 'pon', slot: 2, text: '碰 28', primary: false },
    ])

    const y = 724 * (7 / 9)
    expect(actionOpAtPoint(show, 1287 * (8.6375 / 16), y, 1287, 724)).toBe('kan')
    expect(actionOpAtPoint(show, 1287 * (6.4 / 16), y, 1287, 724)).toBe('pon')
  })

  it('reserves Daiminkan when Kan is legal but omitted from cloud Top-3', () => {
    // Latest live regression: the table showed Pass/Kan/Pon/Chi, while the API
    // returned only Pon/Pass/Chi. Pon must keep the third physical button.
    const show: ShowMeta = {
      legal_ops: ['pass', 'chi', 'pon', 'kan'],
      items: [
        { label: 'Pon', pais: ['5sr', '5s', '5s'], value: '27%' },
        { label: 'Pass', value: '27%' },
        { label: 'Chi', value: '26%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 27', primary: false },
      { op: 'pon', slot: 2, text: '碰 27', primary: true },
      { op: 'chi', slot: 3, text: '吃 26', primary: false },
    ])

    const y = 724 * (7 / 9)
    expect(actionOpAtPoint(show, 1287 * (6.4 / 16), y, 1287, 724)).toBe('pon')
    expect(actionOpAtPoint(show, 1287 * (8.6375 / 16), y, 1287, 724)).toBeNull()
  })

  it('keeps Reach before an own-turn Ankan or Kakan prompt', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Kan', value: '48%' },
        { label: 'Riichi', value: '42%' },
        { label: 'Discard', pais: ['9s'], value: '10%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 10', primary: false },
      { op: 'reach', slot: 1, text: '立 42', primary: false },
      { op: 'kan', slot: 2, text: '杠 48', primary: true },
    ])
  })

  it('uses the best ordinary discard as the score for skipping riichi', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Riichi', pais: ['6p'], value: '61%' },
        { label: 'Discard', pais: ['6p'], value: '31%' },
        { label: 'Discard', pais: ['9s'], value: '8%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 31', primary: false },
      { op: 'reach', slot: 1, text: '立 61', primary: true },
    ])
  })

  it('marks skip as primary when an ordinary discard beats riichi', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Discard', pais: ['9s'], value: '72%' },
        { label: 'Riichi', pais: ['6p'], value: '28%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 72', primary: true },
      { op: 'reach', slot: 1, text: '立 28', primary: false },
    ])
  })

  it('shows an explicit no-riichi marker when riichi is legal but outside top-N', () => {
    const show: ShowMeta = {
      items: [
        { label: 'No Riichi', pais: ['5p'], value: '42%' },
        { label: 'No Riichi', pais: ['9m'], value: '31%' },
        { label: 'No Riichi', pais: ['E'], value: '18%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '不立 42', primary: true },
    ])
    expect(markersFor(show, {
      seat: 0,
      tiles: sortHand(['5p', '9m', 'E']),
      drawn: null,
      dealerOpening: false,
    }, 3)).toHaveLength(3)
  })

  it('deduplicates multiple meld variants that share one physical button', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Chi', pais: ['2m', '3m', '4m'], value: '51%' },
        { label: 'Chi', pais: ['3m', '4m', '5m'], value: '29%' },
        { label: 'Pass', value: '20%' },
      ],
    }

    expect(actionMarkersFor(show)).toEqual([
      { op: 'pass', slot: 0, text: '过 20', primary: false },
      { op: 'chi', slot: 1, text: '吃 51', primary: true },
    ])
  })

  it('keeps button anchors aligned after aspect fitting', () => {
    const position = actionMarkerPosition(0, 502, 129)
    const fittedWidth = Math.floor(1287 * (129 / 724))
    const left = (502 - fittedWidth) / 2 + fittedWidth * (10.875 / 16)

    expect(position.left).toBeCloseTo(left, 5)
    expect(position.bottom).toBeGreaterThan(129 * (1 - 7 / 9))
    expect(position.width).toBeGreaterThan(0)
  })

  it('keeps enlarged action plates clear of neighbouring slots in every layout', () => {
    const W = 1287
    const H = 724
    const width = actionMarkerPosition(0, W, H).width
    const height = actionMarkerPosition(0, W, H).height

    // Same-row slots are ~2.24 normalized units apart; a wider plate must not
    // bridge the gap and cover the neighbour's button. Buttons run right-to-left,
    // so slot 0 is the rightmost position and left values decrease per slot.
    for (const row of [[0, 1, 2], [3, 4, 5], [6, 7, 8]]) {
      const lefts = row.map((slot) => actionMarkerPosition(slot, W, H).left)
      expect(lefts[0] - lefts[1]).toBeGreaterThan(width)
      expect(lefts[1] - lefts[2]).toBeGreaterThan(width)
    }

    // Rows stack upward (smaller y = higher); each row must clear the one below.
    expect(actionMarkerPosition(3, W, H).bottom - actionMarkerPosition(0, W, H).bottom)
      .toBeGreaterThan(height)
    expect(actionMarkerPosition(6, W, H).bottom - actionMarkerPosition(3, W, H).bottom)
      .toBeGreaterThan(height)
  })

  it('recognises a real click on the Chi action slot', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Pass', value: '20%' },
        { label: 'Chi', pais: ['5m', '3m', '4m'], value: '80%' },
      ],
    }
    const x = 1287 * (8.6375 / 16)
    const y = 724 * (7 / 9)

    expect(actionOpAtPoint(show, x, y, 1287, 724)).toBe('chi')
    expect(actionOpAtPoint(show, 1287 / 2, 724 / 2, 1287, 724)).toBeNull()
  })
})

describe('second-stage meld choice overlay markers', () => {
  const hand: HandState = {
    seat: 0,
    tiles: sortHand(['3m', '4m', '6m', '1p', '2p', '3p', '4p', '5p', '6p', '7s', '8s', '9s', 'E']),
    drawn: null,
    dealerOpening: false,
  }

  it('reconstructs legal Chi choices in Majsoul left-to-right sequence order', () => {
    expect(legalChiChoices(hand, '5m')).toEqual([
      ['5m', '3m', '4m'],
      ['5m', '4m', '6m'],
    ])
  })

  it('keeps red-five Chi alternatives as separate physical choices', () => {
    const redHand: HandState = {
      seat: 0,
      tiles: sortHand(['3m', '0m', '5m', '1p', '2p', '3p']),
      drawn: null,
      dealerOpening: false,
    }
    expect(legalChiChoices(redHand, '4m')).toEqual([
      ['4m', '3m', '0m'],
      ['4m', '3m', '5m'],
    ])
  })

  it('places ranked Chi variants over their corresponding chooser groups', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Chi', pais: ['5m', '3m', '4m'], value: '51%' },
        { label: 'Chi', pais: ['5m', '4m', '6m'], value: '29%' },
        { label: 'Pass', value: '20%' },
      ],
    }

    expect(hasMultipleMeldChoices(show, 'chi', hand, '5m')).toBe(true)
    expect(meldChoiceMarkersFor(show, 'chi', hand, '5m')).toEqual([
      {
        op: 'chi',
        slot: 0,
        choiceCount: 2,
        text: '吃 51',
        primary: true,
        pais: ['5m', '3m', '4m'],
      },
      {
        op: 'chi',
        slot: 1,
        choiceCount: 2,
        text: '吃 29',
        primary: false,
        pais: ['5m', '4m', '6m'],
      },
    ])
  })

  it('maps a cloud response with one exact choice into the correct option slot', () => {
    const show: ShowMeta = {
      items: [
        { label: 'Chi', pais: ['5m', '4m', '6m'], value: '61%' },
        // Cloud runner-ups are coarse labels without exact consumed tiles.
        { label: 'Chi', value: '24%' },
        { label: 'Pass', value: '15%' },
      ],
    }

    expect(meldChoiceMarkersFor(show, 'chi', hand, '5m')).toEqual([
      {
        op: 'chi',
        slot: 1,
        choiceCount: 2,
        text: '吃 61',
        primary: true,
        pais: ['5m', '4m', '6m'],
      },
    ])
  })

  it('uses the captured two-choice centres and stays aspect-fitted', () => {
    const left = meldChoiceMarkerPosition(0, 2, 1287, 724)
    const right = meldChoiceMarkerPosition(1, 2, 1287, 724)
    expect(left.left).toBeCloseTo(1287 * 0.442, 3)
    expect(right.left).toBeCloseTo(1287 * 0.558, 3)
    expect(left.bottom).toBeCloseTo(724 * (1 - 0.594), 3)

    const compact = meldChoiceMarkerPosition(1, 2, 502, 129)
    expect(compact.left).toBeGreaterThan(0)
    expect(compact.left).toBeLessThan(502)
    expect(compact.bottom).toBeGreaterThan(0)
  })

  it('keeps enlarged meld-choice plates clear of each other for 2-3 choices', () => {
    const W = 1287
    const H = 724
    for (const count of [2, 3]) {
      const lefts = Array.from(
        { length: count },
        (_, slot) => meldChoiceMarkerPosition(slot, count, W, H).left,
      )
      const width = meldChoiceMarkerPosition(0, count, W, H).width
      for (let i = 1; i < lefts.length; i += 1) {
        expect(lefts[i] - lefts[i - 1]).toBeGreaterThan(width)
      }
    }
  })
})
