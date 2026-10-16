# Config Module

Handles loading and deserializing `config.toml` into typed Rust structs.

## Config file resolution order

1. CLI argument: `--config <path>`
2. Executable directory: `<exe_dir>/configs/config.toml`
3. Working directory: `./configs.toml`
4. If none found, defaults are serialized to `<exe_dir>/configs/config.toml`
   (or to the CLI path when `--config <path>` was given but missing). The
   freshly written file is then loaded so the next launch picks it up.
5. If that write fails, fall back to in-memory built-in defaults.

## Saving (`merge.rs`)

`ipc::commands::persist_config` does **not** serialize `AppConfig` over the
file. `config.toml` is resolved from a shared location, so it can hold keys
this build never declared — another Akagi build's, or the user's own notes —
and every section struct is `#[serde(default)]`, so loading silently drops
them. Serializing back over the file would then delete them for good, along
with every comment.

`merge_into(&config, &existing)` parses the file with `toml_edit`, writes only
the keys this build produces, and leaves everything else (unknown sections,
unknown keys inside known sections, comments, blank lines, key order) exactly
where it is. It is idempotent: saving twice produces identical bytes. If the
file on disk isn't valid TOML there is nothing to merge into, and
`persist_config` falls back to a full rewrite — which is also what the app
already assumed at load time, since `load_config` falls back to defaults on a
parse error.

One thing to keep in step: `OPAQUE_TABLES` in `merge.rs` lists the dotted paths
of map-like tables (`HashMap`/`BTreeMap` fields, currently
`autoplay.delay.lognormal`). Those are replaced wholesale instead of merged,
because merging a map by key would make a deleted entry immortal — nothing in
the new document names it, so nothing overwrites it. **Add the path of any new
map-like config field there**, or users won't be able to delete entries from
it.

## Adding a new config section

1. Create `src/config/foo.rs` with your struct:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FooConfig {
    pub bar: String,
}

impl Default for FooConfig {
    fn default() -> Self {
        Self {
            bar: "default_value".to_string(),
        }
    }
}
```

2. Register in `src/config/mod.rs`:

```rust
mod foo;
pub use foo::FooConfig;
```

3. Add the field to `AppConfig` in `mod.rs`:

```rust
pub struct AppConfig {
    pub general: GeneralConfig,
    pub foo: FooConfig,  // new
}
```

And update `AppConfig::default()` accordingly.

4. Add the section to `configs/config.toml`:

```toml
[foo]
bar = "default_value"
```

## Existing sections

- `general` (`general.rs`) — `first_run_completed` flag controls whether the setup wizard is shown on app start. `developer_mode` unlocks developer-only fields in the frontend (currently the `bot.api.base_url` editor); it gates the UI only — the backend honours whatever the config file says. (UI language lives in the webview's `localStorage`, managed by i18next — not in this config.)
- `logging` (`logging.rs`) — log root dir, console default level (overridden by `RUST_LOG`), `all_level` severity filter for `all.log` (`EnvFilter` syntax).
- `platform` (`platform.rs`) — game platform whose traffic to bridge. `kind` selects which `Bridge` impl runs in the capture pipeline. Currently only `Majsoul`.
- `proxy` (`proxy.rs`) — MITM proxy enable flag, listen addr, CA cert dir. Authoritative when `capture.mode = "mitm"`. Also `rewrite_certificate_report` (default **on**): a Mahjong Soul standalone client reports every TLS certificate it was served on every login, which with Akagi in the path means reporting Akagi's own CA — by name, plus five other fields that differ from a genuine certificate. With this on, the proxy substitutes the certificates it observed upstream. Turn it off to capture what the client *would* have said, which is the only way to verify the correction is still complete after a client update. See `src/proxy/README.md`.
- `bot` (`bot.rs`) — AI bot enable flag, active bot subdir name (per player count), `mjai_bot/` root, and whether to run `uv sync` automatically before spawning. Also nests `[bot.api]` (`NativeApiConfig`): the built-in bot's optional cloud-inference settings — `enabled`, `base_url`, `key`, `model_4p`, `model_3p`, plus an optional `proxy` (http/https/socks5/socks5h) gated by a `proxy_enabled` toggle that routes *all* inference-server traffic — react, key/models, redeem, health, PayPal purchase. `effective_proxy()` collapses the toggle into the applied value (off ⇒ direct even when `proxy` holds a URL). `is_active()` gates the remote path on all three of enabled + URL + key, and the bot re-reads this section at every decision, so edits apply mid-game. A missing `[bot.api]` in an older `config.toml` deserialises to "disabled".
- `capture` (`capture.rs`) — selects the capture transport: `mitm` (uses `[proxy]`) or `chromium` (uses `[capture.chromium]`). Chromium mode launches a controlled browser and intercepts WebSocket frames via CDP — no proxy/CA setup needed. `[capture.chromium] block_telemetry` (default **on**) is the chromium counterpart of `[proxy] block_telemetry`: it fails every request to the game's Aliyun SLS telemetry host (`*.log.aliyuncs.com`) inside the browser. A config written before the field existed picks up the default. See `src/capture/README.md`.
  - `[capture.http]` (`HttpCaptureConfig`) — what to record of the HTTP traffic a backend intercepts, on top of the WebSocket frames it already records. `record_all` defaults to **off**: full capture puts access tokens, cookies and authorization headers into `<session>/inspector.jsonl` and **nothing is redacted**, because redaction would re-create the blind spot the capture exists to remove. With it off, exchanges a recognizer understood (analytics beacons, and Akagi's own notes about traffic it declined to intercept) are still recorded. `bodies` + `max_body_bytes` bound what gets buffered — a body over the cap is recorded with its size and the reason it was skipped, never truncated silently. `static_assets` is chromium-only and defaults off, since a WebGL client pulls enough images and fonts to bury everything else. Treat a session directory as personal data before sharing it.
- `overlay` (`overlay.rs`) — the always-on-top suggestion window. `enabled` (default **true**) doubles as its lifecycle switch: `ipc::overlay::reconcile` opens or closes the window to match it, at startup and on every `update_config` / `set_overlay_enabled`, so both "I closed it" and "I left it open" survive a restart. A `config.toml` written before this section existed therefore gets the overlay switched on, which is the intent. `top_n` and `opacity` are bounds-checked by `clamped_top_n()` / `clamped_opacity()` rather than trusted, because `config.toml` is a text file a user can put `top_n = 0` into — which would render an overlay with no rows and no explanation.
- `network` (`network.rs`) — how GitHub-hosted downloads (app updates, bot installs) are routed. `github_mirror_mode`: `auto` (direct with a short timeout, then gh-proxy-style accelerator mirrors — the default), `direct` (never mirror), or `mirror` (mirrors first, for users who know GitHub is blocked). `github_custom_mirror` is an optional accelerator prefix tried before the built-in list; `custom_mirror()` normalises and validates it (scheme required) rather than trusting the raw string. Consumed by `github::mirror::candidates`. Mirror-fetched bytes are untrusted — see `src/github/README.md` for how signing gates them.

## Notes

- All section structs must derive `Default` and use `#[serde(default)]` so partial configs work.
- Each section lives in its own file for isolation.
