use super::range::{upgrade, MsRange};
use rand::Rng;
use serde::{Deserialize, Deserializer, Serialize};

/// Old single-value defaults, read as "never changed" (see
/// [`super::range::upgrade`]), and the ranges that replace them.
macro_rules! upgraded_range {
    ($name:ident, $old:expr, $new:expr) => {
        fn $name<'de, D: Deserializer<'de>>(d: D) -> Result<MsRange, D::Error> {
            upgrade(d, $old, $new)
        }
    };
}
const MIN_DELAY: MsRange = MsRange::new(1000, 1600);
const MIN_BUTTON_DELAY: MsRange = MsRange::new(1600, 2600);
const INTER_CLICK_DELAY: MsRange = MsRange::new(250, 500);
const HOVER_DELAY: MsRange = MsRange::new(150, 300);
const CLICK_HOLD: MsRange = MsRange::new(60, 140);
const VERIFY_INPUT: MsRange = MsRange::new(300, 500);
const DEALER_FIRST_DISCARD_EXTRA: MsRange = MsRange::new(2000, 3000);
upgraded_range!(upgrade_min_delay, 1000, MIN_DELAY);
upgraded_range!(upgrade_min_button_delay, 1600, MIN_BUTTON_DELAY);
upgraded_range!(upgrade_inter_click_delay, 300, INTER_CLICK_DELAY);
upgraded_range!(upgrade_hover_delay, 200, HOVER_DELAY);
upgraded_range!(upgrade_click_hold, 100, CLICK_HOLD);
upgraded_range!(upgrade_verify_input, 300, VERIFY_INPUT);
upgraded_range!(
    upgrade_dealer_first_discard_extra,
    2000,
    DEALER_FIRST_DISCARD_EXTRA
);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutoplayConfig {
    /// Master switch. When `false`, no `AutoplayManager` is spawned and bot
    /// responses are not converted into UI clicks.
    pub enabled: bool,
    /// Per-platform autoplay knobs. Only the matching platform's section is
    /// consulted at runtime; the others sit dormant.
    pub majsoul: MajsoulAutoplayConfig,
    /// Pre-click delay model (platform-agnostic). See `autoplay::delay`.
    pub delay: DelayModelConfig,
}

/// Which delay policy drives autoplay. Exactly one is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DelayMode {
    /// The historical fixed model: `uniform(pre_click_delay_min_ms,
    /// pre_click_delay_max_ms)` plus the dealer-opening pad. Ignores the
    /// Lua script and the calibrated distributions.
    Legacy,
    /// The scriptable model: `delay.lua` next to the config file (auto-
    /// generated with the calibrated human-like default on first use,
    /// hot-reloaded on save). If the script fails, the built-in
    /// calibrated model runs instead.
    #[default]
    Lua,
}

/// Shape of the base thinking-time distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DelayDistribution {
    /// `uniform(pre_click_delay_min_ms, pre_click_delay_max_ms)` — the
    /// historical Akagi behaviour.
    Uniform,
    /// Per-decision-kind log-normal over seconds (`exp(N(mu, sigma))`),
    /// clamped to the `[pre_click_delay_min_ms, pre_click_delay_max_ms]`
    /// window scaled by 0.5x/4x so the fat tail can exceed the old bounds
    /// without running away. Default: real players' think times are
    /// log-normal with sigma ~0.5–0.6 (measured from the server-side
    /// action clock of ranked game records).
    #[default]
    LogNormal,
}

