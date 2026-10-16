# Proxy Module

MITM HTTP/HTTPS/WebSocket proxy built on [hudsucker](https://crates.io/crates/hudsucker). Used to intercept game traffic (e.g. Majsoul WebSocket frames) for protocol parsing and AI integration.

The hudsucker dependency in `Cargo.toml` opts out of default features and re-enables `rcgen-ca`, `rustls-client`, and `http2`. The `http2` flag is load-bearing: without it, both the MITM TLS server (`rcgen_authority`) and the upstream hyper-rustls connector skip `h2` ALPN / `enable_http2()`, which breaks HTTP/2-only origin servers — the Steam build of Mahjong Soul hits exactly this and stalls at "獲取[配置初始化文件]失敗 [3010]".

## Files

- `mod.rs` — Public entry: `start_proxy(config, session, shutdown)`. Builds the proxy from `ProxyConfig` and shares the logging `Session`.
- `ca.rs` — CA certificate management. Loads `akagi-ca.cer` + `akagi-ca.key` from `ca_dir`, generating a fresh self-signed CA on first run. Also writes the cert in `.crt` / `.pem` / `.der` form and the key in `.key.der` form for OS / tooling compatibility.
- `handler.rs` — `ProxyHandler` implementing `HttpHandler` + `WebSocketHandler`. Logs WS frame direction/length to text log and writes raw binary frames to `<session>/proxy.binlog`. Extend here to parse protocol messages. The HTTP side logs every forwarded request and any upstream-forward failure with full error source chain under tracing target `akagi::proxy::forward` — filter on that target when diagnosing "stuck on loading" reports. `should_intercept` raw-tunnels any CONNECT whose authority is an IP literal so app-side SNI/Host headers reach multi-tenant CDNs intact; hostnames continue to be MITM'd. `handle_request` refuses any CONNECT to a loopback authority with a `403` — see below.

## Loopback CONNECT is refused

A redirector rule that matches the game executable for *any* target host also matches the game's own loopback traffic, including the `bind`+`listen`+self-connect dance that emulates `socketpair()` on Windows. Akagi then receives `CONNECT 127.0.0.1:<ephemeral>`.

Tunneling that deadlocks. hudsucker's `process_connect` answers `200` and then blocks reading the client's first 4 bytes *before* it dials the upstream socket (see `should_intercept` — it is not even called until that read returns). A self-connect's client side never speaks first: it is waiting for its own `accept()`, which can only fire once Akagi dials. Neither side moves and the game hangs on its loading screen.

Tunneling it *correctly* is not an option either — libcurl's `socketpair()` emulation verifies that the address it accepted matches its connecting socket's local address, which a proxied hop breaks. So `handle_request` returns `403` immediately, which short-circuits hudsucker's `proxy()` before that blocking read is reached.

The refusal also pushes a **sticky `warn` toast** (id `proxy-loopback-connect`) onto `NotifyBus` — nobody opens the log while staring at a game that never finishes loading. `ProxyHandler` latches on the first refusal (`loopback_notified`), so the toast fires once even though a misconfigured redirector produces one refused CONNECT per socket the game opens; the log keeps every occurrence (first at `WARN`, the rest at `DEBUG`). `start_proxy` takes the bus as `Option<NotifyBus>` — `None` in "log only" mode. The user-facing fix is to exclude loopback in the redirector. Covered by `tests/proxy_loopback_connect.rs`.
- `upstream.rs` — Custom hyper-rustls connector used for the proxy → server leg. Skips server-cert validation (`NoVerify`) on purpose so we can talk to CDN IPs whose default cert covers only DNS hostnames, matching the game client's own loose validation. Replaces what hudsucker's `with_rustls_connector` would have given us. See the module-level doc for the threat-model justification.

## CA Certificate

On first run, a self-signed root CA is generated at `<ca_dir>/akagi-ca.{cer,crt,pem,der}` (default `./ca`), with the matching private key written as `akagi-ca.key` (PEM) and `akagi-ca.key.der` (DER). To intercept TLS traffic the user must trust the CA cert in their OS / browser store — pick whichever extension that store accepts (Windows commonly wants `.cer`/`.crt`/`.der`, Linux/Firefox `.pem`/`.crt`). Subsequent runs reuse the existing CA and back-fill any missing format files.

### `ca_dir` resolution

If `ca_dir` is absolute, it's used as-is. If relative (default `./ca`), resolution mirrors config loading:

1. `<exe_dir>/<ca_dir>` if it exists
2. `<cwd>/<ca_dir>` if it exists
3. Otherwise create at `<exe_dir>/<ca_dir>` (preferred), falling back to `<cwd>/<ca_dir>` if exe path is unavailable

The proxy responds to `GET /ping` with `pong` — useful for liveness checks.

## Configuration

Lives under `[proxy]` in `config.toml`:

```toml
[proxy]
enabled = true
addr = "127.0.0.1:23410"
ca_dir = "./ca"
block_telemetry = true
```

## HTTP capture

`handle_request` and `handle_response` record every intercepted exchange
onto the Inspector timeline and forward it unaltered — bodies are
buffered and the message rebuilt, never rewritten. Recognized exchanges
(analytics beacons and the like) are annotated by
`crate::inspector::annotate`; the proxy itself knows nothing about what
they mean.

Two proxy-specific notes:

- **Pairing.** hudsucker's `HttpContext` carries only the client address,
  so a response is matched to its request by a FIFO queue per connection.
  That is exact over HTTP/1.x and wrong over HTTP/2, where streams
  interleave — so an h2 response is recorded unpaired and annotated
  `akagi_unpaired` rather than attributed to the wrong URL.

  Only requests hudsucker answers through `handle_response` may be
  queued. A `CONNECT`, a WebSocket upgrade and a failed forward are each
  answered elsewhere, so queueing them would leave an entry nothing
  claims and shift every later response onto the wrong request — which a
  live capture caught as JSON responses recorded against a `CONNECT`. A
  failed forward is claimed in `handle_error` and recorded as a `502`
  annotated `akagi_forward_failed`, so "the game asked and we could not
  reach it" is visible rather than a silent gap.
- **The raw-tunnel bypass is recorded.** `should_intercept` returning
  false means we will never see inside that connection, so the CONNECT is
  logged with an `akagi_bypass` annotation. An announced gap is not a
  blind spot.

Policy lives under `[capture.http]`, not `[proxy]` — it applies to both
capture backends. See `src/capture/README.md`.

## The certificate report is corrected

The one place Akagi deliberately changes what it forwards.

A Mahjong Soul standalone client posts a `certificate_info` beacon on
**every login**, listing every TLS certificate it was served. With Akagi in
the path it is describing Akagi's certificate, and six independent fields
give that away: the issuer names the tool, the subject is the exact
hostname where a genuine certificate is a wildcard, the key algorithm
differs, the serial is half the length, `not_before` is about a minute
before the beacon, and the validity window is a flat 365 days. Renaming
the CA would fix one of the six.

`rewrite/majsoul_cert.rs` substitutes the certificate fields Akagi
observed on the **upstream** leg — the same certificate the client would
have been served otherwise. `url` and `ip` are left alone, as is every
other beacon parameter and their order.

The supporting pieces:

- `certstore.rs` — the origin certificates seen by the TLS verifier,
  rendered the way .NET's `X509Certificate2` renders them (reversed DNs,
  uppercase-hex thumbprint and serial, `M/d/yyyy h:mm:ss tt` local time).
  Getting that formatting wrong would be more conspicuous than the
  problem it fixes, so it is pinned by tests.
