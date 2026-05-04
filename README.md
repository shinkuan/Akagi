<!-- markdownlint-disable MD033 MD041 -->

<br/>

<p align="center">
  <!-- Icon in design — replace src once asset is ready. -->
  <img src="https://github.com/shinkuan/RandomStuff/assets/35415788/db94b436-c3d4-4c57-893e-8db2074d2d22" width="50%">
</p>

<h1 align="center">Akagi</h1>

<p align="center">
  <i>「死ねば助かるのに………」 - 赤木しげる</i>
  <br/><br/>
  Real-time mahjong AI assistant for <b>Mahjong Soul</b>, <b>Tenhou</b>, and more.<br/>
  Akagi V3: A single-binary Rust + Tauri rewrite of
  <a href="https://github.com/shinkuan/Akagi/tree/v2">Akagi</a> and
  <a href="https://github.com/Xe-Persistent/Akagi-NG">AkagiNG</a>.
  <br/><br/>
  <a href="https://discord.gg/Z2wjXUK8bN">Ask anything on Discord</a>
  ·
  <a href="https://github.com/shinkuan/Akagi/issues">Report Bug</a>
  ·
  <a href="https://github.com/shinkuan/Akagi/issues">Request Feature</a>
  ·
  <a href="https://deepwiki.com/shinkuan/Akagi">DeepWiki</a>
</p>

<p align="center">
  <a href="https://github.com/shinkuan/Akagi/stargazers"><img src="https://img.shields.io/github/stars/shinkuan/Akagi?logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/shinkuan/Akagi/releases"><img src="https://img.shields.io/github/v/release/shinkuan/Akagi?label=release&logo=github&include_prereleases" alt="Latest release" /></a>
  <a href="https://github.com/shinkuan/Akagi/issues"><img src="https://img.shields.io/github/issues/shinkuan/Akagi?logo=github" alt="Open issues" /></a>
  <a href="./LICENSE.txt"><img src="https://img.shields.io/badge/license-Apache%202.0-blue?logo=apache" alt="License: Apache-2.0" /></a>
  <a href="https://github.com/shinkuan/Akagi/actions/workflows/release.yml"><img src="https://img.shields.io/github/actions/workflow/status/shinkuan/Akagi/release.yml?branch=v3&logo=githubactions&label=build" alt="Build status" /></a>
  <a href="https://discord.gg/Z2wjXUK8bN"><img src="https://img.shields.io/discord/1192792431364673577?label=discord&logo=discord&color=7289DA" alt="Discord" /></a>
  <a href="https://deepwiki.com/shinkuan/Akagi"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki" /></a>
</p>

<p align="center">
  Other branches:
</p>

<p align="center">
  <a href="https://github.com/shinkuan/Akagi/tree/v2"><img src="https://img.shields.io/badge/Akagi-v2_(Python)-blue?logo=github" alt="v2 branch" /></a>
  <a href="https://github.com/Xe-Persistent/Akagi-NG"><img src="https://img.shields.io/badge/Akagi-NG_(Electron)-blue?logo=github" alt="NG branch" /></a>
</p>

<p align="center">
  <b>English</b>
  ·
  <a href="./README.zh-TW.md">繁體中文</a>
  ·
  <a href="./README.zh-CN.md">简体中文</a>
</p>

---

## About

> The purpose of this project is to provide a convenient way to understand
> your performance in mahjong matches in real time and to learn from it.
> This project is intended for **educational purposes only**. The author is
> not responsible for any actions taken by users. Game developers and
> publishers reserve the right to act against users who violate their
> terms of service; any consequences (account suspension, etc.) are the
> user's responsibility.

Akagi watches your Mahjong Soul / Tenhou game over a local proxy or a
built-in browser, mirrors the game state, and shows **shanten**, **waits**,
**agari rate**, **tenpai rate**, **per-opponent deal-in risk**, and a
**recommended discard** in a draggable HUD. Drop in an mjai-protocol bot
like Mortal and the HUD also shows the bot's recommendation each turn.

## Screenshots

<img width="2559" height="1439" alt="image" src="https://github.com/user-attachments/assets/da9e7cce-d8ef-4e6e-807b-f6f54013cf22" />

https://github.com/user-attachments/assets/42812e85-ccf0-49fd-b825-adbb5b7b58b0

