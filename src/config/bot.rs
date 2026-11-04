use serde::{Deserialize, Serialize};

/// Optional cloud-inference settings for the built-in (native) bot.
///
/// When [`NativeApiConfig::is_active`] is true, the built-in bot proxies each
/// decision to a remote inference server (`POST /v3/react`) instead of running
/// the embedded local model. The local model stays loaded as a fallback: if the
/// server is unreachable, rate-limited, or the key is invalid, the bot silently
/// plays the local model's move so a live game never stalls.
///
/// Read fresh at every decision by `crate::bot::native::NativeBot`, so toggling
/// the API, correcting the key, or switching models takes effect on the next
/// move of the game in progress.
///
/// Everything defaults empty / disabled so a fresh install uses the fully
/// offline local model until the user opts in and pastes a key.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NativeApiConfig {
    /// Route built-in-bot decisions through the remote API. Ignored unless a
    /// `base_url` and `key` are also set (see [`NativeApiConfig::is_active`]).
    pub enabled: bool,
    /// Base URL of the inference server, e.g. `https://host` or
    /// `http://127.0.0.1:8080`. A trailing slash is tolerated.
    pub base_url: String,
    /// Bearer API key (32 alphanumeric chars). Obtain one by redeeming a code.
    pub key: String,
    /// Model id for 4-player games (from `GET /v3/models`). Empty ⇒ let the
    /// server pick its default 4p model.
    pub model_4p: String,
    /// Model id for 3-player games. Empty ⇒ server default 3p model.
    pub model_3p: String,
    /// Whether [`Self::proxy`] is applied. When false the server is reached
    /// directly even if `proxy` holds a value, so a configured proxy can be
    /// switched off without losing the typed URL. Defaults off.
    pub proxy_enabled: bool,
    /// Proxy for ALL requests to the inference server (react, key/models,
    /// redeem, health, PayPal purchase). Accepts `http://host:port`,
    /// `https://host:port`, `socks5://host:port` or `socks5h://host:port`
    /// (the `h` variant resolves DNS through the proxy). Applied only when
    /// [`Self::proxy_enabled`]; empty ⇒ direct.
    pub proxy: String,
    /// Per-decision timeout for `POST /v3/react`, in milliseconds. This call
    /// sits on a live game's critical path, so the value trades turn-clock
    /// headroom against the chance of actually using the cloud model: too low
    /// and a slow-but-valid response is cut off (the bot falls back to the
    /// local model); too high and a reach's two-step (declare + discard, two
    /// react calls) risks overrunning the server's turn timer. Read fresh at
    /// every decision. Bounded by [`Self::effective_react_timeout`]; the raw
    /// value is stored so the UI round-trips exactly what the user typed.
    pub react_timeout_ms: u32,
}

/// Default inference server. Pre-filled so users don't have to type it; the API
/// still stays inactive until they enable it and paste a key.
pub const DEFAULT_API_BASE_URL: &str = "https://mjapi.shinkuan.me";

/// Default react timeout (ms). Comfortably above typical inference latency so a
/// momentarily slow server is still used rather than dropped to the local model,
/// while staying well inside a single Majsoul turn even for a reach's two calls.
pub const DEFAULT_REACT_TIMEOUT_MS: u32 = 3_000;

/// Lower/upper bounds the raw [`NativeApiConfig::react_timeout_ms`] is clamped to
/// before use. Below the floor a valid response is all but guaranteed to be cut
/// off; above the ceiling a reach's two react calls could overrun the turn timer.
pub const REACT_TIMEOUT_MIN_MS: u32 = 500;
pub const REACT_TIMEOUT_MAX_MS: u32 = 10_000;

impl Default for NativeApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: DEFAULT_API_BASE_URL.to_string(),
            key: String::new(),
            model_4p: String::new(),
            model_3p: String::new(),
            proxy_enabled: false,
            proxy: String::new(),
            react_timeout_ms: DEFAULT_REACT_TIMEOUT_MS,
        }
    }
}

/// Written by hand rather than derived: the whole config is logged at
/// startup, and logs get attached to bug reports. The key shows only its
/// length, and credentials in a URL (`socks5://user:pass@host`) are masked;
/// every other field prints as stored.
impl std::fmt::Debug for NativeApiConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = if self.key.is_empty() {
            String::new()
        } else {
            format!("<hidden, {} chars>", self.key.chars().count())
        };
        f.debug_struct("NativeApiConfig")
            .field("enabled", &self.enabled)
            .field("base_url", &mask_userinfo(&self.base_url))
            .field("key", &key)
            .field("model_4p", &self.model_4p)
            .field("model_3p", &self.model_3p)
            .field("proxy_enabled", &self.proxy_enabled)
            .field("proxy", &mask_userinfo(&self.proxy))
            .field("react_timeout_ms", &self.react_timeout_ms)
            .finish()
    }
}

