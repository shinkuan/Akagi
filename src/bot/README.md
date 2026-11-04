# Bot Module

Runs mjai-protocol AI bots and routes `MjaiEvent`s from the platform
bridge to them. 

## Submodules

- `types` — `BotResponse`. Wire shape matches mjai.app: an mjai action
  with an optional free-form `meta` JSON object for HUD-grade data
  (confidence, reasoning, q-values, …). Backend never interprets `meta`;
  the frontend renders it.
- `manifest` — per-bot `manifest.toml` schema + `settings.toml` values.
  See "Per-bot settings" below.
- `install` — fetch a release zip from GitHub, validate it, and drop it
  into `mjai_bot/<name>/`. See "Installing from GitHub" below.
- `registry` — discovers bot directories under `mjai_bot/`. No Python
  invocation; only filesystem layout. Populates `BotEntry.manifest` from
  any `manifest.toml` found in the bot directory.
- `runtime` — `PythonRuntime`: locates bundled `python-build-standalone`
  + `uv` (or falls back to system `python3`/`uv`); runs `uv sync` on
  demand against a bot's `pyproject.toml`; produces a `tokio::process::Command`
  ready to spawn the bot. Sync is idempotent via a `mtime:size` stamp at
  `<bot>/.akagi/synced.stamp`. `reset_sync_state()` wipes the stamp + venv
  for the user-triggered "Reinstall environment" path. See "Bundled
  runtime" below for how the bundled binaries are fetched and shipped.
- `sync_guard` — `SyncGuard`: per-bot mutual exclusion for `uv sync`.
  Both `BotManager::spawn_runner` (game-start sync) and the IPC
  `sync_bot_deps` command (Reinstall environment) acquire-or-bail on the
  bot name through a shared `Arc<Mutex<HashSet<String>>>` so two parallel
  syncs against the same venv can't trample each other.
- `runner` — `BotRunner` async trait + `SubprocessBot` impl. Talks JSONL
  over stdin/stdout, pumps stderr into `tracing` (`bot=<name>` field),
  enforces a 5 s default react timeout, and `kill_on_drop(true)` so a
  dropped runner can't leak children. `reset()` writes `[{"end_game"}]`,
  waits 500 ms, then SIGKILL's and respawns. The stderr pump doubles as a
  bot→frontend notification channel: a line prefixed with `NOTIFY_PREFIX`
  (`@@AKAGI_NOTIFY@@ `) followed by a JSON `Notification` is parsed and
  forwarded onto the `NotifyBus` (→ `notify` Tauri event → bottom-right
  toast); every other line is logged as before, and a malformed payload
  after the prefix is dropped with a `warn!`.
- `manager` — `BotManager`: subscribes to the **post-tracker bus**,
  accumulates events between decision points, flushes the pending batch
  through the `BotRunner`, and broadcasts every `BotResponse` (including
  `MjaiEvent::None`) on the `BotResponseBus`. A decision point is an event
  whose *shape* could open a decision for our seat (own tsumo / others'
  dahai-or-kakan / own chi-or-pon) **and** which the riichi engine says
  our seat can actually act on — see "Deciding what to ask" below. Round
  and game boundaries (hora / ryukyoku / end_kyoku / end_game) flush
  regardless: they open no decision, but they are how the bot hears the
  hand ended, and `end_game` is where the runner is torn down.
  Spawn point is `start_game` carrying the bot's seat in the `id` field
  and the table's `num_players`. The manager picks `active_4p` or
  `active_3p` from `BotConfig` based on `num_players`; an empty slot for
  the matching mode means analysis-only for that game (no runner spawned).
