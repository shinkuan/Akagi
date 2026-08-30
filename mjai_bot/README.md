# Writing an mjai bot for Akagi

This is the developer guide for building your own AI bot. A bot is a
**standalone subprocess** that Akagi spawns and talks to over a simple
line-based JSON protocol — Akagi feeds it the live game as mjai events and the
bot replies with the action it wants to take, plus optional data for the HUD.

Everything a bot needs is in this document: the I/O protocol, the mjai event
stream, the reaction format and the `meta` HUD payload, toast notifications,
and user-configurable settings. The reference bot in
[`example/`](example/) implements all of it in plain Python and is the best
companion to read alongside this guide.

> **Internals?** This guide is for *bot authors*. If you're hacking on how
> Akagi *runs* bots (the Rust runner, the lifecycle manager, the bundled
> Python runtime, GitHub install), see [`../src/bot/README.md`](../src/bot/README.md).

> **Ready-made example.** Mortal (4-player and 3-player) is shipped as a
> template under [`../scripts/mortal_bot/`](../scripts/mortal_bot/) plus a
> setup helper [`../scripts/setup-mortal-bots.sh`](../scripts/setup-mortal-bots.sh).
> It shows a full, real-world `bot.py` that loads an external model/checkpoint
> and emits both raw `meta` and a structured `meta.show` card.

## Contents

