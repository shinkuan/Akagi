# `bridge/riichi_city` — Riichi City (麻雀一番街) protocol bridge

Translates Riichi City's WebSocket traffic into the mjai event stream. Native
client only (no web build), so it is reachable **only through the MITM proxy**;
the Chromium/CDP capture backend does not apply. **Observe-only** — no autoplay.

This is a Rust port of the original Akagi v2 Python bridge
(`mitm/bridge/riichi_city/`). The on-the-wire framing comes from the
riichishitty reverse-engineering notes.

## Files

| File | Responsibility |
|---|---|
| `packet.rs` | `WPacket` framing: 15-byte big-endian header + JSON body decode. |
| `consts.rs` | `card_to_mjai` — Riichi City tile code → mjai tile string. |
| `state.rs` | `GameStatus` — per-flow seat / player / pending state. |
| `mod.rs` | `RiichiCityBridge` — `cmd` dispatch + mjai event building. |

## Wire format (WPacket)

One WebSocket binary message normally carries one WPacket. All multi-byte
header fields are **big-endian**:

```text
[0..4]   packet_size    u32   total bytes including header
[4..6]   header_size    u16   always 15  ─┐ together the magic
[6..8]   version        u16   always 1   ─┘ bytes 00 0f 00 01
[8..12]  message_index  u32   request/response correlation counter
[12..14] cmd            u16   binary command enum (CMDAuth = 1)
[14]     has_body       u8    0 / 1
[15..]   json_payload   UTF-8 JSON (present when packet_size > 15)
```

## Two-level command

- The binary `cmd` matters only for `CMDAuth` (1), whose JSON body carries our
  player `uid` (client → server).
- Every gameplay message carries a **string** `"cmd"` field inside the JSON
  body (`cmd_enter_room`, `cmd_game_start`, `cmd_game_action_brc`, …). Dispatch
  keys on that string.

Both wire directions are parsed (the `uid` packet is uplink; gameplay is
downlink). Client request frames don't match any server `cmd_*` and are ignored.

## cmd → mjai

| Riichi City `cmd` (+ action code) | mjai event(s) |
|---|---|
| `cmd_enter_room` | — (records players, table size) |
| `cmd_game_start` | `start_game` (first kyoku) → `start_kyoku` → opening `tsumo` |
| `cmd_in_card_brc` | `tsumo` (opponent draw, hidden `?`) |
| `cmd_send_current_action` | `tsumo` (our draw, revealed) |
| `cmd_game_action_brc` action 2/3/4 | `chi` |
| … action 5 | `pon` |
| … action 6 | `daiminkan` |
| … action 8 | `ankan` (`card` + `group_cards`) |
| … action 9 | `kakan` (`consumed` from the seat's earlier pon) |
| … action 11 | `reach`? + `dahai` (+ deferred `reach_accepted`, kan-dora flush) |
| … action 13 | `kita` (nukidora) |
| … action 7 / 10 / 12 | — (end flagged here; settled by `cmd_game_end`) |
| `cmd_gang_bao_brc` | — (defers a kan-dora marker) |
| `cmd_game_end` | `hora` (per winner) or `ryukyoku`, then `end_kyoku` |
| `cmd_room_end` | `end_game` |

## Call tiles and red fives

Every call broadcast names its tiles with red fives as their own codes
(`0x105`/`0x115`/`0x125`), so `consumed` is read off the wire rather than
inferred:

- **chi / pon / daiminkan**: `card` is the called tile, `group_cards` the
  tiles from the hand.
- **ankan**: the broadcast echoes the caller's request — `card` is the first
  of the four copies in rack order and `group_cards` the other three. The red
  five can be `card` itself.
- **kakan**: `card` is the added tile and `group_cards` is empty, so the pon's
  three tiles are taken from that seat's earlier `pon` (tracked per kyoku in
  `GameStatus::pons`).

Only an ankan whose `group_cards` is missing, mismatched or names a second
red, or a kakan without a tracked pon, falls back to inference (one red per
suit), with a warning.

## Sanma (3-player)

Unlike v2 (which padded everything to length 4 with a ghost seat), this bridge
emits native length-3 `scores`/`tehais` and keeps actor indices in `0..=2`,
matching the Tenhou bridge and the rest of the V3 sanma pipeline.

## Settlement (win / draw)

Each kyoku ends with `cmd_game_action_brc` carrying action 7 (ron) / 10 (tsumo)
/ 12 (abortive draw), immediately followed by `cmd_game_end` — the settlement.
The action codes only flag the end; `on_game_end` does the work:

- `end_type`: 0 = ron, 1 = tsumo, 6 = 九種九牌, 7 = exhaustive draw (荒牌流局).
  **Win vs draw is decided by `all_point`, not `win_info` presence**: a
  `win_info` entry is a winner only when `all_point > 0`. On an exhaustive draw
  `win_info` lists the *tenpai* players with `all_point == 0` (hand reveal), so
  it must be read as a draw.
- **deltas** = each player's `user_profit[].point_profit` (+ `extra_profit`),
  the per-hand change including honba and collected kyotaku but EXCLUDING that
  player's own riichi stick. The −1000 for declaring riichi is applied by the
  mjai consumer at `reach_accepted` (the Tenhou bridge splits it the same way),
  so a `user_point` running diff would double-count it. Win deltas therefore sum
  to +collected-kyotaku, draw deltas sum to 0.
- **winner** = `win_info[].user_id`; **ron target** = the last discarder
  (`last_dahai_actor`), **tsumo target** = the winner; **ura-dora** =
  `win_info[].li_bao_card`.

Emits a `hora` per winner (so double/triple ron yields multiple), or a
`ryukyoku`, then `end_kyoku`. Verified against full captures: `point_profit`
includes honba and carried kyotaku, and `user_info_list` stays in fixed seat
order across rotating dealers. Since MJAI actor order is dealer-relative, the
bridge assigns each starting score by `user_id` rather than by the list index.

Exhaustive-draw (荒牌流局) tenpai/noten payments use the same `user_point`-diff
path but weren't present in that capture.

## Adding / fixing a `cmd`

1. Find the JSON shape in a capture (or the v2 `bridge.py`).
2. Add a `match` arm in `RiichiCityBridge::dispatch` → a new `on_*` handler.
3. Build mjai events with `field_card` / `card_to_mjai` and the `GameStatus`
   helpers; push to the returned `Vec<MjaiEvent>`.
4. Add a unit test in `mod.rs` using the `feed(..)` helper with **fake** ids.