https://github.com/user-attachments/assets/2ce7cb71-8b25-4895-a12b-0a638665dcab

---

## Table of Contents

**For users**
- [Features](#features)
- [Supported Platforms](#supported-platforms)
- [Quick Start](#quick-start)
- [Configuration](#configuration)
- [Bots](#bots)
- [Game History](#game-history)
- [Logs &amp; Diagnostics](#logs--diagnostics)
- [Troubleshooting](#troubleshooting)
- [Roadmap](#roadmap)

**For developers**
- [Architecture](#architecture)
- [Tech Stack](#tech-stack)
- [Project Layout](#project-layout)
- [Build From Source](#build-from-source)
- [Testing](#testing)
- [Releases &amp; CI](#releases--ci)
- [Reference Materials](#reference-materials)
- [License &amp; Attribution](#license--attribution)
- [Acknowledgements](#acknowledgements)

---

## Features

- **Live HUD** — shanten, waits, agari rate, tenpai rate, per-opponent
  deal-in risk, suggested attack/defence discard. Draggable, resizable
  tile grid persisted to local storage.
- **Two capture modes**
  - **MITM proxy** (default) — system-wide; needs a one-time CA trust.
  - **Chromium** — Akagi launches a controlled Chromium-family browser
    and intercepts WebSocket frames via the Chrome DevTools Protocol.
    Zero proxy/CA setup; just play in the launched window.
- **Pluggable mjai bots** — install Mortal in one click from the Setup
  wizard, or drop any `bot.py` under `mjai_bot/<name>/`. Per-mode
  routing: `bot.active_4p` and `bot.active_3p` swap automatically based
  on the table's player count.
- **Game history** — every completed match is auto-recorded. The
  History tab shows a rank pie chart, a cumulative PT line chart with
  selectable scoring rules (Mahjong Soul tiers / Tenhou ranks /
  Custom uma), and detailed stats (win rate, deal-in rate, riichi rate,
  fuuro rate, ryukyoku rate, average winning / deal-in points, average
  winning turn, yakuman / nagashi-mangan counts).
- **Logs viewer** — **Diagnostic** tab for the application log with
  live tail and per-module filtering; **Inspector** tab for raw
  WebSocket frames → mjai events → bot reactions, with frame counts
  and meta inspection.
- **First-run Setup wizard** — language → platform → capture mode →
  CA trust / Chromium pick → bot install → done.
- **Internationalization** — English, 日本語, 繁體中文, 简体中文.
  Live switch from Setup or the Sidebar. Full coverage across the UI.
- **Sanma (3-player)** — full pipeline: bridge, tracker, snapshot,
  analysis, per-mode bot routing, history stats, 3p uma tables.

## Supported Platforms

| Platform | 4-Player | 3-Player | AutoPlay |
|---|:---:|:---:|:---:|
| **Mahjong Soul (Majsoul)** | &check; | &check; | (planned) |
| **Tenhou** | &check; | &check; | &cross; |
| **Riichi City** | (planned) | (planned) | &cross; |
| **Amatsuki** | (planned) | (planned) | &cross; |

---

## Quick Start

### A. Install a release

Akagi ships as a portable zip — one self-contained folder per platform.
Download the file for your OS from
[Releases](https://github.com/shinkuan/Akagi/releases), unzip anywhere
you have write permission (e.g. `~/Apps/`, Desktop), and run the binary.
Configuration, logs, history, the CA cert, and bots all live next to
the binary, so moving / backing up / uninstalling is just moving /
copying / deleting the folder.

| OS | File | Notes |
|---|---|---|
| Windows | `akagi-<version>-windows-x64.zip` | x86_64. Requires WebView2 (preinstalled on Win10 1803+ / Win11). SmartScreen will warn — *More info → Run anyway*. |
| macOS | `akagi-<version>-macos-arm64.zip` | Apple Silicon. Unsigned: run `xattr -cr <unzipped folder>` once, or right-click → *Open* the first time. |
| Linux | `akagi-<version>-linux-x64.zip` | Built on `ubuntu-22.04` (glibc 2.35+). Requires WebKit2GTK 4.1 (`apt install libwebkit2gtk-4.1-0` / `dnf install webkit2gtk4.1` / `pacman -S webkit2gtk-4.1`). |

Each zip bundles `python-build-standalone` 3.12 + `uv` next to the
binary, so bots run out of the box without any system Python install.

On first launch the **Setup wizard** walks you through language,
platform, capture mode, optional bot install (Mortal), and CA trust
(only if you choose MITM mode).

### B. Chromium mode (no CA trust needed)

The simplest path. After Setup:

1. Settings → **Capture** → set Mode to **Chromium**.
2. Click **Detect** to auto-find Chrome / Edge / Brave / Chromium, or
   set `capture.chromium.executable` manually.
3. Akagi launches the browser with an isolated profile under
   `<config_root>/chrome-profile`. Log in to Mahjong Soul and play.

Frames are intercepted via the Chrome DevTools Protocol — no system
proxy, no certificate.

### C. MITM mode

System-wide proxy with a self-signed root CA at `./ca/`:

1. Trust `./ca/akagi-ca.crt` (or `.cer` / `.pem` / `.der`) in your
   OS / browser certificate store.
2. Route the game client through `127.0.0.1:23410`.
   Health probe: `GET /ping` → `pong`.
3. On Windows, [Proxifier](https://www.proxifier.com/) is the usual
   way to redirect a specific application to the proxy.

---

## Configuration

Configuration lives in `config.toml` next to the binary (or wherever
you point `--config`). Edits saved through the Settings UI hot-reload
the affected subsystem — capture / proxy / bot active slots restart
without an app relaunch.

```toml
[general]
language = "en"

[logging]
dir       = "./logs"
level     = "info"
all_level = "warn"

[platform]
kind = "Majsoul"

[proxy]
enabled = true
addr    = "127.0.0.1:23410"
ca_dir  = "./ca"

[capture]
mode = "mitm"               # or "chromium"

[capture.chromium]
executable    = ""          # blank = auto-detect
user_data_dir = ""          # blank = <config_root>/chrome-profile
start_url     = "https://game.maj-soul.com/1/"
cft_channel   = "stable"
force_cft     = false
extra_args    = []

[bot]
enabled   = true
active_4p = "mortal"        # used in 4-player (yonma) games
active_3p = "mortal3p"      # used in 3-player (sanma); empty = none
auto_sync = true
dir       = "./mjai_bot"
```

<details>
<summary>Where the config file lives (resolution order)</summary>

1. `--config <path>` CLI flag.
2. `<exe_dir>/configs/config.toml`.
3. `./configs.toml` in the current working directory.
4. If none of the above exist, defaults are auto-written to
   `<exe_dir>/configs/config.toml` on first launch.

Pre-3p configs that still use a single `active = "..."` key are
auto-migrated into `active_4p` on load.
</details>

---

## Bots

### Install a bot

The Setup wizard or the **Bots** tab can install bots straight from a
GitHub release:

- Repo: `shinkuan/Akagi-MjaiBot-Mortal`
- Asset (4P): `release4p.zip`
- Asset (3P): `release3p.zip`

The IPC command `install_bot_from_github(repo, asset_glob?, name?)`
fetches the latest release zip, extracts it under `mjai_bot/<name>/`,
validates `bot.py`, and runs `uv sync` once. Subsequent launches are
fast — the sync is gated by a stamp at
`mjai_bot/<name>/.akagi/synced.stamp`.

> [!IMPORTANT]
> Because of GitHub's file-size limit, the Mortal weights bundled in
> the release zip are a small, weak **placeholder model** — useful to
> verify the install works, **not recommended for real play**.
> **Stronger Mortal model weights** and an **online API-server model**
> (a hosted, even stronger model — point your bot at the server and
> an API key; no local NN needed) are both distributed through the
> [Discord server](https://discord.gg/Z2wjXUK8bN). Ask there for
> access; both 4P and 3P versions are available.

### Per-mode bots

`bot.active_4p` and `bot.active_3p` are independent. Akagi picks the
right one when the game starts, based on the table's player count.
Leave a slot empty to play that mode with **analysis only** (no bot
suggestion).

### Write your own

```
mjai_bot/<name>/
├── bot.py            # JSONL stdin → JSONL stdout
├── pyproject.toml    # requires-python = ">=3.12"
├── manifest.toml     # optional — supported_modes, settings schema
└── README.md
```

`bot.py` reads one JSON array of mjai events per line and writes one
mjai action object per line (`{"type":"none"}` when no action is owed).
Akagi pumps stderr into the application log under `bot=<name>`.

See [`src/bot/README.md`](./src/bot/README.md) for the full contract,
the manifest schema, and the secret-field handling.
[`mjai_bot/example/`](./mjai_bot/example/) is a working rule-based
reference that ships in tree.

### AGPL boundary

Bots run as a **separate OS subprocess** spawned by Akagi. Communication
is strictly JSONL over stdin / stdout — no in-process linking, no
shared address space, no FFI. This is an intentional license boundary:
an AGPL-licensed bot (e.g. Mortal, which links libriichi) stays inside
its own process, so dropping it under `mjai_bot/<name>/` does **not**
make Akagi a derived work of the bot.

---

## Game History

Every cleanly-ended match (one that produced an `end_game` mjai event)
is persisted under `<config_root>/history/`:

```
<config_root>/history/
├── index.jsonl              # one GameRecord per line (ULID-keyed)
└── games/
    └── <ulid>.mjai.jsonl    # full event-stream copy
```

Mid-game disconnects leave an unfinalised buffer and are silently
dropped — only complete games make it to disk.

The frontend's **History** tab shows:

- **Rank pie chart** — 1st / 2nd / 3rd / 4th distribution
  (3 slices for sanma).
- **Cumulative PT line chart** — selectable scoring rule:
  - **Mahjong Soul**: pick `場次` (銅 / 銀 / 金 / 玉 / 王座) and
    `段位` (初心 1 星 → 魂天).
  - **Tenhou**: pick `段位` (新人 → 天鳳位 across 21 ranks).
  - **Custom**: edit the uma + dan-bonus arrays directly.
  Switching rule / dan re-renders immediately — no backend round-trip.
- **Detailed stats** — win rate, deal-in rate, riichi rate, fuuro rate,
  ryukyoku rate, average winning / deal-in points, average winning
  turn, yakuman / nagashi-mangan counts.
- **Game list** — filterable by platform / players / east-or-south /
  date. Click a row for final standings + per-game stats; the trash
  icon deletes both the index entry and the per-game `.mjai.jsonl`.

PT-rule and filter selections persist to `localStorage`. Records load
from the backend on bridge boot and stay current via the
`history-recorded` Tauri event.

See [`src/history/README.md`](./src/history/README.md) for the math,
the storage schema, and how to add a new platform / stat field /
filter dimension.

---

## Logs & Diagnostics

Per-session logs land under `<log_dir>/<YYYYMMDD-HHMMSS>/`:

```
<log_dir>/<session>/
├── all.log                       # combined tracing output
├── <target>.log                  # per-module filtered logs
├── proxy.binlog                  # raw binary WS frames
├── majsoul/<flow_id>.log         # per-WebSocket flow JSON log
├── majsoul/<flow_id>.mjai.jsonl  # per-game mjai event stream
└── inspector.jsonl               # frames seen by the Inspector
```

The frontend's **Logs** route has two tabs:

### Diagnostic

Filterable application log. Filter by level (trace / debug / info /
warn / error) and by module. Live-tail or browse past sessions; click
a row to see source location + raw structured fields. An **Open
Folder** button reveals the session directory in the OS file manager.

### Inspector

Protocol-level frame viewer. Three entry types:

- **WS Frame** — raw binary (base64-truncated) plus the bridge's
  first-pass parse.
- **MjaiEvent** — decoded events flowing to the bot.
- **BotReaction** — bot responses with the `meta` field
  (confidence / q-values / whatever the bot emits).

Frame counts show how many mjai events each WS frame produced.
Useful when debugging a bot or a bridge issue.

---

## Troubleshooting

> [!TIP]
> Reproduce the problem, then save the session folder under
> `<log_dir>/<session>/` — it has everything (app log, raw frames,
> mjai events, bot meta) needed to file a useful bug report.

- **Capture not working in MITM mode.** Make sure the CA at
  `./ca/akagi-ca.crt` is trusted in your OS store. Verify the proxy is
  running: `curl http://127.0.0.1:23410/ping` should reply `pong`.
  Check your proxy redirector (Proxifier / system proxy) is sending
  the game client to the right host:port.
- **Capture not working in Chromium mode.** Detect did not find your
  browser. Set `capture.chromium.executable` manually in Settings or
  `config.toml`. If the launched browser starts but no frames flow,
  check that `--remote-debugging-port` was not blocked by another
  extension.
- **Bot stuck in `Loading{SyncingDeps}`.** First-run `uv sync` is
  slow — watch the Diagnostic tab for `bot=<name>` lines. If it never
  finishes, delete `mjai_bot/<name>/.akagi/synced.stamp` and retry.
- **Bot crashed mid-game.** The Inspector tab shows the last frame the
  bot saw before dying; attach it to the bug report.
- **Wrong bot picked for a 3-player game.** Check `bot.active_3p` in
  Settings → Bot — it is independent of `bot.active_4p`.
- **Where do I get help?** [Discord](https://discord.gg/Z2wjXUK8bN)
  for chat, [GitHub Issues](https://github.com/shinkuan/Akagi/issues)
  for tracked bugs and feature requests.

---

## Roadmap

Done in alpha.8:

- [x] 3-player mahjong (sanma) — full pipeline
- [x] Tenhou bridge (observe-only)
- [x] Game history persistence + History tab (rank pie / PT chart / stats)
- [x] Logs viewer (Diagnostic + Inspector tabs)
- [x] i18n: en / ja / zh-TW / zh-CN, with Setup-wizard language picker
- [x] Bot install from a GitHub release
- [x] Chromium capture mode (no CA trust needed)

Planned:

- [ ] **Riichi City** platform support
- [ ] **Amatsuki** platform support
- [ ] **Custom themes** (frontend theming hooks)
- [ ] **AutoPlay** (Mahjong Soul first; the bot drives the table
      autonomously, like the original Akagi's Windows AutoPlay)
- [ ] **Refine Frontend** — tile layout, animations, accessibility
- [ ] **Tenhou autoplay** (currently observe-only)

Detailed bug tracking lives in
[GitHub Issues](https://github.com/shinkuan/Akagi/issues).

---
---

## Architecture

Single Rust binary. Subsystems own only their bus handles, never each
other. [`src/event_bus.rs`](./src/event_bus.rs) is the single source of
truth for channel types.

```
                ┌────────────────────────┐
   Game client ─│  capture (mitm | cdp)  │── CA at ./ca (mitm only)
   WebSocket    └─────────┬──────────────┘
                          ▼
                ┌────────────────────────┐
                │  bridge::<platform>    │   wire bytes → MjaiEvent
                └─────────┬──────────────┘
                          ▼ MjaiBus
       ┌──────────────────┼──────────────────┐
       ▼                  ▼                  ▼
  game_state::tracker   bot::manager     ipc forwarder
       │                  │                  │
       ▼ PostBus          ▼ BotResponseBus   ▼ app.emit
  analysis::runner   subprocess (uv)    Tauri webview
       │
       ▼ AnalysisBus
       └──► ipc forwarder ──► app.emit
```

[`src/lib.rs`](./src/lib.rs) wires the buses on boot. The frontend
talks to the backend over six push events (`mjai-event`, `bot-response`,
`bot-status`, `proxy-status`, `notify`, `history-recorded`) and a set
of pull commands documented in [`src/ipc/README.md`](./src/ipc/README.md).

## Tech Stack

| Layer | Tech |
|---|---|
| Shell | [Tauri](https://tauri.app) 2 |
| Backend | Rust (edition 2021), `tokio`, `tracing`, `clap` |
| MITM | [`hudsucker`](https://crates.io/crates/hudsucker) 0.24 (`rcgen-ca`, `rustls-client`) |
| CDP capture | [`chromiumoxide`](https://crates.io/crates/chromiumoxide) 0.9 |
| Mahjong engine | [`riichienv-core`](https://github.com/smly/RiichiEnv) 0.4 |
| Protobuf | `prost` 0.14 + `prost-reflect` 0.16 |
| Frontend | [React](https://react.dev) 19, TypeScript, [Vite](https://vitejs.dev) 8 |
| Styling | [Tailwind CSS](https://tailwindcss.com) v4, [shadcn/ui](https://ui.shadcn.com) (Radix Nova preset) |
| State | [Zustand](https://github.com/pmndrs/zustand) |
| Charts | [Recharts](https://recharts.org) |
| Tile rendering | [`<mah-gen>`](https://github.com/eric200203/mahgen) Web Component |
| i18n | [react-i18next](https://react.i18next.com) |
| Bot runtime | `python-build-standalone` 3.12 + [`uv`](https://github.com/astral-sh/uv) (bundled per platform) |

## Project Layout

```
.
├── src/
│   ├── analysis/      Shanten / waits / agari-rate / risk / discard search
│   ├── bot/           Registry, Python runtime, JSONL subprocess runner
│   ├── bridge/        Per-platform protocol → MjaiEvent
│   │   ├── majsoul/   Mahjong Soul (liqi protobuf)
│   │   └── tenhou/    Tenhou (JSON tag stream, observe-only)
│   ├── capture/       Capture backends abstraction (mitm | chromium)
│   ├── config/        AppConfig (TOML) sections + resolution
│   ├── event_bus.rs   Broadcast channels between subsystems
│   ├── game_state/    riichienv-driven mirror, snapshot, mahgen view
│   ├── history/       Game replay storage + index
│   ├── inspector/     Frame / event / bot-reaction broadcaster
│   ├── ipc/           Tauri commands, app state, capture supervisor
│   ├── logger/        Per-session log dir + per-target file appenders
│   ├── proxy/         MITM HTTP/HTTPS/WS via hudsucker; CA at ./ca
│   ├── schema/        MjaiEvent enum + IPC payload types
│   └── lib.rs         Boot / wiring
├── mjai_bot/
│   └── example/       Rule-based shanten optimizer (ships in tree)
├── frontend/          React + Vite + Tailwind + shadcn UI
│   └── src/
│       ├── routes/    Overview / GameDashboard / Bots / History / Logs / Settings / Setup / InspectorView / DiagnosticView
│       ├── tiles/     Dashboard tiles (header, hands, opponents, analysis, …)
│       ├── stores/    Zustand slices (game, analysis, bot, proxy, notify, layout, config)
│       └── i18n/      en / ja / zh-TW / zh-CN
├── tests/             Integration tests
├── capabilities/      Tauri permissions
├── icons/             App icons
├── tauri.conf.json    Window + bundle config
└── Cargo.toml
```

Per-module developer guides live in each `src/*/README.md`.

## Build From Source

**Prerequisites**

- Rust (latest stable, 1.80+)
- Node.js 20+ and npm
- Tauri 2 system deps:
  - **Linux**: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
    `libayatana-appindicator3-dev`, `librsvg2-dev`,
    `protobuf-compiler`
  - **macOS**: Xcode Command Line Tools
  - **Windows**: WebView2 (preinstalled on Windows 11)

**Run / build**

```bash
# Debug — launches the GUI; Vite dev-server proxied by Tauri
cargo run

# Pass a custom config path
cargo run -- --config ./my-config.toml

# Build a portable zip for the current target
cargo install tauri-cli --locked          # if not already installed
bash scripts/fetch-runtime.sh             # populate runtime/<triple>/
cargo tauri build --no-bundle             # writes target/<triple>/release/akagi
bash scripts/package-zip.sh <target-triple>
# → dist/akagi-<version>-<os>-<arch>.zip

# Frontend dev only (Vite on :1420)
cd frontend && npm ci && npm run dev
```

**Bundled runtime**

`scripts/fetch-runtime.sh <target-triple>` downloads
`python-build-standalone` 3.12 + `uv` for the target and stages them
under `runtime/`. `scripts/package-zip.sh` then copies that tree next
to the binary inside the zip; `src/bot/runtime.rs` finds it
exe-adjacent at runtime, so the shipped app works without a system
Python install.

## Testing

Integration tests live in [`tests/`](./tests/):

| File | Covers |
|---|---|
| `analysis_pipeline.rs` | End-to-end analysis (events → shanten → discard recommendation) |
| `analysis_bench.rs` | Hot-path performance |
| `bot_lifecycle.rs` | Install → sync → spawn → roundtrip |
| `example_bot.rs` | Rule-based reference bot driving a synthetic game |
| `mortal_zip_layout.rs` | Validates the Mortal release-zip layout |

```bash
cargo test               # all tests, incl. integration
cargo test --release     # for the perf bench
```

## Releases & CI

GitHub Actions [`release.yml`](./.github/workflows/release.yml) builds
on tag push (`v3.*`) or manual dispatch. One portable zip per target:

| OS runner | Target | Artifact |
|---|---|---|
| `ubuntu-22.04` (glibc 2.35) | `x86_64-unknown-linux-gnu` | `akagi-<version>-linux-x64.zip` |
| `macos-14` | `aarch64-apple-darwin` | `akagi-<version>-macos-arm64.zip` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `akagi-<version>-windows-x64.zip` |

Each zip ships `python-build-standalone` 3.12 + `uv` next to the
binary, so bots run without a system Python install.

Tags must be on the `v3` branch.

## Reference Materials

| Source | Used in | What for |
|---|---|---|
| [mjai JSONL spec (Gimite)](https://gimite.net/pukiwiki/index.php?Mjai%20%E9%BA%BB%E9%9B%80AI%E5%AF%BE%E6%88%A6%E3%82%B5%E3%83%BC%E3%83%90) | `src/schema/mjai/` | `MjaiEvent` enum + bot wire contract — 15 event types, tile-string format, state-machine rules. |
| [`EndlessCheng/mahjong-helper`](https://github.com/EndlessCheng/mahjong-helper) (Go analysis CLI) | `src/analysis/` | Direct Rust port of `util/` — shanten, waits, agari-rate, tenpai-rate, risk model, discard search. |
| [`Xerxes-2/MajsoulMax-rs`](https://github.com/Xerxes-2/MajsoulMax-rs) (Rust MITM proxy, **GPL-3.0**) | `src/proxy/handler.rs`, `src/bridge/majsoul/parser.rs`, `src/bridge/majsoul/proto/liqi.proto` | Reference for the 5-layer Mahjong Soul WS wire format (type byte → Wrapper → inner message → action protobuf). **Format only — no code copied.** |
| [`smly/RiichiEnv`](https://github.com/smly/RiichiEnv) (Rust RL env w/ Python bindings) | `Cargo.toml` (`riichienv-core` dep), `src/analysis/`, `src/game_state/` | Tile / hand / shanten / yaku / score primitives + game-state model. The analysis engine and game tracker are built on this. |
| [`eric200203/mahgen`](https://github.com/eric200203/mahgen) (mahjong-tile rendering DSL) | `src/game_state/mahgen_view.rs`, frontend `<mah-gen>` | DSL syntax for pre-encoding hand / meld / river strings backend-side. |
| [`smly/mjai.app`](https://github.com/smly/mjai.app) (mahjong AI competition platform) | `mjai_bot/`, `src/bot/` | Bot subprocess convention — JSONL stdin/stdout, argv `python bot.py <player_id>`, `AKAGI_PLAYER_ID` env, end-of-batch flush points. |
| [`shinkuan/Akagi`](https://github.com/shinkuan/Akagi/tree/v2) | Architecture / behaviour parity | The original feature set we are reproducing: MITM proxy, mjai bridge, pluggable bots, recommendation HUD. |

## License & Attribution

Akagi v3 is licensed under the [Apache License 2.0](./LICENSE.txt).
Copyright 2026 Shinkuan. Third-party attributions live in
[`NOTICE`](./NOTICE) — read it alongside the license. Per
Apache-2.0 §4(d), redistributions must include both files.

**Bundled / linked sources**

- **mahjong-helper** (MIT) — `src/analysis/` is a Rust port of `util/`.
- **riichienv-core** / RiichiEnv (Apache-2.0) — Cargo dependency.
- **mahgen** (MIT) — DSL + `<mah-gen>` custom element.

**Reference-only** (no code copied; listed in `NOTICE` for credit)

- **MajsoulMax-rs** (GPL-3.0) — Mahjong Soul WS wire format reference only.
- **mjai spec** (Gimite) — bot wire contract.
- **mjai.app** — bot subprocess convention.

## Acknowledgements

- [Akagi](https://github.com/shinkuan/Akagi/tree/v2) (Python, v2) and
  [AkagiNG](https://github.com/Xe-Persistent/Akagi-NG) (Electron + Python) —
  the predecessors this v3 is built on.
- [`mjai.app`](https://github.com/smly/mjai.app) and the mjai
  specification by Gimite — the protocol that makes pluggable bots
  possible.
- The community on [Discord](https://discord.gg/Z2wjXUK8bN) for bug
  reports, model contributions, and feedback.
