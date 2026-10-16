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

## HTTP capture

Both backends record the HTTP traffic they intercept, not just WebSocket
frames. Before this, everything except WS was discarded, which hid the
game's own HTTP entirely — route topology, version and CDN endpoints, and
the analytics beacons through which the client reports on itself.

`http.rs` holds the policy both backends share: what to keep, what to
skip, and how to say so. Nothing in it understands any protocol above
HTTP; meaning is added separately by `crate::inspector::annotate`.

Two rules are load-bearing:

- **A skipped body records why.** A timeline that looks the same whether a
  body was empty or merely dropped is the blind spot this exists to
  remove.
- **Bodies are decided from headers alone**, before a byte is read, so a
  multi-megabyte asset is never buffered and a chunked response never
  stalls the forward. Akagi forwards traffic untouched; buffering is the
  one thing that could change that.

### What the two backends see differently

| | MITM | Chromium |
|---|---|---|
| volume | a whole session is ~23 requests | every page subresource; static assets are filtered out by default |
| bodies | buffered within the cap | not captured — needs a separate `Network.getResponseBody` round-trip per request |
| pairing | best-effort FIFO per connection; unpaired under HTTP/2 (no stream id in hudsucker's context) | exact — CDP hands out a real request id |
| blind spots | raw-tunneled `CONNECT` (recorded as an `akagi_bypass` annotation); cleartext non-`GET` inside a tunnel | anything the page does not route through the network stack |

Configured under `[capture.http]`. `record_all` is **off** by default:
full capture puts access tokens and cookies in the session file and
nothing is redacted, so the default keeps only exchanges a recognizer
understood. See `src/config/README.md`.

## Telemetry blocking

The game clients report on themselves through Aliyun SLS web-tracking
beacons. Both backends keep them from leaving the machine, each under its
own switch (default **on**), and both decide what counts as telemetry with
`telemetry.rs::is_sls_host` — the `log.aliyuncs.com` service domain:

- **MITM** — `[proxy] block_telemetry`. The proxy refuses the `CONNECT` to
  the host. See `src/proxy/README.md`.
- **Chromium** — `[capture.chromium] block_telemetry`. `cdp.rs` arms the
  `Fetch` domain on every page with request-stage patterns for the domain
  (one `Fetch.enable` alongside the Tenhou client rewrite, since each call
  replaces the patterns of the last) and fails each paused request whose
  host passes `is_sls_host` as `BlockedByClient` — the
  `net::ERR_BLOCKED_BY_CLIENT` an ad blocker produces. Anything the globs
  catch that is not an SLS host is continued untouched.

Page-scoped interception covers more than the page's own requests: the
CORS preflight the beacon XHR triggers (which carries the whole payload in
its URL) and requests from the page's dedicated workers are paused too.
Each blocked request is logged under `akagi::capture::http`, and the
page's own requests go on the timeline with an `akagi_blocked` annotation
next to their `sls_beacon` one. Worker requests are blocked but do not
appear on the timeline — page-scoped `Network` events do not report them.

The window it cannot cover is a tab's first second: interception is armed
when the page poll attaches to the tab, and requests made before that go
out. The game fires its first beacon at login, well after.

`tests/chromium_telemetry_block.rs` drives the real CDP loop against a
headless browser and asserts no beacon — preflight, image, `sendBeacon`,
worker — reaches the wire. It needs a browser, so it is `#[ignore]`d; run it
with `cargo test --test chromium_telemetry_block -- --ignored`.

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
