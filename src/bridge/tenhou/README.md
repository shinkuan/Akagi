# Tenhou bridge

Translates Tenhou (天鳳) WebSocket frames into the mjai event stream the rest
of AkagiV3 consumes, and back again for autoplay.

Only server → client frames are **parsed**: all game state analysis and the
bots need arrives on them, and client frames are user input that contributes no
new information. `Bridge::build` goes the other way — it encodes a bot's chosen
action as the client frame that performs it (see `encode.rs`). Autoplay does
**not** use it: the Tenhou client owns its board state and freezes if a discard
reaches the server without going through its own handler, so `autoplay::tenhou`
drives the client's input path instead and takes only the tile-index lookup
from that module.

## Wire format at a glance

Tenhou's WS frames are plain JSON: one event per frame, dispatched by `tag`.
The complete tag inventory and field semantics follow the original Akagi
Python Tenhou bridge; this is a faithful Rust port, with comments calling
out any deliberate divergence.

### Tags we handle

| Tag | Trigger | mjai output |
|---|---|---|
| `<Z/>` | heartbeat | (none) |
| `HELO` / `REJOIN` / `GO` / `UN` / `BYE` / `SHUFFLE` | session control | (none) |
| `TAIKYOKU` | start of game | `start_game` (resolves our seat from `oya`) |
| `INIT` | start of kyoku | `start_kyoku` (sanma detected via 0-score slot) |
| `T<n>` / `U<n>` / `V<n>` / `W<n>` | tsumo (rel seats 0..3) | `tsumo` |
| `D<n>` / `E<n>` / `F<n>` / `G<n>` (uppercase) | tedashi (discard from the hand) | `dahai { tsumogiri: false }` |
| `d<n>` / `e<n>` / `f<n>` / `g<n>` (lowercase) | tsumogiri (discard of the just-drawn tile) | `dahai { tsumogiri: true }` |
| `N` with `m` | call (chi/pon/kan/kakan/nukidora) | `chi` / `pon` / `daiminkan` / `kakan` / `ankan` / `kita` |
| `REACH step=1` | declare riichi | `reach` |
| `REACH step=2` | riichi accepted | `reach_accepted` |
| `DORA` | new dora indicator | `dora` |
| `AGARI` (no `owari`) | win | `hora` + `end_kyoku` |
| `AGARI` (with `owari`) | win at game end | `hora` + `end_kyoku` + `end_game` |
| `RYUUKYOKU` (no `owari`) | exhaustive draw | `ryukyoku` + `end_kyoku` |
| `RYUUKYOKU` (with `owari`) | draw at game end | `ryukyoku` + `end_kyoku` + `end_game` |

The tag case only decides tsumogiri for *other* seats. Our own discards
compare the tile against our last draw instead (`on_dahai`), which also
covers the digit-less own-discard echo (a `D`/`d` tag with no tile index).

### Tile encoding

Tenhou tiles are integer indices `0..=135`. `index / 4` gives tile type
(`0..=33`); `index % 4` is the variant. Red 5s live at exactly `16`, `52`, `88`
(serialize as `5mr`, `5pr`, `5sr`). See `tile.rs`.

### Seat encoding

Tenhou messages always use *relative* seats: rel 0 is the observing player.
`State::rel_to_abs` / `abs_to_rel` translate to mjai's absolute frame. The
bridge resolves our absolute seat from `<TAIKYOKU oya="N"/>`: `seat = (4 - N) % 4`
(`(3 - N) % 3` once sanma is detected at INIT).

### Meld bitfield

`<N m="..."/>` packs the meld kind, target seat, and tile composition into one
integer. Bit decoding lives in `meld.rs` and follows
<http://tenhou.net/img/mentsu136.txt> exactly. Nukidora (北抜き) is the special
case `(m & 0x3F) == 0x20` — handled before the structured decoder.

### The `t` attribute — decision windows

Tsumo and discard frames may carry a `t` bitmask naming what the server is
offering us. It plays the same role Majsoul's `OptionalOperationList` does, and
the bridge tracks it in `State::window` for `autoplay::tenhou_state`. The bits
mean different things depending on which frame carried them, but the two sets do
not overlap:

| bit | on our draw (`T<n>`) | on a discard (`D`/`E`/`F`/`G<n>`) |
|---|---|---|
| 1 | — | pon |
| 2 | — | daiminkan |
| 4 | — | chi |
| 8 | — | ron |
| 16 | tsumo agari | — |
| 32 | riichi | — |
| 64 | 九種九牌 | — |

Ankan and kakan are not in the mask — the client derives them from the hand, and
so does Akagi, via the riichi engine's legal actions. Parsing is unconditional
even though only autoplay consumes it: it is cheap, and a stale window is worse
than none.

One more frame carries the claim bits: an opponent's kakan `N` when the kan can
be robbed (chankan). The client builds its ron menu straight from that
attribute — `(u=~~a.t)&&ec.Ni(u)` in its `N` handler — and the bridge opens a
window from it the same way it does for a discard. Our own calls never open one
off their own frame; the rinshan draw that follows does.

## Encoding actions (`encode.rs`)

The inverse direction, and the only implementation of `Bridge::build` for this
platform. Nothing sends its output today — see above — but the tile lookup at
its core is what autoplay uses to name a discard, and the frame table is the
executable statement of the client protocol.

Tenhou addresses tiles by index in `0..=135` — the specific physical copy — so
an mjai tile *string* only resolves against a tracked hand, which is why this
lives in the bridge rather than in autoplay.
Lookup scans candidates in descending index order and matches the red/plain
distinction exactly, so a request for a plain five never consumes the red copy;
an unsatisfiable request fails instead of substituting the wrong tile. Matched
tiles are removed from a working pool, so a pon of two identical tiles yields
two distinct indices.

Session control (`JOIN` / `GOK` / `NEXTREADY`) is deliberately **not** encoded:
Akagi observes a real client, which sends those itself.

## Adding a new tag handler

1. Add a `match` arm in `TenhouBridge::dispatch` (`mod.rs`) that routes the new
   tag to a private handler.
2. Implement the handler. It should return `Vec<MjaiEvent>`. If you need new
   per-flow state, add it to `state::State` (and reset in `reset_for_kyoku`
   when appropriate).
3. Add a unit test covering at least one realistic JSON input.

## Why client frames are still not parsed

`parse(Direction::Up, ..)` remains a no-op even now that autoplay exists. What
autoplay does goes through the client's own handlers, and the client then sends
the frame itself — after which the server echoes the action to all seats, so it
arrives on the downlink like any other event. Parsing the uplink would only
duplicate that.

## References

- Bit-level meld spec: <http://tenhou.net/img/mentsu136.txt>
- Tile-image table: <http://tenhou.net/img/tehai.js>
- mjai event types: `src/schema/mjai/mod.rs`
- Sister bridge for protobuf platforms: `src/bridge/majsoul/`
