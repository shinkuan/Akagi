use serde::{Deserialize, Serialize};

/// Bounds for [`OverlayConfig::top_n`]. One row is the minimum that still
/// says something; past five the window stops being a glanceable HUD.
pub const TOP_N_MIN: usize = 1;
pub const TOP_N_MAX: usize = 5;

/// Below ~0.3 the overlay is unreadable, which looks like a bug rather than
/// a setting. Clamp instead of trusting a hand-edited `config.toml`.
pub const OPACITY_MIN: f64 = 0.3;
pub const OPACITY_MAX: f64 = 1.0;

/// The always-on-top suggestion overlay ("PiP") window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Open the floating overlay window. Off by default: the suggestions are
    /// in the panel next to the board, so a second window over the game is
    /// noise unless the player asks for it.
    pub enabled: bool,
    /// How many Bot Show rows to render.
    pub top_n: usize,
    /// Card opacity. The window itself is transparent; this fades the card
    /// drawn inside it so the table stays partly visible underneath.
    pub opacity: f64,
    /// Keep the window above the game client. Off is useful on a second
    /// monitor, where being topmost only gets in the way.
    pub always_on_top: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            top_n: 3,
            opacity: 0.95,
            always_on_top: true,
        }
    }
}

impl OverlayConfig {
    /// `top_n` clamped into [`TOP_N_MIN`, `TOP_N_MAX`].
    ///
    /// The frontend picks from a bounded control, but `config.toml` is a
    /// plain text file a user can put `top_n = 0` into — which would render
    /// an overlay with no rows and no explanation.
    pub fn clamped_top_n(&self) -> usize {
        self.top_n.clamp(TOP_N_MIN, TOP_N_MAX)
    }

    /// `opacity` clamped into [`OPACITY_MIN`, `OPACITY_MAX`], with NaN
    /// treated as the default.
    pub fn clamped_opacity(&self) -> f64 {
        if self.opacity.is_nan() {
            return Self::default().opacity;
        }
        self.opacity.clamp(OPACITY_MIN, OPACITY_MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_a_three_row_overlay_that_stays_closed() {
        let c = OverlayConfig::default();
        // The floating window is opt-in: the suggested tile is highlighted on
        // the board itself, so a second window over the game is noise unless
        // the player asks for it.
        assert!(!c.enabled, "the floating window is off out of the box");
        assert_eq!(c.top_n, 3);
        assert!(c.always_on_top);
        assert_eq!(c.clamped_top_n(), 3);
        assert_eq!(c.clamped_opacity(), 0.95);
    }

    /// A config with no `[overlay]` section gets these defaults — closed, with
    /// the on-board hint available.
    #[test]
    fn a_config_without_an_overlay_section_keeps_the_window_closed() {
        let cfg: crate::config::AppConfig = toml::from_str("[bot]\nenabled = true\n").unwrap();
        assert!(!cfg.overlay.enabled);
    }

    /// A hand-edited `config.toml` must not be able to produce a zero-row or
    /// invisible overlay — both read as "the feature is broken".
    #[test]
    fn out_of_range_values_are_clamped() {
        let zero = OverlayConfig {
            top_n: 0,
            opacity: 0.0,
            ..Default::default()
        };
        assert_eq!(zero.clamped_top_n(), TOP_N_MIN);
        assert_eq!(zero.clamped_opacity(), OPACITY_MIN);

        let huge = OverlayConfig {
            top_n: 99,
            opacity: 4.2,
            ..Default::default()
        };
        assert_eq!(huge.clamped_top_n(), TOP_N_MAX);
        assert_eq!(huge.clamped_opacity(), OPACITY_MAX);
    }

    #[test]
    fn nan_opacity_falls_back_to_default() {
        let nan = OverlayConfig {
            opacity: f64::NAN,
            ..Default::default()
        };
        assert_eq!(nan.clamped_opacity(), OverlayConfig::default().opacity);
    }

    /// A `config.toml` written before this section existed must still parse.
    #[test]
    fn missing_section_deserialises_to_defaults() {
        let cfg: crate::config::AppConfig =
            toml::from_str("[bot]\nenabled = true\n").expect("legacy config should parse");
        assert_eq!(cfg.overlay, OverlayConfig::default());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.overlay.enabled = true;
        cfg.overlay.top_n = 5;
        cfg.overlay.opacity = 0.6;
        cfg.overlay.always_on_top = false;

        let body = toml::to_string_pretty(&cfg).unwrap();
        assert!(body.contains("[overlay]"), "expected [overlay] in:\n{body}");

        let back: crate::config::AppConfig = toml::from_str(&body).unwrap();
        assert_eq!(back.overlay, cfg.overlay);
    }
}