/// `url` with any `user:pass@` in front of its host replaced by `***@`.
/// A missing scheme is fine (`user:pass@host:port`); an `@` past the host,
/// in the path or query, is left alone.
fn mask_userinfo(url: &str) -> std::borrow::Cow<'_, str> {
    let start = url.find("://").map_or(0, |i| i + 3);
    let host_end = url[start..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |i| start + i);
    match url[start..host_end].rfind('@') {
        Some(at) => format!("{}***{}", &url[..start], &url[start + at..]).into(),
        None => url.into(),
    }
}

impl NativeApiConfig {
    /// True only when the API path is fully configured (opted in with both a
    /// server URL and a key). The manager uses this to decide whether to build
    /// the API-backed runner or the local one.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.base_url.trim().is_empty() && !self.key.trim().is_empty()
    }

    /// Model id to request for the given player count. Empty string ⇒ omit the
    /// `model` field and let the server pick its game default.
    pub fn model_for(&self, num_players: u8) -> &str {
        if num_players == 3 {
            &self.model_3p
        } else {
            &self.model_4p
        }
    }

    /// The proxy actually used for inference-server traffic: the trimmed
    /// [`Self::proxy`] when [`Self::proxy_enabled`], else `""` (direct). Keeps
    /// the toggle authoritative in one place so a disabled-but-nonempty `proxy`
    /// never leaks into a client build.
    pub fn effective_proxy(&self) -> &str {
        if self.proxy_enabled {
            self.proxy.trim()
        } else {
            ""
        }
    }

    /// The react timeout actually applied to `POST /v3/react`, clamped to
    /// `[REACT_TIMEOUT_MIN_MS, REACT_TIMEOUT_MAX_MS]`. Clamping (rather than
    /// trusting the stored value) keeps a mistyped `0` from guaranteeing a miss
    /// and an absurdly large value from parking the bot past its turn timer.
    pub fn effective_react_timeout(&self) -> std::time::Duration {
        let ms = self
            .react_timeout_ms
            .clamp(REACT_TIMEOUT_MIN_MS, REACT_TIMEOUT_MAX_MS);
        std::time::Duration::from_millis(u64::from(ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_proxy_honors_the_toggle() {
        let mut cfg = NativeApiConfig {
            proxy: "  socks5://127.0.0.1:1080  ".to_string(),
            ..Default::default()
        };
        // Disabled: the configured value is kept but never applied.
        assert!(!cfg.proxy_enabled);
        assert_eq!(cfg.effective_proxy(), "");
        // Enabled: the trimmed value is used.
        cfg.proxy_enabled = true;
        assert_eq!(cfg.effective_proxy(), "socks5://127.0.0.1:1080");
    }

    /// Regression: the startup log printed the whole config, API key
    /// included, and logs get attached to bug reports.
    #[test]
    fn debug_output_hides_the_key_and_proxy_credentials() {
        let api = NativeApiConfig {
            key: "FAKEfakeFAKEfakeFAKEfakeFAKEfake".to_string(),
            proxy: "socks5h://someone:hunter2@127.0.0.1:1080".to_string(),
            ..Default::default()
        };
        let shown = format!("{api:?}");
        assert!(!shown.contains("FAKEfake"), "{shown}");
        assert!(!shown.contains("someone"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<hidden, 32 chars>"), "{shown}");
        assert!(shown.contains("socks5h://***@127.0.0.1:1080"), "{shown}");

        // The startup line prints the whole app config.
        let app = crate::config::AppConfig {
            bot: BotConfig {
                api,
                ..Default::default()
            },
            ..Default::default()
        };
        let shown = format!("{app:?}");
        assert!(!shown.contains("FAKEfake"), "{shown}");
        assert!(!shown.contains("hunter2"), "{shown}");
    }

    #[test]
    fn mask_userinfo_touches_only_credentials() {
        assert_eq!(mask_userinfo(""), "");
        assert_eq!(
            mask_userinfo("https://mjapi.example"),
            "https://mjapi.example"
        );
        assert_eq!(
            mask_userinfo("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/"
        );
        assert_eq!(
            mask_userinfo("https://host/a@b?c=@d"),
            "https://host/a@b?c=@d"
        );
        assert_eq!(
            mask_userinfo("http://u:p@host:3128"),
            "http://***@host:3128"
        );
        assert_eq!(mask_userinfo("u:p@host:1080"), "***@host:1080");
    }

    #[test]
    fn react_timeout_defaults_to_3s() {
        let cfg = NativeApiConfig::default();
        assert_eq!(cfg.react_timeout_ms, 3_000);
        assert_eq!(
            cfg.effective_react_timeout(),
            std::time::Duration::from_millis(3_000)
        );
    }

    #[test]
    fn react_timeout_is_clamped() {
        let mk = |ms: u32| NativeApiConfig {
            react_timeout_ms: ms,
            ..Default::default()
        };
        // Below the floor (incl. a mistyped 0) clamps up, not down to zero.
        assert_eq!(
            mk(0).effective_react_timeout(),
            std::time::Duration::from_millis(u64::from(REACT_TIMEOUT_MIN_MS))
        );
        assert_eq!(
            mk(100).effective_react_timeout(),
            std::time::Duration::from_millis(u64::from(REACT_TIMEOUT_MIN_MS))
        );
        // Above the ceiling clamps down.
        assert_eq!(
            mk(60_000).effective_react_timeout(),
            std::time::Duration::from_millis(u64::from(REACT_TIMEOUT_MAX_MS))
        );
        // In range passes through untouched.
        assert_eq!(
            mk(2_500).effective_react_timeout(),
            std::time::Duration::from_millis(2_500)
        );
    }

    #[test]
    fn react_timeout_missing_field_defaults() {
        // An older on-disk config predating the field must load with the
        // default (serde container `default`), not fail to parse.
        let cfg: NativeApiConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.react_timeout_ms, DEFAULT_REACT_TIMEOUT_MS);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BotConfig {
    /// Master switch. When `false`, no `BotManager` is spawned and the
    /// MJAI event bus runs without a consumer.
    pub enabled: bool,
    /// Active bot for 4-player (yonma) games. Subdirectory of `dir`.
    ///
    /// Reads the legacy `active` key on first load (see `migrate_legacy_active`)
    /// so existing config files keep working.
    pub active_4p: String,
    /// Active bot for 3-player (sanma) games. Empty string ⇒ no bot
    /// configured for 3p (analysis-only mode in 3p matches).
    pub active_3p: String,
    /// Legacy field, kept for one release for migration purposes. New code
    /// reads `active_4p` / `active_3p`. If the on-disk config has only
    /// `active` set, `active_4p` is populated from it during deserialise.
    #[serde(skip_serializing)]
    pub active: String,
    /// Run `uv sync` automatically before spawning the bot. Disabling
    /// makes startup faster on slow disks but assumes the venv is
    /// already in sync — usually for advanced users.
    pub auto_sync: bool,
    /// Root directory containing one subdir per bot. Resolved with the
    /// same fallback chain as other directory configs (`util::resolve_dir`).
    pub dir: String,
    /// Optional cloud-inference settings for the built-in native bot. When
    /// active, the native bot proxies decisions to a remote server instead of
    /// running the embedded model. See [`NativeApiConfig`].
    pub api: NativeApiConfig,
}

impl BotConfig {
    /// Pick the active bot name for the given player count. 3 ⇒ `active_3p`,
    /// anything else ⇒ `active_4p`.
    pub fn active_for(&self, num_players: u8) -> &str {
        if num_players == 3 {
            &self.active_3p
        } else {
            &self.active_4p
        }
    }

    /// Migrate the legacy `active` field into `active_4p` if the user's
    /// config file predates the per-mode split.
    pub fn migrate_legacy_active(&mut self) {
        if !self.active.is_empty() && self.active_4p.is_empty() {
            self.active_4p = std::mem::take(&mut self.active);
        } else {
            // Drop any legacy value we read; future writes won't include it.
            self.active.clear();
        }
    }
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            // Enabled by default: the built-in native bots (below) are always
            // available and need no install, so a fresh install shows bot
            // recommendations out of the box.
            enabled: true,
            // Built-in, pure-Rust default bots (no Python / libriichi). See
            // `crate::bot::native`. They are always available and need no
            // install, so they make sensible out-of-the-box defaults.
            active_4p: crate::bot::native::NATIVE_4P.to_string(),
            active_3p: crate::bot::native::NATIVE_3P.to_string(),
            active: String::new(),
            auto_sync: true,
            dir: "mjai_bot".to_string(),
            api: NativeApiConfig::default(),
        }
    }
}
