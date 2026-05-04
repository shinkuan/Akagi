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
        };
        let s = toml::to_string(&cfg).unwrap();
        assert!(s.contains("mode = \"chromium\""), "got: {s}");
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
            },
        };
        let s = toml::to_string(&original).unwrap();
        let back: CaptureConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.mode, CaptureMode::Chromium);
        assert_eq!(back.chromium.executable, "/opt/chrome/chrome");
        assert!(back.chromium.force_cft);
        assert_eq!(back.chromium.extra_args, vec!["--lang=ja"]);
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
