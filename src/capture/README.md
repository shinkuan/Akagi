# Capture Module

`crate::capture` is the transport layer that supplies binary WebSocket
frames to `crate::bridge::Bridge` (the protocol parser). Two backends
share one trait so the rest of the app — bot manager, game tracker,
analysis runner, IPC forwarders — never sees which capture mode is
running.

## Backends

- **`hudsucker_backend::HudsuckerBackend`** — Wraps the existing
  `crate::proxy::start_proxy`. The user routes traffic through the MITM
  proxy by setting their system proxy and trusting the generated CA.
- **`chromium::ChromiumBackend`** — Launches a Chromium-family browser
  with `--user-data-dir=<our profile>` and intercepts WebSocket frames
  via the Chrome DevTools Protocol. No proxy / CA setup.

Both implement `CaptureBackend`:

```rust
#[async_trait]
pub trait CaptureBackend: Send {
    async fn run(self: Box<Self>, ctx: CaptureCtx, shutdown: ShutdownToken) -> Result<()>;
    fn descriptor(&self) -> CaptureDescriptor;
}
```

`CaptureCtx` carries the buses and session. `ShutdownToken` is a
cooperative cancellation handle wired to the supervisor.

## Adding a new backend

1. Create `src/capture/<name>_backend.rs` (or a sub-module if it needs
   more than one file).
2. Implement `CaptureBackend`. Use `flow::FlowBridges<K>` to manage one
   `Bridge` instance per WS flow — `K` is whatever uniquely identifies
   a flow on your transport.
3. Surface a config section if needed (`src/config/`), and add a
   `CaptureMode` variant.
4. Wire it into `src/ipc/capture_supervisor.rs::spawn_capture_supervisor`
   alongside the existing match arms.

## Why two backends, not one trait + N parsers

The `Bridge` trait already abstracts protocols (Majsoul, future
Tenhou…). What this module abstracts is *transport* — the same Majsoul
parser runs against either MITM-intercepted bytes or CDP-intercepted
bytes. Don't reimplement parsing per backend; route frames into the
existing platform bridge.

## Frame routing

```
hudsucker WS event ─┐
                    ├─→ FlowBridges::acquire(K) ─→ Bridge::parse(dir, &bytes) ─→ MjaiBus
CDP frame event   ─┘
```

`FlowBridges<K>` is in `flow.rs`; both backends use it identically.
Lazy create on first frame, ref-count clean-up on close.

## Path conventions

For data the backend writes at runtime (Chromium profile, downloaded
Chrome-for-Testing install, etc.):

- Use `crate::util::resolve_dir(Path::new("./<name>"))` — exe-adjacent
  first, with an AppImage / user-config fallback baked in. This keeps
  the portable zip distribution single-folder: the chrome profile and
  the downloaded CfT browser sit next to the binary alongside `logs/`,
  `history/`, `ca/`, `mjai_bot/`, so moving / backing up / removing the
  app is one folder operation.
- `crate::util::user_subdir(name)` is still available for callers that
  *must* use the OS user-config dir regardless of where the binary
  lives. None of the capture code currently needs that — `resolve_dir`
  already routes to user-config under AppImage and similar read-only
  mounts.

## Phasing

Phase 1 (this commit): trait, system Chrome detect, basic CDP frame
capture, supervisor multiplexing, Settings UI toggle.

Phase 2 (planned): Chrome-for-Testing manifest fetch + download +
extract, macOS quarantine strip, Windows + macOS smoke testing.

Phase 3 (planned): first-run wizard, `Snapshot.capture_status`
migration, removal of `start_proxy`/`stop_proxy` aliases.