- `upstream.rs` — records the leaf in `NoVerify::verify_server_cert`,
  which is the only place Akagi ever sees the real certificate.

It declines rather than half-doing the job: an entry whose gateway we
never connected to is left untouched and counted, and the count is logged
at `WARN` under `akagi::proxy::rewrite`. Dropping the entry instead would
change the array length, which tracks how many gateways the client probed.

Switched by `[proxy] rewrite_certificate_report` (default **on**). Turn it
off to capture what the client *would* have said — the only way to check
the correction is still complete after a client update. MITM-only; a
browser cannot report peer certificates at all, so the chromium backend
has nothing to correct. Covered by `tests/proxy_cert_report_rewrite.rs`,
which stands up a real TLS origin and asserts on what left the machine.

## Telemetry beacons are blocked

The game clients report on themselves through Aliyun SLS **web-tracking**
beacons — fire-and-forget requests to `*.log.aliyuncs.com/logstores/<store>/track`
carrying login stats, game status, and device / account identifiers (the
`certificate_info` beacon above is one of these). `handle_request` drops
every one the recognizer matches instead of forwarding it, answering the
client with the same empty `200` the real endpoint returns —
`handler.rs::block_telemetry_beacon`.

We used to forward them on the theory that a *missing* beacon might itself
be a signal. It is not: many ad blockers already block that host, so a
missing beacon is indistinguishable from an ad-blocked one. Dropping them
keeps that data from leaving the machine at all.

Blocking takes precedence over the certificate-report correction above:
when it is on, the `certificate_info` beacon is dropped along with every
other beacon, so there is nothing left to rewrite — and nothing that could
leak Akagi's CA in the window before the genuine upstream certificate has
been observed. The drop is still recorded on the timeline (`akagi_blocked`
plus the beacon's own `sls_beacon` annotation): an announced gap is not a
blind spot.

Switched by `[proxy] block_telemetry` (default **on**). Turn it off to
forward the beacons — the certificate report is then corrected instead of
dropped. MITM-only; the chromium backend blocks the same host inside the
browser under its own switch, `[capture.chromium] block_telemetry` — see
`src/capture/README.md`. Covered by `tests/proxy_telemetry_block.rs`, which asserts a beacon is
answered locally and never reaches the upstream while ordinary traffic
still forwards.

## Adding traffic interception

Edit `handler.rs::ProxyHandler::handle_message`. The `WebSocketContext` distinguishes upstream (`ClientToServer`) vs downstream (`ServerToClient`) frames. Return `Some(msg)` to forward unchanged, return a modified `Message` to inject changes, or return `None` to drop.

For protobuf parsing, see `src/bridge/majsoul/parser.rs` — Majsoul WS frames use a 5-layer format: `[type byte][BaseMessage protobuf][inner message][XOR-encrypted action]`.

## Adding state to the handler

Currently `ProxyHandler` is unit-struct + `Clone`. To add shared state (sender channel, parser, settings), give it an `Arc<...>` field and clone is cheap. See MajsoulMax `handler.rs` for the pattern.