- `native` — the built-in, in-process bot (no Python, no subprocess).
  Two reserved names select it: `akagi-native` (4p) / `akagi-native3p`
  (3p). `NativeBot` always loads the embedded pure-Rust `native_bot` candle
  model, and re-reads `BotConfig.api` from the shared `AppConfig` at **every
  decision**: when `is_active()`, the decision is proxied to the remote
  inference API (see `api`) with the local model kept as a legal-action gate
  (skip the API when we can't act) and an error/timeout fallback. Re-reading
  per decision is what lets the user enable cloud inference, fix a mistyped
  key, or switch models mid-game — a change resets the `Breaker` so the new
  settings are tried on the very next move. `Breaker` is an exponential-backoff
  circuit breaker (5s → 120s): after a failed call the API is skipped for the
  window, so a dead server costs one slow turn per window instead of a request
  timeout on every decision. `native::build(actor_id, num_players, config,
  notify_tx)` seeds the session silently; `BotManager::spawn_runner` calls it
  for the reserved names, bypassing the registry / venv path entirely.
- `api` — `ApiClient` + free `redeem`/`health`: a `reqwest` wrapper over
  the remote inference server (`/v3/react`, `/v3/key`, `/v3/models`,
  `/v3/redeem`, `/healthz`). Auth is a bearer header. `react` carries its own
  short `REACT_TIMEOUT` because it blocks the bot's turn; everything else uses
  the client-wide `REQUEST_TIMEOUT`. Building an `ApiClient` builds a fresh
  connection pool, so hold one and reuse it. Consumed by `NativeBot` and by the
  `native_api_*` IPC commands (redeem a code, check a key, list models).
  Also wraps the whole-game review surface behind the Review page:
  `submit_review` (gzipped `POST /v3/review`, its own generous
  `REVIEW_SUBMIT_TIMEOUT` — the server replays the full game at submit),
  `review_status` (job poll; meta-only — result bodies are served solely
  through the share URL), `review_share` (issue/re-issue the public link),
  `shares` (list) and `revoke_share`. Ids interpolated into URL paths are
  validated to ASCII alphanumerics first. The submit IPC command
  (`native_api_review_history_game`) loads the recorded history log in Rust
  and shapes it with `native::build_api_events`, so `/v3/review` sees the
  identical censored perspective `/v3/react` does and the whole-game log
  never round-trips through the webview.
- `purchase` — the unauthenticated payment handshakes used by the in-app "Buy
  key" flow, one per provider. PayPal: `create_order` / `create_subscription`
  return an `approve_url` plus a `claim_secret`, and `order_result` /
  `subscription_result` poll with that secret until the server hands back a
  key. Creem (merchant of record): `create_checkout` serves both product kinds
  through one endpoint and `checkout_result` is the single poll — its payload
  reuses `OrderResult` (a subscription resolves to `key` with `days: 0`).
  `create_order` / `create_checkout` take a `redeem` flag: with `true` the
  server redeems the prepaid code into a key itself, so the poll — and the
  buyer's backup email — carry the key rather than a one-time code the app has
  already spent. Pass `false` only to renew an existing key, the one case that
  needs the raw code, since `renew_key` exists only on `/v3/redeem`; the
  caller branches on whichever of `OrderResult`'s `key` / `code` is set, not
  on the flag it sent, so an older server that ignores `redeem` still degrades
  to the classic flow. (On the Creem create, `redeem: false` is omitted from
  the wire entirely — the endpoint is shared with subscriptions, where an
  unexpected field risks a `400`.) Prices are server-owned — only the product
  id crosses the wire, and no client secret is embedded in the binary.
  Stateless like `api`; the polling state machine lives in the frontend's
  purchase store, driven through the `native_api_*` IPC commands.
- `supervisor` — `run_bot_manager`: constructs the `BotManager` and drives
  its run loop off the `PostTrackerBus`. Tolerates a missing Python runtime
  — the built-in `native` bots need none.
- `test_http` (test-only) — a tiny scripted HTTP mock shared by the `api`,
  `purchase` and `native` tests, so they can assert on the raw request
  (path, `Authorization` header, body) and script the response.

## Deciding what to ask

A bot is fed every event and answers every event, so its replies cannot say
which ones it was *asked*: an mjai `none` is the same three bytes for "I
weighed this call and decline it" and "this was never mine to answer". The
distinction matters to anything that acts on replies — autoplay pressed a
pass button for a filler `none` once, and the real `hora` behind it arrived
to find its decision window already spent.

The riichi engine knows the difference, so `BotManager` asks it rather than
the bot. `GameTracker` computes `can_act` for our seat the instant it applies
an event and ships it with that event on the `PostTrackerBus`
(`event_bus::TrackedEvent`); the manager gates the flush on it.

Computing it *there* is the point. One server frame can carry several seats'
actions, and the tracker digests all of them in microseconds while the
manager is still waiting on inference for the first — so a manager that
looked the answer up on arrival would be asking about a state several events
old. Riding along with the event is what makes the answer belong to it.

`can_act` is `None` when the engine has no opinion — no game in progress, no
seat tagged (observer / replay). That is not "cannot act": the manager falls
back to the event shape alone, so a stream the tracker cannot follow costs
wasted round-trips rather than a silent bot.

The rule the engine applies is the same one `native::is_decision_point` uses
on its own candidate set: a legal set that is empty, or that holds nothing
but `Pass`, is not a decision (riichienv hands a `Pass` to every seat while
it is in its response phase, including the seats with nothing to claim).

## The `meta.show` card (built-in bot)

The built-in bot attaches a `meta.show` card — the ranked candidates with their
policy probabilities — to its `BotResponse`. The frontend renders it in the Bot
Show tile and the suggestion overlay. One rule governs it, and everything in
`native.rs` that touches `meta` exists to keep it true:

> **The card changes exactly when the bot chose something, and never otherwise.**

Both halves matter, and getting either wrong is invisible in tests but obvious in
a game.

*Never otherwise*: most events reaching the bot are not decisions — an opponent's
discard we cannot call, a draw that isn't ours. Those reply `MjaiEvent::None` with
`meta: None`, and the frontend leaves the card up. If they carried a card, the tile
would flicker through the whole hand.

*Exactly when it chose*: **declining a call is a choice**, and it must refresh the
card. `is_decision_point` is the gate — a legal set that is empty, or that contains
nothing but `Pass`, is not a decision. Anything else is, including a call window
where the bot passes, and there the pass is ranked as a row of its own against the
pon/chi/kan it turned down ("Pass 87% / Pon 13%"). That comparison is the most
useful thing on screen at that moment.

This was got wrong once (#190): the local path suppressed the card whenever the
top candidate was a pass, so declining a call left the *previous turn's* discard
advice on screen, reading as live advice for a decision that was already over. The
cloud-inference path had it right. Both now share the `PASS_LABEL` row, so the card
reads the same whichever one answered.

Two fields on each row are for machines, not the card, and the frontend ignores
them: `prob` (the raw probability; `value` is only a whole-percent string) and
`type` (the mjai event the row stands for — `dahai`, `reach`, `pon`, `none`, …;
omitted for `Kan`, which names no single kan kind on the API path). Autoplay's
delay inputs read both (`autoplay::delay::probs`): `type` is what tells a
runner-up *discard* from a button. Every row goes through `make_show_item`, so
a new label needs a `show_type` entry too.

## Adding a new bot

Drop a folder under `mjai_bot/`:

```
mjai_bot/<name>/
├── bot.py            # JSONL stdin → JSONL stdout
├── pyproject.toml    # uv-resolved deps; requires-python = ">=3.12"
├── manifest.toml     # OPTIONAL — settings schema (see below)
├── settings.toml     # OPTIONAL — current values; gitignored, written by Akagi
└── README.md         # bot-specific notes (model paths, license, etc.)
```

`bot.py` MUST:

- read one JSON array per line from stdin (a batch of `MjaiEvent`s)
- write one JSON object per line to stdout (a single mjai action — use
  `{"type":"none"}` when no action is owed)
- print **only** protocol JSON to stdout; logs go to stderr
- exit cleanly when it sees `{"type":"end_game"}` in a batch

A bot MAY also surface a toast in the UI by writing an
`@@AKAGI_NOTIFY@@ {…}` line to stderr (level / title / body — see
[`mjai_bot/README.md`](../../mjai_bot/README.md)).

The bot's seat is delivered three ways (pick whichever fits):

- **argv:** `python bot.py <player_id>` — matches mjai.app convention,
  so unmodified mjai.app bots run as-is.
- **env:** `AKAGI_PLAYER_ID=<player_id>`.
- **`start_game.id`:** the field on the first `start_game` event.

## Per-bot settings

A bot can ship a `manifest.toml` declaring its configurable knobs (API
URLs, keys, model selection, …). The frontend reads the manifest and
renders a generic settings form; the user's edits are persisted in
`settings.toml`.

Example `manifest.toml`:

```toml
manifest_version = 1

[bot]
name             = "mortal"
display          = "Mortal"
description      = "AGPL deep-RL mahjong bot."
version          = "0.5.0"
supported_modes  = ["4p"]   # ["3p"], ["4p"], or ["4p", "3p"]

[settings.api_url]
type    = "string"
label   = "API Server URL"
default = "https://api.example.com"
help    = "Endpoint for online inference. Leave blank to run offline."

[settings.api_key]
type    = "string"
label   = "API Key"
secret  = true
default = ""

[settings.online]
type    = "bool"
label   = "Online Mode"
default = false

[settings.temperature]
type    = "float"
label   = "Sampling Temperature"
default = 1.0
min     = 0.0
max     = 2.0
step    = 0.05

[settings.model]
type    = "enum"
label   = "Model"
default = "mortal.pth"
choices = ["mortal.pth", "mortal-1.5.pth"]
```

Field types: `string`, `bool`, `int`, `float`, `enum` (one of `choices`).
`secret = true` makes the frontend render a password input and Akagi
substitutes the value with `***` in tracing.

`supported_modes` declares which game modes this bot can play. Accepted
values: `"4p"` (yonma) and `"3p"` (sanma). Defaults to `["4p"]` when the
field is absent — pre-3p manifests stay 4p-only without edits. The Bots
route in the frontend disables the per-mode active toggle when the bot
doesn't support that mode.

Bots see the resolved settings (defaults ⊕ on-disk values, validated
against the manifest) via the env var `AKAGI_BOT_CONFIG`, which points
at a JSON file like:

```json
{
  "api_url": "https://api.example.com",
  "online": true
}
```

The bot script can `json.load(open(os.environ["AKAGI_BOT_CONFIG"]))`.
Bots that don't read the env var simply ignore it.

### Secrets caveat

For v1, `secret = true` only changes the *rendering* and *log
substitution* — the value is still stored in `settings.toml`. We
`.gitignore` `settings.toml` so it doesn't end up in source control.
OS keychain integration is on the roadmap; until then, treat
`settings.toml` as a credential file.

### When changes take effect

Updating settings does **not** restart the running bot subprocess. The
new values take effect on the next `start_game` event. The frontend
should warn the user accordingly.

## Installing from GitHub

The `install_bot_from_github` IPC command fetches the latest release of
a public GitHub repo, picks one asset, and drops it into
`mjai_bot/<name>/`. Frontend usage:

```ts
const info: BotInfo = await invoke('install_bot_from_github', {
  repo: 'Equim-chan/Mortal',          // owner/name
  assetGlob: 'mortal-v*.zip',         // optional; first .zip if omitted
  name: 'mortal',                     // optional; defaults to repo's second segment
});
```

Behaviour:

- Refuses to overwrite an existing `mjai_bot/<name>/` — the user must
  remove it first (or call `update_bot_from_manifest` for an explicit
  reinstall).
- Hits `https://api.github.com/repos/<repo>/releases/latest` anonymously.
  No token support in v1; only public repos.
- Asset selection: glob (rejecting zero or multiple matches) or first
  asset whose name ends in `.zip`.
- Streams the asset to a tempfile under `<bot_dir>/.downloads/`.
- Validates the zip header before extracting; rejects entries with `..`
  or absolute paths (zip-slip defence).
- If the archive has a single top-level directory (typical for release
  zips like `mortal-v0.5.0/…`), strips it.
- Validates that the extracted layout contains `bot.py` at the top.
- Atomic rename into `mjai_bot/<name>/`.
- After the rename, checks for the recommended `pyproject.toml` and
  `manifest.toml`. These are not required (only `bot.py` is), but if any
  are absent the install raises a `warn` toast — the target probably
  isn't the bot the user meant to install. The install still succeeds.
- Post-install: if a `PythonRuntime` is available and the extracted bot
  has a `pyproject.toml`, runs `uv sync` so dependency failures surface
  here instead of at game-start. On failure the freshly-extracted bot dir
  is **rolled back** (removed) so the install is atomic — a failed install
  leaves nothing in the registry; the user re-runs the installer to retry.
  With no runtime available, sync is skipped with a `warn` toast and the
  install still succeeds.
- Progress is reported through `NotifyBus` with sticky id
  `bot-install-<name>` (info → info → info → success).

If the bot's `manifest.toml` declares a `[bot.source]` block, calling
`update_bot_from_manifest(name)` re-runs the install using the recorded
repo/glob. The previous `mjai_bot/<name>/` is removed first — settings
and other bot-local files are not preserved.

## Installing from a local ZIP

The `install_bot_from_zip` IPC command installs a bot from a `.zip`
already on disk — offline installs, a locally-built bot, or an archive
received out-of-band. Frontend usage:

```ts
const info: BotInfo = await invoke('install_bot_from_zip', {
  zipPath: '/path/to/mortal.zip',     // absolute path to a local .zip
  name: 'mortal',                     // optional; defaults to the zip file stem
});
```

It shares the entire tail of the GitHub installer
(`install::extract_place_and_finalize`) — header/zip-slip validation,
single top-level-dir stripping, `bot.py` layout check, missing-file
warnings, atomic rename into `mjai_bot/<name>/`, and the post-install
`uv sync`. The only differences from the GitHub path:

- No release lookup or download — the caller supplies the zip directly.
- The user's source `.zip` is **never** modified or deleted (the GitHub
  path deletes its download tempfile after extraction).
- `name` defaults to the zip file stem rather than a repo segment.

Progress is reported through `NotifyBus` under sticky id
`bot-install-<name>`, identical to the GitHub install. The frontend picks
the file with the native OS dialog (`@tauri-apps/plugin-dialog`).

### Reinstall environment

The `sync_bot_deps(name, force)` IPC command re-runs `uv sync` for an
already-installed bot. With `force = true` (the only mode the frontend
button uses) the `.akagi/synced.stamp` and `.akagi/venv/` are wiped first
so a corrupted venv is rebuilt from scratch — incremental sync against a
broken venv can otherwise mask the breakage. Progress goes to `NotifyBus`
under sticky id `bot-sync-<name>`. The shared `SyncGuard` rejects a
second concurrent call for the same bot.

## Bundled runtime

End users get a packaged Akagi build that ships its own python and uv —
no system install required. The two binaries live under
`<resource_dir>/runtime/{python,uv}/<target-triple>/` and are picked up
by `PythonRuntime::locate(Some(resource_dir))` in `RuntimeMode::Bundled`.
On a dev box without bundled binaries, `locate` falls through to
`RuntimeMode::System` and uses whatever's on PATH.

### Layout

```
runtime/
├── python/<triple>/        # python-build-standalone tree (3.12.x)
│   ├── bin/python3         (linux/mac)
│   ├── lib/...
│   └── python.exe          (windows)
└── uv/<triple>/
    ├── uv                  (linux/mac)
    ├── uv.exe              (windows)
    └── uvx[.exe]
```

`<triple>` is the cargo target triple (e.g. `x86_64-unknown-linux-gnu`,
`aarch64-apple-darwin`, `x86_64-pc-windows-msvc`).

### Fetching locally

```sh
scripts/fetch-runtime.sh                          # host triple
scripts/fetch-runtime.sh x86_64-pc-windows-msvc   # cross-target
scripts/fetch-runtime.sh --force                  # re-download
```

Override the versions via env vars: `PYTHON_VERSION`, `PBS_RELEASE`,
`UV_VERSION`. The script is idempotent — re-runs are no-ops once the
target binaries exist.

### Bundling

`tauri.conf.json` ships the tree via `bundle.resources = ["runtime/**/*"]`.
The `runtime/` directory is `.gitignore`'d (`runtime/*` with
`!runtime/.gitkeep` exception) so the placeholder file keeps the glob
non-empty even before `fetch-runtime.sh` runs. CI must re-run the
script once per matrix target before `tauri build`.

