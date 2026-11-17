# Bridge Module

Platform-specific protocol bridges between game wire protocols and the
[mjai JSONL protocol](https://gimite.net/pukiwiki/index.php?Mjai%20%E9%BA%BB%E9%9B%80AI%E5%AF%BE%E6%88%A6%E3%82%B5%E3%83%BC%E3%83%90)
consumed by AI bots.

## Trait

```rust
pub trait Bridge: Send {
    fn parse(&mut self, direction: Direction, content: &[u8]) -> ParseResult;
    fn build(&mut self, command: &MjaiEvent) -> Option<Vec<u8>>;
}
```

- `parse` — raw WS frame → `ParseResult { events, parsed, shareable }`: zero or
  more `MjaiEvent`s, the bridge's decoded view for the Inspector, and whether
  the frame may go into the shareable logs.
- `build` — outbound mjai command (also `MjaiEvent`) → optional raw WS binary frame (autoplay).

## mjai types

`MjaiEvent` lives in [`crate::schema::mjai`](../schema/mjai/mod.rs) — it's used
across the project (bridge output, AI bots, frontend HUD) and isn't owned by
this module. See `src/schema/README.md`.

One bridge instance per independent game session. For Majsoul that means one
per WebSocket flow, since each flow has its own request id sequence and game
state.

## Selecting a bridge

`bridge::for_platform(platform, flow_log, session, hooks)` returns a
`Box<dyn Bridge>` for the configured [`Platform`](../config/platform.rs). The
proxy handler builds a fresh bridge inside `handle_websocket` so per-flow state
is isolated.

## `BridgeHooks`

Shared slots the autoplay layer hands to a bridge. Every field is optional and
platform-specific, and only the chromium capture path wires them at all — the
MITM path has no `Page` handle, so nothing consumes them. They exist because
autoplay needs facts only the protocol parser sees:

| Field | Platform | Carries |
|---|---|---|
| `time_budget` | Majsoul | The server's per-decision-window time grant (`OptionalOperationList.time_fixed/time_add`). |
| `input_watch` | Majsoul | A counter of the client's own uplink input commands, so a click can be told from one the UI swallowed. |
| `tenhou_state` | Tenhou | The hand at Tenhou tile-index resolution plus the current decision window — what makes a client frame encodable. |

Bundled into one struct so adding a platform's slot doesn't grow the argument
list of every constructor between here and the capture backend.

## Adding a new platform

1. Add a variant to `config::Platform` (`src/config/platform.rs`).
2. Create `src/bridge/<name>/mod.rs` with a struct that implements `Bridge`.
3. Re-export it from `src/bridge/mod.rs` and add the match arm in
   `for_platform`.
4. If the platform will support autoplay and its planner needs something only
   the parser sees, add a field to `BridgeHooks` rather than a new parameter.
5. Give it an allowlist and set `ParseResult::shareable` (see below).

## Shareable frames

A session's logs outside `sensitive/` are meant to be shared in bug reports,
and nobody can know in advance which frames a platform puts credentials in.
So a frame's content reaches them only when the bridge says so:
`ParseResult::shareable` is `false` by default, and each bridge sets it from an
explicit allowlist (`is_shareable` in `majsoul/`, `tenhou/`, `riichi_city/`).
Everything else is logged as method, direction and size; the full frame is
still recorded under `sensitive/`.

To allow a message, check it carries no credentials (tokens, passwords, login
handshakes) in either direction, then add it. A message the bridge reads can
still stay off the list: Majsoul's `authGame` request is read for the account
id but carries the game token, and Riichi City's `CMD_AUTH` is read for the uid.
A new message the bridge starts reading is withheld until someone adds it.
Bridges must not log payloads of frames off the list themselves; see
`bridge::shown`.

## Existing bridges

- `majsoul/` — Majsoul (lq.* protobuf over WS). `parser.rs` decodes raw
  WS frames into `ParsedMessage { msg_type, msg_id, method_name, payload }`;
  `mod.rs` runs the state machine that turns them into mjai events. See
  `majsoul/parser.rs` module docs for the 5-layer wire format. Protocol
  layout follows the published Majsoul `liqi.proto` schema; no third-party
  source code is copied.
- `tenhou/` — Tenhou (plain JSON over WS). Parses server frames into mjai, and
  implements `build` in the other direction — not on the autoplay path, which
  drives the client's own input instead. See `tenhou/README.md`.
- `riichi_city/` — Riichi City (JSON inside a 15-byte binary header).
  Observe-only. See `riichi_city/README.md`.
