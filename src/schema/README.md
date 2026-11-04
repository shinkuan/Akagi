# Schema Module

Shared types used across the project. Anything that crosses module boundaries
— protocol events, backend↔frontend IPC payloads, persisted records — lives
here so it isn't owned by a single subsystem.

## Existing schemas

- `mjai/` — `MjaiEvent` enum covering all 15 mjai event variants from the
  mjai protocol spec **plus** the 3p-only `Kita` variant
  (北抜き / nukidora). Used by
  `bridge::Bridge` to expose parsed game events, and by anything
  downstream that consumes them (AI bots, loggers, frontend HUD). Tiles
  are kept as `String`. JSON serialization uses `#[serde(tag = "type")]`,
  so `"type"` is always the first key. Player-shaped fields (`names`,
  `scores`, `tehais`, `deltas`) are `Vec<T>` of native length; `StartGame`
  and `StartKyoku` carry `num_players: u8` (serde default `4` for
  backward-compat with pre-3p log lines). Fields marked `#[serde(skip)]`
  (`StartGame::game_meta`, `StartKyoku::catch_up`, `EndGame`'s result) are
  Akagi-internal: they ride the event buses but never reach a bot or a log
  line.

## Adding a new schema

1. Create `src/schema/<name>/mod.rs` (or `src/schema/<name>.rs`) with your
   types. Derive `Serialize`/`Deserialize` so the type is usable on both
   sides of any boundary it might cross.
2. Register the module in `src/schema/mod.rs` and re-export the main type.
3. If the schema is consumed by `tauri` IPC commands, also derive
   `tauri::specta::Type` (or whatever binding generator is in use) so the
   frontend gets a typed definition.

## Conventions

- Keep schemas decoupled from runtime/library types (e.g. don't use a
  third-party `Tile` struct here — use `String`). The schema module should
  not pull in heavy deps.
- For tagged enums that go to JSON, prefer `#[serde(tag = "type", rename_all = "snake_case")]`
  so the discriminant is a stable, predictable string.
- Prefer `serde_with::skip_serializing_none` over hand-written
  `skip_serializing_if` attributes when an enum has many `Option` fields.
