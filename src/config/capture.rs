//! Capture-mode configuration: which transport supplies WebSocket frames
//! to the bridge layer.
//!
//! Two modes:
//! - `Mitm` (default): hudsucker MITM proxy — see `[proxy]`.
//! - `Chromium`: a Chromium browser launched and controlled by Akagi via
//!   the Chrome DevTools Protocol.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub mode: CaptureMode,
    pub chromium: ChromiumConfig,
    pub http: HttpCaptureConfig,
}

/// What to record of the HTTP traffic a backend intercepts.
///
/// Akagi used to record WebSocket frames and discard everything else,
/// which hid the game's own HTTP entirely — route topology, version
/// endpoints, and the analytics beacons through which the client reports
/// on itself. This turns that back on.
///
/// `record_all` defaults to **off** on purpose. Full HTTP capture puts
/// access tokens, cookies and authorization headers into the session
/// file, and nothing is redacted — redaction would re-create the blind
/// spot this exists to remove. So the default keeps only the exchanges a
/// recognizer understood (analytics beacons, and Akagi's own notes about
/// traffic it declined to intercept), and the firehose is opt-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpCaptureConfig {
    /// Record every intercepted exchange, not just recognized ones.
    /// Off by default — see the struct docs.
    pub record_all: bool,
    /// Keep textual request/response bodies within `max_body_bytes`.
    /// Only consulted when the exchange is being recorded at all.
    pub bodies: bool,
    /// Ceiling on a buffered body. Larger ones are recorded with their
    /// size and the reason they were skipped, never truncated silently.
    pub max_body_bytes: usize,
    /// Chromium backend only: also record static subresources (images,
    /// fonts, media, stylesheets). Off by default — a WebGL client pulls
    /// enough of them to bury everything else, and none of it says
    /// anything about the client.
    pub static_assets: bool,
}

impl Default for HttpCaptureConfig {
    fn default() -> Self {
        Self {
            record_all: false,
            bodies: true,
            max_body_bytes: 256 * 1024,
            static_assets: false,
        }
    }
}

impl HttpCaptureConfig {
    pub fn policy(&self) -> crate::capture::http::HttpCapturePolicy {
        crate::capture::http::HttpCapturePolicy {
            record_all: self.record_all,
            bodies: self.bodies,
            max_body_bytes: self.max_body_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    #[default]
    Mitm,
    Chromium,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromiumConfig {
    /// Absolute path to a chrome/chromium binary. `""` = auto-detect.
    pub executable: String,
    /// User-data-dir for the controlled profile. `""` = exe-adjacent
    /// `chrome-profile/` (resolved via `util::resolve_dir`, with an
    /// AppImage / read-only fallback to `<user_config_root>/chrome-profile`).
    pub user_data_dir: String,
    /// URL to navigate to on launch. `""` = don't auto-navigate (open new tab page).
    pub start_url: String,
    /// Chrome-for-Testing channel/version to download as fallback.
    /// `"stable"` / `"beta"` resolve to the latest channel pin; otherwise
    /// treated as a literal version (e.g. `"131.0.6778.85"`).
    pub cft_channel: String,
    /// Force using Chrome-for-Testing even when system Chrome is detected.
    pub force_cft: bool,
    /// Extra CLI args appended after our defaults. Advanced users only.
    pub extra_args: Vec<String>,
    /// Fail the game's Aliyun SLS web-tracking (telemetry) beacons inside
    /// the browser instead of letting them out.
    ///
    /// On by default — the chromium counterpart of `[proxy]
    /// block_telemetry`, for the same reason: the beacons report login
    /// stats, game status and device / account identifiers, and many ad
    /// blockers already block the host, so a missing beacon is not
    /// evidence of a tool. Every request to `*.log.aliyuncs.com` — CORS
    /// preflights and worker requests included — is paused through the
    /// DevTools `Fetch` domain and failed as `net::ERR_BLOCKED_BY_CLIENT`,
    /// exactly what an ad blocker produces.
    ///
    /// Takes effect on the next capture start. See
    /// `src/capture/chromium/cdp.rs`.
    pub block_telemetry: bool,
}

impl Default for ChromiumConfig {
    fn default() -> Self {
        Self {
            executable: String::new(),
            user_data_dir: String::new(),
            start_url: "https://game.maj-soul.com/1/".to_string(),
            cft_channel: "stable".to_string(),
            force_cft: false,
            extra_args: vec![],
            block_telemetry: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let cfg = CaptureConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.mode, CaptureMode::Mitm);
        assert_eq!(back.chromium.cft_channel, "stable");
    }

    #[test]
    fn mode_serialises_lowercase() {
        let cfg = CaptureConfig {
            mode: CaptureMode::Chromium,
            chromium: Default::default(),
            http: Default::default(),
        };
        let s = toml::to_string(&cfg).unwrap();
        assert!(s.contains("mode = \"chromium\""), "got: {s}");
    }

    /// The default is deliberately conservative: full HTTP capture would
    /// put access tokens and cookies into every session file, and nothing
    /// is redacted (redaction would re-create the blind spot the capture
    /// exists to remove). Recognized exchanges are still recorded.
    #[test]
    fn http_capture_is_opt_in_but_bodies_are_on_once_it_is() {
        let cfg = CaptureConfig::default();
        assert!(!cfg.http.record_all, "full capture must be opt-in");
        assert!(cfg.http.bodies);
        assert!(!cfg.http.static_assets);
        assert_eq!(cfg.http.max_body_bytes, 256 * 1024);

        let s = toml::to_string(&cfg).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.http, cfg.http);
    }

    #[test]
    fn chromium_config_round_trip() {
        let original = CaptureConfig {
            mode: CaptureMode::Chromium,
            chromium: ChromiumConfig {
                executable: "/opt/chrome/chrome".into(),
                user_data_dir: "/tmp/profile".into(),
                start_url: "https://example.test/".into(),
                cft_channel: "131.0.6778.85".into(),
                force_cft: true,
                extra_args: vec!["--lang=ja".into()],
                block_telemetry: false,
            },
            http: Default::default(),
        };
        let s = toml::to_string(&original).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.mode, CaptureMode::Chromium);
        assert_eq!(back.chromium.executable, "/opt/chrome/chrome");
        assert!(back.chromium.force_cft);
        assert_eq!(back.chromium.extra_args, vec!["--lang=ja"]);
        assert!(!back.chromium.block_telemetry);
    }

    /// Telemetry blocking is on by default in chromium mode too — it has
    /// to protect without the user knowing the beacons exist.
    #[test]
    fn chromium_blocks_telemetry_by_default() {
        assert!(ChromiumConfig::default().block_telemetry);
    }

    /// A `[capture.chromium]` written before the field existed must pick up
    /// the default, not silently leave telemetry unblocked.
    #[test]
    fn older_chromium_configs_gain_telemetry_blocking() {
        let cfg: CaptureConfig =
            toml::from_str("mode = \"chromium\"\n[chromium]\nexecutable = \"\"\nforce_cft = false")
                .expect("older config must still parse");
        assert!(cfg.chromium.block_telemetry);
    }

    #[test]
    fn missing_section_uses_defaults() {
        // Older configs that lack [capture] entirely should still parse.
        #[derive(serde::Deserialize)]
        struct Wrap {
            #[serde(default)]
            capture: CaptureConfig,
        }
        let s = "";
        let w: Wrap = toml::from_str(s).unwrap();
        assert_eq!(w.capture.mode, CaptureMode::Mitm);
    }
}