- [Directory layout](#directory-layout)
- [The I/O protocol](#the-io-protocol)
- [Knowing your seat](#knowing-your-seat)
- [The mjai event stream](#the-mjai-event-stream)
- [The reaction & the `meta` field](#the-reaction--the-meta-field)
- [Frontend toast notifications](#frontend-toast-notifications)
- [User-configurable settings (`manifest.toml`)](#user-configurable-settings-manifesttoml)
- [Registering the bot](#registering-the-bot)
- [AGPL boundary](#agpl-boundary)

## Directory layout

Each bot lives in its own folder under `mjai_bot/`:

```
mjai_bot/<name>/
├── bot.py            # entry point — speaks the I/O protocol below
├── pyproject.toml    # dependencies; requires-python = ">=3.12"
├── manifest.toml     # OPTIONAL — UI metadata + settings schema
├── settings.toml     # OPTIONAL — current setting values (written by Akagi at runtime)
└── README.md         # bot-specific notes (model files, license, …)
```

`pyproject.toml` declares the bot's Python dependencies. The first time the
environment is built, Akagi runs `uv sync` into a per-bot virtual environment,
so a bot can pull in whatever packages it needs without touching the rest of
the system. (Akagi ships its own Python + `uv`, so bots run even with no system
Python installed.) Two things it must contain:

```toml
[project]
requires-python = ">=3.12"   # Akagi bundles Python 3.12

[tool.uv]
package = false              # the bot is a script, not an installable library
```

Without `[tool.uv] package = false`, `uv sync` tries to **build** your bot as a
package and fails — set it on every bot. See
[`example/pyproject.toml`](example/pyproject.toml) for a complete, known-good
file. A bot with no `pyproject.toml` at all is treated as dependency-free and
is activatable immediately (it must then rely only on the stdlib).

## The I/O protocol

Akagi spawns `bot.py` once per game (working directory = the bot's folder) and
communicates over **stdin / stdout**, one JSON value per line:

- **stdin → bot**: a JSON **array** of mjai events, newline-terminated. Each
  line is a *batch* — every event the game produced since the last time the bot
  was asked to react.
- **bot → stdout**: **exactly one** JSON reaction object, newline-terminated.
  This is a single mjai action (see below), or `{"type":"none"}` when the bot
  has nothing to do this turn.

```
 stdin  →  [{"type":"tsumo","actor":2,"pai":"5p"}]
stdout  ←  {"type":"dahai","actor":2,"pai":"1m","tsumogiri":false}
```

Rules:

- **Reply to every line with exactly one reaction.** Reading is strictly
  one-reaction-per-batch; printing extra lines (or none) desyncs the protocol.
- **stdout is for protocol JSON only.** Send logs and diagnostics to **stderr**
  (Akagi pumps stderr into its application log under `bot=<name>`). stderr also
  carries [toast notifications](#frontend-toast-notifications).
- **`{"type":"none"}`** is the correct reply whenever it isn't the bot's turn or
  no call is available. The bot still sees every event so its internal state
  stays current; it just declines to act.
- **Shut down on `end_game`.** When a batch contains `{"type":"end_game"}`,
  reply once (`{"type":"none"}` is fine) and exit cleanly.
- **React budget ≈ 5 s.** Akagi aborts the bot if a reaction takes longer than
  the default timeout, so keep inference within a few seconds.

A minimal loop:

```python
import json, sys

def react(events: list[dict]) -> dict:
    # ... inspect events, update state, decide ...
    return {"type": "none"}

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    events = json.loads(line)
    sys.stdout.write(json.dumps(react(events), separators=(",", ":")) + "\n")
    sys.stdout.flush()
    if any(e.get("type") == "end_game" for e in events):
        break
```

> **Note on batches.** Akagi only asks the bot to react at *decision points*
> (your own draw, an opponent's discard that opens a call window, your own
> call, round/game boundaries). The triggering event is the **last** element of
> the batch; the earlier elements are the events that accumulated since the
> previous reaction. The bot doesn't need to detect decision points itself —
> just keep state current from the whole batch and act on the last event.

## Knowing your seat

The bot's seat (`actor_id`, 0–3) is delivered three ways. Use whichever is
convenient; prefer the `start_game` value as authoritative:

1. **argv** — `python bot.py <player_id>` (matches the mjai.app convention, so
   unmodified mjai.app bots work as-is).
2. **`AKAGI_PLAYER_ID`** — environment variable, always set to the same value.
3. **`start_game.id`** — the `id` field on the first `start_game` event. This is
   the source of truth (argv/env exist mainly for mjai.app compatibility).

The reference bot resolves argv → env → default, then trusts `start_game.id`
once the game begins.

## The mjai event stream

Akagi speaks the **mjai** protocol. The authoritative, always-current list of
event types and their exact field shapes is the `MjaiEvent` enum in
[`../src/schema/mjai/mod.rs`](../src/schema/mjai/mod.rs) — read it as the
schema. The wire `type` strings:

| `type`           | Fields (key ones)                                     | Notes |
| ---------------- | ----------------------------------------------------- | ----- |
| `start_game`     | `names`, `id` (your seat), `num_players`, `aka_flag`  | First event. |
| `start_kyoku`    | `bakaze`, `kyoku`, `honba`, `oya`, `dora_marker`, `scores`, `tehais` | Hand start; `tehais[seat]` is the deal. |
| `tsumo`          | `actor`, `pai`                                        | A draw. |
| `dahai`          | `actor`, `pai`, `tsumogiri`                           | A discard. |
| `chi`            | `actor`, `target`, `pai`, `consumed[2]`               | |
| `pon`            | `actor`, `target`, `pai`, `consumed[2]`               | |
| `daiminkan`      | `actor`, `target`, `pai`, `consumed[3]`               | open kan |
| `kakan`          | `actor`, `pai`, `consumed[3]`                         | added kan |
| `ankan`          | `actor`, `consumed[4]`                                | closed kan |
| `dora`           | `dora_marker`                                         | new dora indicator |
| `reach`          | `actor`, `pai?`                                       | riichi declaration |
| `reach_accepted` | `actor`                                               | stick committed |
| `hora`           | `actor`, `target`, `deltas?`, `ura_markers?`          | a win |
| `ryukyoku`       | `deltas?`                                             | exhaustive draw |
| `kita`           | `actor`, `pai?`                                       | 3-player only (North) |
| `end_kyoku`      | —                                                     | hand over |
| `end_game`       | —                                                     | game over → exit |

**Tile strings** (`pai`, `consumed`, `tehais`, …) are mjai notation: `"1m"`…
`"9m"` (man), `"…p"` (pin), `"…s"` (sou); honors `"E" "S" "W" "N"` (winds) and
`"P" "F" "C"` (haku / hatsu / chun); a red five is suffixed `r`, e.g. `"5mr"`.
A hidden/unknown tile is `"?"`.

## The reaction & the `meta` field

A reaction is one mjai action object — the same shape as the events above. The
common replies:

```jsonc
{"type":"none"}                                              // not my turn / pass
{"type":"dahai","actor":2,"pai":"1m","tsumogiri":false}      // discard
{"type":"reach","actor":2}                                   // declare riichi (the dahai follows next turn)
{"type":"pon","actor":2,"target":0,"pai":"1m","consumed":["1m","1m"]}
{"type":"hora","actor":2,"target":0,"pai":"5p"}              // ron / tsumo
```

### `meta` — optional HUD payload

A reaction may carry an extra **`meta`** object alongside the action. Akagi
treats `meta` as **opaque** and forwards it verbatim to the frontend; the bot
owns its contents. Use it to surface *why* the bot chose its action:

```json
{"type":"dahai","actor":0,"pai":"9m","tsumogiri":false,
 "meta":{"q_values":[0.12,0.05,0.85],"confidence":0.87}}
```

Any keys you put in `meta` are visible in the HUD's bot-responses view as raw
data, so even ad-hoc fields are useful for debugging.

### `meta.show` — the structured HUD card

For a clean, rendered display, populate **`meta.show`**. Akagi's *Bot Show* HUD
tile renders the most recent reaction that carries it as a titled list of rows
(top-N discards, opponent reads, yaku breakdowns — you decide the semantics):

```json
{"type":"dahai","actor":0,"pai":"1m","tsumogiri":false,
 "meta":{"show":{
   "title":"Discard candidates",
   "items":[
     {"label":"Discard 1m","pais":["1m"],"value":"85.42%","color":"#00ff80","note":"keeps tenpai"},
     {"label":"Discard 9p","pais":["9p"],"value":"11.30%"},
     {"label":"Riichi","value":"+12000","color":"#ffaa00"}
   ]
 }}}
```

`meta.show` shape:

| Field    | Type       | Meaning                                                                 |
| -------- | ---------- | ----------------------------------------------------------------------- |
| `title`  | string?    | Card heading; falls back to a default title.                            |
| `items`  | array      | One row each. Rows with no `label`/`tiles`/`pais` are skipped.          |

Each item in `items`:

| Field   | Type      | Meaning                                                                            |
| ------- | --------- | ---------------------------------------------------------------------------------- |
| `label` | string?   | Primary text on the row.                                                           |
| `pais`  | string[]? | mjai tile strings, rendered as tile graphics.                                      |
| `tiles` | string?   | Raw mahgen DSL string; takes precedence over `pais` if both are set.               |
| `value` | string?   | Right-aligned text — any format, e.g. `"85.42%"`, `"+12000"`.                       |
| `color` | string?   | Hex accent, e.g. `"#00ff80"` — drawn as a left bar + faint row tint.                |
| `note`  | string?   | Small subtitle under `label`.                                                      |

## Frontend toast notifications

A bot can push a **toast notification** to the Akagi UI — it pops in the app's
bottom-right corner. This rides on **stderr** so it never interferes with the
stdout reaction protocol, and can be sent at any time, not just on a reaction.

Write a single stderr line of the form:

```
@@AKAGI_NOTIFY@@ {"level":"warn","title":"...","body":"...","sticky":false,"id":"..."}
```

The JSON object after the `@@AKAGI_NOTIFY@@ ` prefix (note the trailing space):

| Field    | Type   | Required | Meaning                                                            |
| -------- | ------ | -------- | ------------------------------------------------------------------ |
| `level`  | string | yes      | Severity: `"info"`, `"success"`, `"warn"`, or `"error"`.           |
| `title`  | string | yes      | Short headline, shown in bold.                                     |
| `body`   | string | no       | Longer description.                                                |
| `sticky` | bool   | no       | `true` keeps the toast until dismissed (default `false`).          |
| `id`     | string | no       | Stable key; a later toast with the same `id` replaces the earlier. |

Any stderr line **without** this exact prefix is logged as an ordinary
diagnostic line. A malformed payload after the prefix is logged as a warning
and dropped — it never reaches the UI.

A copy-pasteable helper (also in [`example/bot.py`](example/bot.py)):

```python
import json, sys

NOTIFY_PREFIX = "@@AKAGI_NOTIFY@@ "

def notify(level, title, body=None, *, sticky=False, id=None):
    payload = {"level": level, "title": title}
    if body is not None:
        payload["body"] = body
    if sticky:
        payload["sticky"] = True
    if id is not None:
        payload["id"] = id
    sys.stderr.write(NOTIFY_PREFIX + json.dumps(payload, separators=(",", ":")) + "\n")
    sys.stderr.flush()

# Examples:
notify("info", "Bot ready")
notify("warn", "Low wall", "Fewer than 8 tiles left — playing defensively.")
notify("error", "Model failed to load", "Falling back to rule-based play.")
```

The protocol is just a stderr line, so any language can implement it — the
helper is a convenience, not a requirement.

## User-configurable settings (`manifest.toml`)

A bot can expose settings that Akagi renders as a form in the **Bots** tab. Add
a `manifest.toml`:

```toml
manifest_version = 1

[bot]
name        = "my-bot"                 # should match the folder name
display     = "My Bot"                 # label in the bot picker
description = "One-line description."
version     = "0.1.0"
supported_modes = ["4p", "3p"]         # defaults to ["4p"] if omitted

[settings.temperature]
type    = "float"                      # string | bool | int | float | enum
label   = "Sampling temperature"
default = 1.0
help    = "Higher = more random."      # optional tooltip
min     = 0.1                          # int/float only
max     = 2.0
step    = 0.1

[settings.api_key]
type   = "string"
label  = "API key"
default = ""
secret = true                          # password input; redacted to *** in logs

[settings.style]
type    = "enum"
label   = "Play style"
default = "balanced"
choices = ["aggressive", "balanced", "defensive"]   # required for enum
```

Field types (`type`): `string`, `bool`, `int`, `float`, `enum`. `int`/`float`
accept `min`/`max`/`step`; `enum` requires `choices`. `secret = true` renders a
password input and redacts the value to `***` in Akagi's logs (it is still
stored in plaintext in `settings.toml`).

The current values live in `settings.toml` (Akagi writes it when the user edits
the form — it isn't something you commit). At spawn time Akagi merges manifest
defaults with the saved values, writes the result to a JSON file, and points
**`AKAGI_BOT_CONFIG`** at its absolute path. Read it on startup:

```python
import json, os

cfg = {}
path = os.environ.get("AKAGI_BOT_CONFIG")
if path:
    with open(path) as f:
        cfg = json.load(f)          # {"temperature": 1.0, "style": "balanced", ...}
```

A bot with no `manifest.toml` simply gets no `AKAGI_BOT_CONFIG` and no settings
panel — perfectly fine for a no-knobs bot.

## Registering the bot

1. Drop the folder under `mjai_bot/<name>/`. Akagi rescans on each game start
   and whenever you open or **Refresh** the Bots tab, so a freshly added bot is
   picked up without a restart.
2. **Build its Python environment.** If the bot has a `pyproject.toml`, its row
   shows an **Install environment** button until the env is built — click it to
   run `uv sync` once (a possibly slow, one-time step). The activation toggle
   stays disabled until the environment is ready, so a game never picks a bot
   that would need a slow `uv sync` mid-match. Editing `pyproject.toml` later
   marks the env stale and the button returns — click it again to rebuild.
   (A bot with no `pyproject.toml` skips this step entirely.)
3. **Activate it.** Toggle the bot on for 4-player and/or 3-player games
   (`bot.active_4p` / `bot.active_3p` — the two slots are independent; leave one
   empty to run that mode analysis-only). You can rebuild the env any time later
   via **Configure → Reinstall environment**.

This local-folder flow is the fastest way to iterate while developing. Bots can
also be installed straight from a GitHub release or a local `.zip` via the
**Bots** tab — those paths build the environment automatically as part of the
install. See [`../src/bot/README.md`](../src/bot/README.md) for that flow.

## AGPL boundary

Bots run as a **separate OS subprocess** and communicate strictly via JSONL
over stdin/stdout — no in-process linking, no shared address space, no FFI.
This is a deliberate license boundary: an AGPL-licensed bot stays inside its
own process, so dropping it under `mjai_bot/<name>/` does **not** make Akagi a
derived work of the bot.
