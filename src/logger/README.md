# Logger Module

Structured text + binary logging built on [`tracing`](https://crates.io/crates/tracing) and [`tracing-appender`](https://crates.io/crates/tracing-appender).

## Files

- `mod.rs` — `init(log_root, default_level, all_level, targets)` entry point.
- `session.rs` — `Session` (active log session + binary logger registry) and `LogTarget`.
- `binary.rs` — `BinaryLogger` for raw byte streams (e.g. captured WS frames).
- `flow.rs` — `FlowLogger` for one text log file per logical "flow" (e.g. one Majsoul WS connection).
- `pack.rs` — `pack()` bundles sessions into one zip for a bug report (the Logs page's "Pack logs").

## Session layout

`logger::init` creates a session directory:

```
<log_root>/<YYYYMMDD-HHMMSS>/
├── all.log              # every event from every target
├── proxy.log            # one file per LogTarget (filtered by tracing target prefix)
├── inspector.jsonl      # shareable Inspector timeline (see below)
├── majsoul/             # one subdir per platform (FlowLogger)
│   ├── 000001-gateway.log                # one file per WS flow: <id:06>-<uri-slug>.log
│   ├── 000002-game-gateway.log
│   └── majsoul_<ts>.mjai.jsonl          # one file per game: emitted MjaiEvents
└── sensitive/           # may hold account credentials (SENSITIVE_DIR)
    ├── proxy.binlog     # every frame verbatim, via BinaryLogger
    └── inspector.jsonl  # full Inspector timeline; what the Inspector tab reads
```

## Shareable vs. sensitive

Everything outside `sensitive/` is meant to be shareable. Frame content gets
there only for frames on the bridge's allowlist (see "Shareable frames" in
`src/bridge/README.md`); any other frame is written as method, direction and
size. Two rules keep it that way when you add logging:

- Don't log a frame payload, a URL query or an HTTP header value unless the
  bridge marked the frame shareable. Use `bridge::shown(shareable, &payload)`
  for payloads and `util::redact_query` for URLs.
- Record inspector entries with `InspectorWriter::record_as(entry, shareable)`
  (or `record_sensitive`). `record` is only for content known to be safe.

`pack::pack` copies the shareable part as is. The `sensitive/` part goes in
only when asked. With a password it goes into an AES-256 encrypted
`sensitive.zip` stored inside the pack, so the pack itself extracts without a
password; its entries keep their `<session>/...` paths. A session without a
`sensitive/` directory predates the split, so all of it counts as sensitive.

`log_root` resolution mirrors `ca_dir` (see `src/proxy/README.md`): exe-adjacent first, then CWD, else create.

## Text log format

Each line carries: timestamp, level, tracing target, source `file:line`, message. ANSI colour is stripped in files. Console (stderr) keeps colour.

File outputs (`all.log`, per-target `*.log`) use a custom `CompactNoSpans` formatter that **omits the parent-span list**. Third-party crates (e.g. `hudsucker`) wrap our handlers in nested `#[instrument]` spans whose rendered prefix is longer than the actual event — the file format drops them. Console keeps the default `Full` formatter so span context stays visible interactively.

The console layer uses `EnvFilter` honouring `RUST_LOG`; if unset, falls back to `default_level` (from `[logging] level`). The combined `all.log` is severity-filtered by `all_level` (from `[logging] all_level`, same `EnvFilter` syntax — e.g. `"info"` or `"akagi=debug,hyper=warn"`) so you can suppress trace/debug noise. Per-target files always capture every event so you can grep historic runs without re-running.

## Adding a new target file

In `lib.rs::run`, append a `LogTarget` to the slice passed to `logger::init`:

```rust
&[
    logger::LogTarget::new("proxy", "akagi::proxy"),
    logger::LogTarget::new("bridge", "akagi::bridge"),
]
```

`prefix` is matched against each event's tracing target (longest-prefix). Module path is the default target, so any `tracing::info!` inside `src/bridge/` lands in `bridge.log`.

## Binary logging

```rust
let bin = session.binary_logger("proxy")?;     // get-or-create sensitive/proxy.binlog
bin.write(0, &bytes);                           // tag 0 = upstream, 1 = downstream (caller convention)
```

Frame format (little-endian):

```
[u64 micros_since_epoch][u8 tag][u32 len][bytes; len]
```

`BinaryLogger::write` swallows write errors via `tracing::warn`. Use `BinaryLogger::log` if you need the `Result`.

## Per-flow text logging

```rust
let flow = session.flow_logger(
    "majsoul",
    "000001-gateway.log",
    "majsoul 127.0.0.1:54170 wss://.../gateway",
)?;
flow.writeln(&serde_json::to_string(&parsed)?);
```

Each call to `Session::flow_logger` opens a fresh file at
`<session>/<subdir>/<file_name>` (subdir auto-created; caller supplies the
full filename including extension, so the same logger handles `.log`,
`.mjai.jsonl`, etc.). The proxy handler generates one per WebSocket
upgrade and hands the resulting `Arc<FlowLogger>` to
`bridge::for_platform`. Every parsed message gets written as a JSON line.
When all references drop (both directions of the WS exit), the file
closes.

`MajsoulBridge` opens an additional `majsoul_<ts>.mjai.jsonl` file every
time it emits a `start_game` event — one file per game on the same flow,
each line a serialized `MjaiEvent`.

## Lifetime

`Session` owns the `tracing-appender` `WorkerGuard`s. Drop it only at app shutdown — dropping flushes + closes the file appenders. `lib.rs` keeps the `Arc<Session>` alive for the full Tauri runtime.
