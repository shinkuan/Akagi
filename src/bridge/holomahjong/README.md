# HoloMahjong bridge

ホロじゃん / HoloMahjong (Aiming). 4- and 3-player, observe-only, MITM only —
the game is a native Windows client.

## Transport

Gameplay runs on a per-game battle server reached by IP on a high port. The
client speaks gRPC to it over **cleartext HTTP/2** (h2c with prior knowledge):
a single MagicOnion StreamingHub call, `/IMahjongHub/Connect`, carries every
message of the game in both directions. The server address changes from game to
game; the lobby hands it out just before the match.

hudsucker cannot look inside such a tunnel (after a `CONNECT` it only knows
HTTP/1 and TLS), so for this platform the proxy answers every `CONNECT` itself
(`proxy::handler::serve_tunnel`): a tunnel whose first bytes are the HTTP/2
client preface is relayed through `proxy::h2c`, which yields complete gRPC
messages, and everything else — the lobby's HTTPS included — is relayed
untouched and noted on the Inspector timeline as not inspected. No TLS is
intercepted, so the client does not need to trust Akagi's CA.

The game has no proxy setting of its own; its TCP has to be redirected to
Akagi's HTTP proxy (`[proxy] addr`) as `CONNECT`s — a per-process redirector on
Windows, or e.g. proxychains when the client runs under Wine.

## Messages

`hub.rs` splits the envelope: `[method, body]` is a server broadcast or a
client fire-and-forget call, `[message_id, method, body]` a request and its
response. `method` is MagicOnion's id for the hub method — FNV-1a 32 over its
name as a signed int — which `hub::method_id` computes at compile time.

Bodies are MessagePack-CSharp objects serialized as arrays indexed by their
`[Key(n)]`. Event DTOs start at key 1 (index 0 is always nil); nested structs
start at key 0. Each handler in `mod.rs` documents the layout it reads.

| Server broadcast | mjai |
|---|---|
| `OnKaiju` (game start) | — (seat, names, player count kept) |
| `OnQipai` (deal) | `start_game` (first deal of a game), `start_kyoku` |
| `OnZimo` | `tsumo` |
| `OnDapai` | `reach` (riichi discard) + `dahai` |
| `OnFulou` | `chi` / `pon` / `daiminkan` |
| `OnAnGangAddGang` | `ankan` / `kakan` |
| `OnAddBaopai` | `dora` |
| `OnBabei` (3-player) | `kita` |
| `OnLizhi` | `reach_accepted` |
| `OnHule` | `hora` per winner + `end_kyoku` |
| `OnPingju` | `ryukyoku` + `end_kyoku` |
| `OnJieju` (result) | `end_game` with final scores and ranks |
| `OnUpDateEndGameVote` (accepted) | `end_game` (terminated) |

Client calls and responses are recorded for the Inspector but drive nothing.

## Notation

Tiles, hands and melds use majiang-core's notation (see `tile.rs`): `m`/`p`/`s`/`z`
plus a digit, `0` for the red five; a discard may carry `_` (tsumogiri) and `*`
(riichi); in a meld the called tile's digit is followed by `+` (next seat), `=`
(opposite) or `-` (previous seat), e.g. `p1-23`, `z444+`, `m5555=`, `m555=5`.

## Seats

Wire seats are fixed for the game, but nothing assumes how they are numbered
(contiguous, or a 4-seat table with a gap at 3-player). mjai actor 0 is the
first dealer and actors follow turn order, and so do winds, so every deal maps
a seat to `(its wind + hand number) mod players`. `PingjuEvent` reports
players by **wind**, which the same formula resolves.

The target of a chi, pon or open kan is the seat that made the discard being
called. The meld string's direction mark (`+ = -`) is only a fallback for a
call whose discard we did not see: at a 3-player table it is not known whether
the server counts it in turn order or by 4-seat geometry. Win deltas include riichi sticks and
honba and map straight onto `hora.deltas`; with several winners the wire only
has the combined result, so each `hora` gets its own hand value (`Defen`) and
the first also takes what is left over (honba and riichi sticks).

## Limitations

- A reconnect restores seat and names from `OnZhunbeiChonglian`, but the
  snapshot of the hand in progress (`OnChongxinLianjieChenggong`) is not
  replayed: events resume at the next deal.
- Compressed gRPC messages are skipped (none observed).
- No autoplay.

## Shareable frames

`is_shareable` allowlists the gameplay broadcasts and the client's game calls.
`JoinRoomAsync` (account and battle-session ids), `OnKaiju` and the reconnect
snapshots (every player's account id and name) and `OnJieju` (result-screen
account data) stay in `sensitive/`. `OnQipai` has an account slot per player
that has only been seen nil; it is shared only while every slot is nil.

## Adding a message

1. Add `CONST = "MethodName"` to the `methods!` list in `hub.rs`.
2. Add a handler in `mod.rs` reading the body by key index, and a match arm in
   `dispatch`.
3. Decide whether it is shareable.
4. Add a fixture-based test in `tests.rs` — made-up names and ids only.