/// Parameters of the built-in pre-click delay model (`autoplay::delay`).
///
/// The log-normal defaults are **calibrated against real ranked-game
/// records** (Throne-room 4p; think times measured on the server-side
/// action clock of decoded game records). Additive rule bonuses still
/// default to 0 — the calibrated per-kind distributions already encode
/// the human differences they were meant to approximate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayModelConfig {
    /// Which policy is active: `legacy` (old fixed model) or `lua`.
    pub mode: DelayMode,
    /// Minimum total thinking time per decision, ms; each decision draws
    /// its own floor from the range. This is a functional floor, not a
    /// style knob: Mahjong Soul needs time to render tiles after the
    /// triggering frame, and a click issued before the UI exists is
    /// silently lost. Applies in every mode and cannot be undercut by the
    /// Lua script. A range rather than one value because every decision
    /// the model aims below the floor lands *on* it — with a single value,
    /// on the same instant every time.
    #[serde(deserialize_with = "upgrade_min_delay")]
    pub min_delay_ms: MsRange,
    /// UI-readiness floor for decisions that click an **action button**
    /// (chi/pon/kan, ron/tsumo, riichi, skip, kyuushu, kita), ms, drawn
    /// per decision. The buttons appear only after the triggering
    /// discard's animation and their own pop-in — noticeably later than
    /// hand tiles render — so they need a higher floor than plain
    /// discards. Applies in every mode and cannot be undercut by the Lua
    /// script.
    #[serde(deserialize_with = "upgrade_min_button_delay")]
    pub min_button_delay_ms: MsRange,
    /// Base distribution shape (built-in model; also the fallback when
    /// the Lua script fails).
    pub distribution: DelayDistribution,
    /// Per-decision-kind log-normal parameters `[mu, sigma]` in
    /// ln(seconds). Keys: `dahai_tedashi`, `dahai_tsumogiri`,
    /// `post_call_dahai`, `reach`, `claim`, `hora`. A missing key falls
    /// back to `dahai_tedashi`. Only used for `LogNormal`.
    pub lognormal: std::collections::BTreeMap<String, [f64; 2]>,
    /// Let a long-thought sample (one exceeding the soft cap) dip into
    /// the server's extra time pool even without a top-two near-tie.
    /// Real players do this routinely; the bank refills every kyoku.
    /// The spend stays bounded by `bank_use_fraction` /
    /// `bank_max_single_ms`. `false` restores the strict
    /// near-tie-only gate.
    pub bank_on_long_thought: bool,
    /// Extra target time when riichi can be declared this turn (a genuine
    /// decision), ms. 0 = off.
    pub riichi_extra_ms: u32,
    /// Extra target time when the action is a kan declaration, ms. 0 = off.
    pub kan_extra_ms: u32,
    /// Reserved headroom inside the server window for network RTT and
    /// scheduling jitter, ms. The soft cap is
    /// `time_fixed - safety_margin - click_overhead`.
    pub safety_margin_ms: u32,
    /// Fraction of the server's extra time pool (`time_add`) a single
    /// hard decision may spend when the model allows bank use.
    pub bank_use_fraction: f64,
    /// Absolute cap on extra-pool spend for a single decision, ms.
    pub bank_max_single_ms: u32,
    /// Static cap applied when no server budget is known (non-Majsoul
    /// platform, or before the first operation list), ms. 0 = no cap.
    pub no_budget_cap_ms: u32,
}

