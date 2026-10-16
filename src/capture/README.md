# Capture Module

`crate::capture` is the transport layer that supplies binary WebSocket
frames to `crate::bridge::Bridge` (the protocol parser). Three backends
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
- **`ingest::IngestBackend`** — Listens on a loopback WebSocket that
  something else pushes the game's frames into: today, the userscript in
  `ingest/akagi.user.js`, running in the user's own browser. No proxy /
  CA setup, no browser launched by Akagi. See [Ingest](#ingest) below.

All implement `CaptureBackend`:

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

## Why N backends, not one trait + N parsers

The `Bridge` trait already abstracts protocols (Majsoul, Tenhou, Riichi
City). What this module abstracts is *transport* — the same Majsoul
parser runs against MITM-intercepted, CDP-intercepted or pushed bytes.
Don't reimplement parsing per backend; route frames into the existing
platform bridge.

## Frame routing

```
CDP frame event   ─┐
                   ├─→ FlowBridges::acquire(K) ─→ Bridge::parse(dir, &bytes) ─→ MjaiBus
ingest frame msg  ─┘
hudsucker WS event ──→ ProxyHandler's own bridge map ─→ Bridge::parse ─→ MjaiBus
```

`FlowBridges<K>` is in `flow.rs`: lazy create on first frame, ref-count
clean-up on close. Chromium and ingest share it; the MITM handler
predates it and keeps an equivalent map keyed by client `SocketAddr`.
`frame.rs` holds the Inspector `ws_frame` recording Chromium and ingest
share — both receive frames as text or base64, which is recorded
verbatim.

## Ingest

`ingest/` is a loopback server (axum) with four routes:

| Route | What |
|---|---|
| `GET /ingest` | WebSocket a source pushes frames into |
| `GET /` | the install page (`install_page.rs`, `?lang=en\|ja\|zh-TW\|zh-CN`) |
| `GET /akagi.user.js` | the userscript, rendered for this address |
| `GET /ping` | `pong` |

**The install page** is what the UI's Install button opens. Opening
`/akagi.user.js` directly only works where a userscript manager
intercepts it; anywhere else the browser shows a page of JavaScript. The
page links the script and tells a browser without a manager what is
missing. It is static HTML: `lang` only selects built-in strings, and
nothing from the request is echoed back.

**Install before capture runs** (`preview.rs`). The backend only starts
once the configuration is saved, but the user wants to install the
userscript the moment they pick this mode — in Setup, before Finish. So
while ingest is picked and not running, the UI asks for a preview server
on the same address (`start_userscript_preview` / `stop_userscript_preview`)
that serves `/`, `/akagi.user.js` and `/ping` and nothing else: no `/ingest`,
so no frame reaches a bridge before capture starts. At most one preview
runs, and `IngestBackend::run` stops it (`preview::stop_any`) before it
binds, so the two never fight over the port.

**Who may connect** (`origin.rs`). Any page in the browser can open a
WebSocket to `127.0.0.1`, and whatever reaches the endpoint goes into the
bridges, so the handshake's `Origin` decides: a game site from
`GAME_SITES` is accepted, any other page gets `403`, and no `Origin` at
all (a local program, not a page) is accepted. The listen address must be
loopback; the backend refuses to start otherwise. The same `GAME_SITES`
list renders the userscript's `@match` lines, so the pages the script runs
on and the origins the endpoint accepts cannot drift apart. A page for a
different platform than the configured one is turned away after `hello`
with a toast.

**Wire protocol** (`protocol.rs`), one JSON text message per event:

```text
{"t":"hello","v":1,"agent":"userscript/3.7.1","origin":"https://tenhou.net","url":"https://tenhou.net/4/"}
{"t":"open","flow":1,"url":"wss://…"}
{"t":"frame","flow":1,"dir":"up"|"down","kind":"text"|"binary","data":"<text or base64>"}
{"t":"close","flow":1}
```

`hello` comes first; anything before it is ignored. `flow` numbers the
page's sockets and is only unique within one connection, so the flow key
is `(connection, flow)`. Both directions are sent — Mahjong Soul pairs
each response with its request inside the parser. A source announcing
another protocol version is closed with a toast asking to reinstall the
userscript; bump `PROTOCOL_VERSION` whenever the envelope changes
incompatibly.

**The userscript** (`akagi.user.js`) wraps `window.WebSocket` at
`document-start` and is strictly read-only: the game must behave exactly
as if it were not installed. It is valid JavaScript as committed — every
placeholder (`__AKAGI_ENDPOINT__`, `__AKAGI_VERSION__`, the `@match`
line) sits in a string or a comment — so `frontend/src/lib/userscript.test.ts`
runs it against a fake page. Change its behaviour there first. While Akagi
is unreachable it holds only the last minute of frames (capped by count
and size), forgets those of sockets that closed in the meantime — so a
game that ended while Akagi was down is never replayed as live — and stops
encoding altogether once the reconnect backoff tops out. A policy close
(`1008`, the version and platform rejections above) stops it until the
page reloads; retrying could not change the answer.

Autoplay is unavailable on this backend: there is no page handle to drive.

**Adding a source.** Anything that can speak the protocol above over a
WebSocket works, if its handshake passes the Origin check: code running
in the game page itself (it carries the page's `Origin`), or a program on
this machine (no `Origin`; it names its page in `hello.origin` instead).
An extension's background page or service worker sends its own
`chrome-extension://` / `moz-extension://` origin and is refused unless
that origin is added to the check.

## HTTP capture

The MITM and Chromium backends record the HTTP traffic they intercept,
not just WebSocket frames; ingest sees no HTTP at all. Before this,
everything except WS was discarded, which hid the game's own HTTP
entirely — route topology, version and CDN endpoints, and the analytics
beacons through which the client reports on itself.

`http.rs` holds the policy both of them share: what to keep, what to
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

### What MITM and Chromium see differently

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
