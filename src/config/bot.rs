use serde::{Deserialize, Serialize};

/// Cloud API protocol selected by the built-in bot.
///
/// The serialized values stay lowercase for the TypeScript settings UI. Common
/// historical/manual spellings are accepted on read and normalized on the next
/// config write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NativeApiProvider {
    #[default]
    Original,
    Flya,
}

impl<'de> Deserialize<'de> for NativeApiProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match raw.trim().to_ascii_lowercase().as_str() {
            "original" => Ok(Self::Original),
            "flya" => Ok(Self::Flya),
            _ => Err(serde::de::Error::unknown_variant(
                &raw,
                &["original", "flya"],
            )),
        }
    }
}

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
    /// Saved cloud profile to use: `original` or `flya`.
    pub provider: NativeApiProvider,
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
    /// FlyA profile. Kept separately so switching providers never overwrites
    /// the original cloud key or model choices.
    pub flya_base_url: String,
    pub flya_key: String,
    pub flya_model_4p: String,
    pub flya_model_3p: String,
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
}

/// Default inference server. Pre-filled so users don't have to type it; the API
/// still stays inactive until they enable it and paste a key.
pub const DEFAULT_API_BASE_URL: &str = "https://mjapi.shinkuan.me";
pub const DEFAULT_FLYA_API_BASE_URL: &str = "https://api.nashout.com";

impl Default for NativeApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: NativeApiProvider::Original,
            base_url: DEFAULT_API_BASE_URL.to_string(),
            key: String::new(),
            model_4p: String::new(),
            model_3p: String::new(),
            flya_base_url: DEFAULT_FLYA_API_BASE_URL.to_string(),
            flya_key: String::new(),
            flya_model_4p: String::new(),
            flya_model_3p: String::new(),
            proxy_enabled: false,
            proxy: String::new(),
        }
    }
}

impl NativeApiConfig {
    pub fn uses_flya(&self) -> bool {
        self.provider == NativeApiProvider::Flya
    }

    pub fn selected_base_url(&self) -> &str {
        if self.uses_flya() {
            &self.flya_base_url
        } else {
            &self.base_url
        }
    }

    pub fn selected_key(&self) -> &str {
        if self.uses_flya() {
            &self.flya_key
        } else {
            &self.key
        }
    }

    /// True only when the API path is fully configured (opted in with both a
    /// server URL and a key). The manager uses this to decide whether to build
    /// the API-backed runner or the local one.
    pub fn is_active(&self) -> bool {
        self.enabled
            && !self.selected_base_url().trim().is_empty()
            && !self.selected_key().trim().is_empty()
    }

    /// Model id to request for the given player count. Empty string ⇒ omit the
    /// `model` field and let the server pick its game default.
    pub fn model_for(&self, num_players: u8) -> &str {
        if self.uses_flya() && num_players == 3 {
            &self.flya_model_3p
        } else if self.uses_flya() {
            &self.flya_model_4p
        } else if num_players == 3 {
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
}

impl std::fmt::Debug for NativeApiConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeApiConfig")
            .field("enabled", &self.enabled)
            .field("provider", &self.provider)
            .field("base_url", &self.base_url)
            .field("key", &"<redacted>")
            .field("model_4p", &self.model_4p)
            .field("model_3p", &self.model_3p)
            .field("flya_base_url", &self.flya_base_url)
            .field("flya_key", &"<redacted>")
            .field("flya_model_4p", &self.flya_model_4p)
            .field("flya_model_3p", &self.flya_model_3p)
            .field("proxy_enabled", &self.proxy_enabled)
            .field(
                "proxy",
                &if self.proxy.is_empty() {
                    "<not configured>"
                } else {
                    "<configured>"
                },
            )
            .finish()
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

    #[test]
    fn native_api_debug_redacts_bearer_keys() {
        let config = NativeApiConfig {
            key: "TEST_SECRET_TOKEN".into(),
            flya_key: "FLYA_SECRET_TOKEN".into(),
            proxy: "socks5://user:PROXY_SECRET@127.0.0.1:1080".into(),
            ..NativeApiConfig::default()
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("TEST_SECRET_TOKEN"));
        assert!(!rendered.contains("FLYA_SECRET_TOKEN"));
        assert!(!rendered.contains("PROXY_SECRET"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn cloud_provider_switch_keeps_both_profiles_and_selects_one() {
        let mut config = NativeApiConfig {
            enabled: true,
            key: "ORIGINAL_KEY".into(),
            model_4p: "original-4p".into(),
            flya_key: "FLYA_KEY".into(),
            flya_model_4p: "flya-4p".into(),
            ..NativeApiConfig::default()
        };

        assert_eq!(config.selected_base_url(), DEFAULT_API_BASE_URL);
        assert_eq!(config.selected_key(), "ORIGINAL_KEY");
        assert_eq!(config.model_for(4), "original-4p");

        config.provider = NativeApiProvider::Flya;
        assert_eq!(config.selected_base_url(), DEFAULT_FLYA_API_BASE_URL);
        assert_eq!(config.selected_key(), "FLYA_KEY");
        assert_eq!(config.model_for(4), "flya-4p");
    }

    #[test]
    fn provider_alias_is_read_case_insensitively_and_written_canonically() {
        for mask in 0..16 {
            let spelling: String = "flya"
                .chars()
                .enumerate()
                .map(|(index, ch)| {
                    if mask & (1 << index) == 0 {
                        ch
                    } else {
                        ch.to_ascii_uppercase()
                    }
                })
                .collect();
            let config: NativeApiConfig =
                toml::from_str(&format!("provider = {spelling:?}\nflya_key = \"TEST_KEY\""))
                    .unwrap();
            assert_eq!(config.provider, NativeApiProvider::Flya);
            assert!(config.uses_flya());
            assert!(toml::to_string(&config)
                .unwrap()
                .contains("provider = \"flya\""));
        }

        let original: NativeApiConfig = toml::from_str("provider = \"OrIgInAl\"").unwrap();
        assert_eq!(original.provider, NativeApiProvider::Original);
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