## Why subprocess

- AGPL bots (Mortal et al.) link via stdin/stdout pipe only — arms-length,
  not derivative work. See plan §10.
- Crash isolation: a bot SIGSEGV does not take Akagi down.
- Hot model swap = kill + spawn, no in-process state to flush.

In-process embedding (PyO3) was considered and rejected: linking
libpython makes single-binary distribution painful on Windows/macOS and
couples Akagi's lifecycle to libriichi's.

## Per-mode active bot (4p / 3p)

`BotConfig` stores two slots: `active_4p` and `active_3p`. `BotManager`
picks the slot matching `start_game.num_players` (3 → `active_3p`, else
`active_4p`). Empty slot ⇒ no runner spawned for that game (analysis
still runs).

Frontend → backend: `set_active_bot(mode, name)` IPC command, where
`mode` is `"4p"` or `"3p"` and `name` is the bot subdir name (or `""`
to clear the slot). The Bots route shows two switches per row.

`set_active_bot` refuses to activate a bot whose Python environment isn't
installed yet (`runtime::is_synced` is false — `pyproject.toml` present but
no up-to-date `.akagi/venv` + `synced.stamp`). Otherwise the bot's first
in-game spawn would run `uv sync`, which can exceed the react time limit and
error. `BotInfo.env_ready` surfaces this to the frontend, which disables the
activation switch (and shows a tooltip) until the env is installed. A bot with
no `pyproject.toml` has nothing to install and is always `env_ready`. The
install/sync IPC paths run `uv sync` to completion before returning, and the
frontend shows a non-dismissible blocking overlay for their whole duration, so
the user can't navigate away or start a game mid-install.

Pre-3p config files with a single `[bot] active = "..."` key are
migrated on load: the legacy value is moved to `active_4p` once via
`BotConfig::migrate_legacy_active`, and the on-disk file is rewritten
without the legacy field on the next persist.

## What's NOT here

- Mortal weights. Users place `mortal.pth` inside their `mjai_bot/mortal/`
  folder; Akagi never ships, fetches, or configures weight paths. The
  bot script loads them itself.
- HUD rendering. `BotResponse`s land on the broadcast bus; the HUD layer
  is a downstream consumer added later.