/// Calibrated per-kind log-normal parameters, ln(seconds).
///
/// Source: 30 Throne-room ranked 4p records (~21,500 decision windows),
/// think time on the server's game clock (includes each player's own
/// client→server latency — a bias in the human direction). Medians:
/// tedashi ≈2.2s, tsumogiri ≈1.55s, post-call discard ≈1.5s, riichi
/// declaration ≈2.7s, claim windows (mostly passes) ≈1.4s.
///
/// This flat table is the built-in fallback; the generated `delay.lua`
/// models the same data with more structure (routine-vs-think mixture,
/// tile class, defence reads, budget shaping).
fn default_lognormal() -> std::collections::BTreeMap<String, [f64; 2]> {
    [
        ("dahai_tedashi", [0.87, 0.62]),
        ("dahai_tsumogiri", [0.52, 0.53]),
        ("post_call_dahai", [0.52, 0.42]),
        ("reach", [1.10, 0.55]),
        ("claim", [0.26, 0.57]),
        ("hora", [0.15, 0.50]),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

impl DelayModelConfig {
    /// This decision's settings: every range drawn once (see
    /// [`MsRange::draw`]).
    pub fn draw<R: Rng + ?Sized>(&self, rng: &mut R) -> Self {
        Self {
            min_delay_ms: self.min_delay_ms.draw(rng),
            min_button_delay_ms: self.min_button_delay_ms.draw(rng),
            ..self.clone()
        }
    }
}

impl Default for DelayModelConfig {
    fn default() -> Self {
        Self {
            mode: DelayMode::Lua,
            // From the historical uniform lower bound — the value the old
            // model implicitly relied on for UI readiness.
            min_delay_ms: MIN_DELAY,
            // Buttons render after the discard animation + their own
            // pop-in; observed to still be missing at 1000ms on real
            // hardware.
            min_button_delay_ms: MIN_BUTTON_DELAY,
            distribution: DelayDistribution::LogNormal,
            lognormal: default_lognormal(),
            bank_on_long_thought: true,
            riichi_extra_ms: 0,
            kan_extra_ms: 0,
            safety_margin_ms: 1000,
            bank_use_fraction: 0.25,
            bank_max_single_ms: 5000,
            // 15s static ceiling: far above anything the default
            // distribution produces, low enough to survive even a 5s+20
            // room's base window if the budget is somehow unknown.
            no_budget_cap_ms: 15_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MajsoulAutoplayConfig {
    /// Lower bound of the random pre-click delay (ms). The reference
    /// Akagi autoplay used `random.uniform(1.0, 3.0)` seconds; the same
    /// distribution is replicated here as `[1000, 3000]` ms by default.
    pub pre_click_delay_min_ms: u32,
    /// Upper bound of the random pre-click delay (ms).
    pub pre_click_delay_max_ms: u32,
    /// Inter-click delay between staged clicks within one action (e.g.
    /// reach button → riichi tile, or chi button → candidate select).
    /// Drawn per decision, like every range here.
    #[serde(deserialize_with = "upgrade_inter_click_delay")]
    pub inter_click_delay_ms: MsRange,
    /// How long to hover the mouse over a target before pressing.
    /// Empirically Laya's input system samples hover state before a
    /// mousedown registers a hit on the tile sprite — clicks issued
    /// without a hover delay (or shorter than ~100ms) get dropped on
    /// the floor. Keep the lower end at 100ms or more.
    #[serde(deserialize_with = "upgrade_hover_delay")]
    pub hover_delay_ms: MsRange,
    /// How long to hold the mouse button down between mousePressed and
    /// mouseReleased. Non-zero so the engine doesn't collapse the pair
    /// into a single frame.
    #[serde(deserialize_with = "upgrade_click_hold")]
    pub click_hold_ms: MsRange,
    /// How long to wait for the client's own uplink command
    /// (`inputOperation` / `inputChiPengGang`) after a click sequence
    /// before treating the click as swallowed and pressing again, ms.
    /// A drawn `0` disables verification for that decision, so `[0, 0]`
    /// turns it off.
    ///
    /// Checked by polling, so a click that worked costs nothing — only a
    /// genuinely lost one waits this out. Raising it is safer than
    /// lowering it: a retry that fires while the first press was merely
    /// slow presses a second time.
    #[serde(deserialize_with = "upgrade_verify_input")]
    pub verify_input_ms: MsRange,
    /// How many times a click sequence may be repeated when no input
    /// command follows it. `0` disables retrying (verification then only
    /// logs). Each retry re-checks that the decision window is still the
    /// one the plan was made for.
    pub click_retries: u32,
    /// Reload the game page after this many decisions in a row where the
    /// client accepted no input at all. `0` disables it.
    ///
    /// The client can end up in a state where presses on the action
    /// buttons stop registering and stay that way for the rest of the
    /// game — once it starts it does not recover on its own, and every
    /// remaining decision runs to timeout. A reload reconnects into the
    /// hand through the bridge's `GameRestore` path, so the cost is a
    /// reconnect rather than the game. Deliberately not 1: a single lost
    /// press is common enough and costs only that decision.
    pub reload_after_failures: u32,
    /// Extra delay tacked onto the dealer's first discard. Mahjong Soul
    /// plays a hand-sort animation when the dealer receives all 14 tiles
    /// at once; clicks issued during the animation are dropped. ~2s
    /// covers the animation across normal device speeds. Set to `[0, 0]`
    /// to opt out (e.g. on a fast box where the animation finishes inside
    /// the regular pre-click delay anyway).
    #[serde(deserialize_with = "upgrade_dealer_first_discard_extra")]
    pub dealer_first_discard_extra_delay_ms: MsRange,
}

impl MajsoulAutoplayConfig {
    /// This decision's settings: every range drawn once (see
    /// [`MsRange::draw`]).
    pub fn draw<R: Rng + ?Sized>(&self, rng: &mut R) -> Self {
        Self {
            inter_click_delay_ms: self.inter_click_delay_ms.draw(rng),
            hover_delay_ms: self.hover_delay_ms.draw(rng),
            click_hold_ms: self.click_hold_ms.draw(rng),
            verify_input_ms: self.verify_input_ms.draw(rng),
            dealer_first_discard_extra_delay_ms: self.dealer_first_discard_extra_delay_ms.draw(rng),
            ..self.clone()
        }
    }
}

impl Default for MajsoulAutoplayConfig {
    fn default() -> Self {
        Self {
            pre_click_delay_min_ms: 1000,
            pre_click_delay_max_ms: 3000,
            inter_click_delay_ms: INTER_CLICK_DELAY,
            hover_delay_ms: HOVER_DELAY,
            click_hold_ms: CLICK_HOLD,
            verify_input_ms: VERIFY_INPUT,
            click_retries: 2,
            reload_after_failures: 3,
            dealer_first_discard_extra_delay_ms: DEALER_FIRST_DISCARD_EXTRA,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pre-existing config written before the delay model existed has
    /// no `[autoplay.delay]` section — it must deserialize to the
    /// calibrated defaults, not zeros. Pins the container-level
    /// `#[serde(default)]` attributes against regression.
    #[test]
    fn missing_delay_section_gets_defaults() {
        let cfg: AutoplayConfig = toml::from_str("enabled = true").unwrap();
        let default = DelayModelConfig::default();
        assert_eq!(cfg.delay.mode, default.mode);
        assert_eq!(cfg.delay.min_delay_ms, default.min_delay_ms);
        assert_eq!(cfg.delay.min_button_delay_ms, default.min_button_delay_ms);
        assert_eq!(cfg.delay.lognormal, default.lognormal);
    }

    /// A partial `[delay]` table keeps its explicit values and fills
    /// everything else from the defaults.
    #[test]
    fn partial_delay_section_fills_remaining_defaults() {
        let cfg: AutoplayConfig = toml::from_str("[delay]\nmin_delay_ms = 5\n").unwrap();
        let default = DelayModelConfig::default();
        assert_eq!(cfg.delay.min_delay_ms, MsRange::fixed(5));
        assert_eq!(cfg.delay.mode, default.mode);
        assert_eq!(cfg.delay.min_button_delay_ms, default.min_button_delay_ms);
        assert!(!cfg.delay.lognormal.is_empty());
    }

    /// A config written while these were single values: the old defaults
    /// become the new ranges, a value the user changed stays exactly what
    /// they chose, and a range is read as written.
    #[test]
    fn single_values_from_an_older_config_are_upgraded() {
        let cfg: AutoplayConfig = toml::from_str(
            "[majsoul]\nhover_delay_ms = 200\nclick_hold_ms = 80\ninter_click_delay_ms = [100, 400]\n\
             [delay]\nmin_delay_ms = 1000\nmin_button_delay_ms = 2000\n",
        )
        .unwrap();
        assert_eq!(cfg.majsoul.hover_delay_ms, HOVER_DELAY);
        assert_eq!(cfg.majsoul.click_hold_ms, MsRange::fixed(80));
        assert_eq!(cfg.majsoul.inter_click_delay_ms, MsRange::new(100, 400));
        assert_eq!(cfg.delay.min_delay_ms, MIN_DELAY);
        assert_eq!(cfg.delay.min_button_delay_ms, MsRange::fixed(2000));
        assert_eq!(cfg.majsoul.verify_input_ms, VERIFY_INPUT, "absent: default");
    }

    /// A decision's settings hold one drawn value per range, inside it.
    #[test]
    fn a_draw_fixes_every_range_inside_its_bounds() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let cfg = MajsoulAutoplayConfig::default();
        let d = DelayModelConfig::default();
        for _ in 0..50 {
            let c = cfg.draw(&mut rng);
            for (drawn, range) in [
                (c.inter_click_delay_ms, cfg.inter_click_delay_ms),
                (c.hover_delay_ms, cfg.hover_delay_ms),
                (c.click_hold_ms, cfg.click_hold_ms),
                (c.verify_input_ms, cfg.verify_input_ms),
                (
                    c.dealer_first_discard_extra_delay_ms,
                    cfg.dealer_first_discard_extra_delay_ms,
                ),
            ] {
                assert_eq!(drawn.min, drawn.max);
                assert!((range.min..=range.max).contains(&drawn.ms()));
            }
            let e = d.draw(&mut rng);
            assert!((d.min_delay_ms.min..=d.min_delay_ms.max).contains(&e.min_delay_ms.ms()));
            assert_eq!(e.min_button_delay_ms.min, e.min_button_delay_ms.max);
        }
    }
}
