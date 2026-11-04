//! Pre-click delay model: how long a decision should appear to take.
//!
//! The model computes a **target total thinking time** for the current
//! decision window — the interval the server observes between opening the
//! window and receiving our action. That interval already contains network
//! latency, proxy overhead, bot inference and the click sequence itself,
//! so the caller converts the target into a sleep with
//! `target - elapsed_since_window_opened - click_overhead` (see
//! `majsoul::push_pre_delay`), not by sleeping the target verbatim.
//!
//! Layering:
//! - [`decide`] — the pure built-in model. Base distribution (uniform by
//!   default, log-normal opt-in) plus additive bonuses for decision types
//!   that genuinely take a human longer (riichi declarable, kan). The
//!   model deliberately ignores the bot's probability distribution:
//!   measured bot policies can be nearly flat, which turned every
//!   confidence-based rule into a permanent bias. Defaults are
//!   behaviour-equivalent with the historical fixed `uniform(min, max)`.
//! - Budget enforcement (soft/hard caps from the server-granted
//!   [`TimeBudget`](crate::autoplay::budget::TimeBudget)) sits on top —
//!   the sampled target is clamped so autoplay can never run the window
//!   into an auto-discard.
//! - A user Lua script can replace [`decide`]'s policy; functional floors
//!   and the budget hard cap are enforced *after* the script so a
//!   misbehaving script cannot click into an animation or overrun the
//!   server clock.
//!
//! All quantities are milliseconds unless stated otherwise.

pub mod probs;
pub mod script;

pub use probs::DecisionProbs;
pub use script::{refresh_outdated_default, DelayScript, ScriptHost};

use crate::config::{DelayDistribution, DelayModelConfig, MajsoulAutoplayConfig};
use rand::Rng;

/// What kind of action the bot chose. Derived from the mjai event; used
/// to pick delay characteristics, never to alter the action itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    Dahai,
    Reach,
    Chi,
    Pon,
    Daiminkan,
    Ankan,
    Kakan,
    Hora,
    Ryukyoku,
    Kita,
    /// Declining a claim window (mjai `none` with a visible Skip button).
    Pass,
}

impl DecisionKind {
    pub fn is_kan(self) -> bool {
        matches!(self, Self::Daiminkan | Self::Ankan | Self::Kakan)
    }

    /// The first click of this decision lands on an **action button**
    /// (chi/pon/kan, ron/tsumo, riichi, skip, kyuushu, kita) rather than
    /// a hand tile. Buttons render later than tiles — after the
    /// triggering discard's animation plus their own pop-in — so these
    /// decisions carry a higher UI-readiness floor.
    pub fn clicks_action_button(self) -> bool {
        !matches!(self, Self::Dahai)
    }
}

/// Rough class of a discarded tile. Measured effect (Throne records):
/// honor discards are routine far more often than middle tiles (fast
/// fraction ~22% vs ~4% for tedashi) and think ~35% shorter overall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileClass {
    Honor,
    Terminal,
    Middle,
}

impl TileClass {
    /// Classify an mjai tile string ("1m".."9m", "5pr", "E","P","C"...).
    pub fn of_mjai(pai: &str) -> Option<Self> {
        let mut chars = pai.chars();
        let first = chars.next()?;
        match first {
            'E' | 'S' | 'W' | 'N' | 'P' | 'F' | 'C' => Some(Self::Honor),
            '1' | '9' => Some(Self::Terminal),
            // '0' guards against Majsoul-style red-five notation leaking
            // through; mjai proper writes red fives as "5?r".
            '0' | '2'..='8' => Some(Self::Middle),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Honor => "honor",
            Self::Terminal => "terminal",
            Self::Middle => "middle",
        }
    }
}

/// How a policy that imitates people sees a decision (see
/// `autoplay::advisor`): the share of players making their likeliest move,
/// the next likeliest, and the move that is actually being made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HumanView {
    pub top: f64,
    pub second: Option<f64>,
    pub chosen: f64,
}

/// What is known about a decision besides the bot's own numbers: facts read
/// off the table (`autoplay::table`) and a second opinion
/// (`autoplay::advisor`). Every part is optional — `None` is "not read", not
/// "no" — and, like the probabilities, none of it can alter the action. The
/// built-in model ignores it; it is there for delay scripts.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Reading {
    /// The discarded tile is loose in the hand.
    pub loose: Option<bool>,
    /// The runner-up discard is loose in the hand. `None` unless the bot's
    /// two likeliest candidates are discards of different tiles.
    pub runner_up_loose: Option<bool>,
    /// The discarded tile cannot deal in to any riichi. `None` with no
    /// riichi on the table.
    pub safe: Option<bool>,
    pub human: Option<HumanView>,
}

/// Copy of the server time budget taken at planning time.
/// `elapsed_ms` is pre-computed so the planner stays free of clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BudgetSnapshot {
    /// Base window time (`operation.time_fixed`).
    pub fixed_ms: u32,
    /// Extra time pool (`operation.time_add`).
    pub add_ms: u32,
    /// Already consumed since the window opened.
    pub elapsed_ms: u32,
}

/// Everything the model may consider. All fields are derivable from data
/// the autoplay manager already holds; none of them can alter the action.
#[derive(Debug, Clone)]
pub struct DelayInput<'a> {
    pub kind: DecisionKind,
    /// For `Dahai`: the discard is the just-drawn tile. Real players
    /// tsumogiri ~500ms faster (median) than tedashi.
    pub is_tsumogiri: bool,
    /// For `Dahai`: a discard following our own chi/pon (hand size
    /// 3n+1, no draw) — a distinctly faster decision in real play.
    pub is_post_call: bool,
    /// No tile in any kawa yet — the very first action of a kyoku.
    pub first_action_of_kyoku: bool,
    /// The click must wait out a dealing/sorting animation (dealer's
    /// 14-tile opening hand, or a kita on the opening draw). The
    /// animation wait is a functional floor: scripts cannot lower it.
    pub opening_animation: bool,
    /// Riichi is declarable this turn (`ActionType::Riichi` legal).
    pub can_riichi: bool,
    /// We are in accepted riichi (only Skip windows reach the planner).
    pub in_riichi: bool,
    /// An opponent has declared riichi — every decision is now also a
    /// defence read (measured: +15–25% think time across the board).
    pub opponent_riichi: bool,
    /// Class of the discarded tile, for `Dahai` (None otherwise).
    pub tile_class: Option<TileClass>,
    /// Our discard number this kyoku, 1-based (0 = unknown). Real
    /// players slow down as the hand develops.
    pub junme: u32,
    pub legal_action_count: usize,
    /// Normalized top/second candidate probabilities, if the bot's meta
    /// could be interpreted (see [`probs::normalize_meta`]). The built-in
    /// model ignores them; they are exposed to user Lua scripts only.
    pub probs: Option<DecisionProbs>,
    /// Table facts and a second opinion on this decision, as far as they
    /// were read. Exposed to Lua scripts only.
    pub reading: Reading,
    /// Server time budget for this window, if known.
    pub budget: Option<BudgetSnapshot>,
    /// Time the click sequence itself will take *after* the pre-delay
    /// (hover/hold, candidate clicks, inter-click gaps). Deducted from
    /// the budget caps so multi-stage actions don't overrun.
    pub click_overhead_ms: u32,
    pub cfg: &'a MajsoulAutoplayConfig,
    pub delay_cfg: &'a DelayModelConfig,
}

/// Model output. `total_target_ms` is the target **total** thinking time
/// since the window opened — the caller subtracts what has already
/// elapsed. `allow_bank` reports whether this decision justifies dipping
/// into the server's extra time pool (`time_add`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelayDecision {
    pub total_target_ms: u32,
    pub allow_bank: bool,
}

/// The built-in delay policy. Pure given `rng`.
pub fn decide<R: Rng + ?Sized>(input: &DelayInput, rng: &mut R) -> DelayDecision {
    let d = input.delay_cfg;

    let mut target = base_sample(input, rng);
    let mut allow_bank = false;

    // A naturally long-thought sample may spend bank (bounded by the
    // hard cap): real players dip into the pool routinely and it refills
    // every kyoku. Decided on the *base* sample so a huge additive bonus
    // alone doesn't unlock the bank when the gate is off.
    if d.bank_on_long_thought && input.budget.is_some() && target > budget_cap(input, false) {
        allow_bank = true;
    }

    if input.opening_animation {
        // Historical behaviour: the animation wait is added on top of the
        // random delay, keeping the visible post-animation pause random.
        target = target.saturating_add(input.cfg.dealer_first_discard_extra_delay_ms.ms());
    }
    if input.can_riichi {
        target = target.saturating_add(d.riichi_extra_ms);
    }
    if input.kind.is_kan() {
        target = target.saturating_add(d.kan_extra_ms);
    }
    // Budget enforcement, then the functional floor. The floor is applied
    // last: clicking into the dealing animation loses the click entirely,
    // which is strictly worse than shaving budget headroom (and in
    // practice the base window is minutes, the floor a few seconds).
    target = target.min(budget_cap(input, allow_bank));
    target = target.max(functional_floor(input));

    DelayDecision {
        total_target_ms: target,
        allow_bank,
    }
}

/// The hard ceiling for the target total thinking time.
///
/// With a known server budget:
///
/// ```text
/// soft_cap = time_fixed - safety_margin - click_overhead
/// hard_cap = soft_cap + min(time_add * bank_use_fraction, bank_max_single)
/// ```
///
/// The soft cap never touches the extra time pool; only a decision the
/// model marked `allow_bank` (top-two near-tie) may reach the hard cap.
/// With `time_add == 0` the two coincide. Exceeding the hard cap would
/// mean the client auto-discards for us — the most conspicuous failure
/// mode there is — so the cap is unconditional.
///
/// Without a budget, a static configurable ceiling applies.
pub fn budget_cap(input: &DelayInput, allow_bank: bool) -> u32 {
    let d = input.delay_cfg;
    match input.budget {
        Some(b) => {
            let soft = b
                .fixed_ms
                .saturating_sub(d.safety_margin_ms)
                .saturating_sub(input.click_overhead_ms);
            if allow_bank {
                let bank = (f64::from(b.add_ms) * d.bank_use_fraction.clamp(0.0, 1.0)) as u32;
                soft.saturating_add(bank.min(d.bank_max_single_ms))
            } else {
                soft
            }
        }
        None => {
            if d.no_budget_cap_ms == 0 {
                return u32::MAX;
            }
            // Never bind below what the user's own base configuration can
            // produce — a config with pre_click_delay_max_ms above the
            // static cap must keep its legacy behaviour. The cap only
            // trims rule bonuses and distribution tails.
            let configured_base = input
                .cfg
                .pre_click_delay_max_ms
                .saturating_add(functional_floor(input));
            d.no_budget_cap_ms.max(configured_base)
        }
    }
}

/// The minimum target no policy — built-in or script — may go below.
///
/// All components are about the client UI rather than style — a click
/// issued before the UI exists is silently lost (the reason the legacy
/// model had a hard lower bound):
/// - `min_delay_ms` covers tile render latency after the triggering
///   frame;
/// - `min_button_delay_ms` covers action buttons (chi/pon/kan, ron,
///   riichi, skip, ...), which appear only after the triggering
///   discard's animation plus their own pop-in;
/// - the dealer-opening pad covers the 14-tile hand-sort animation.
pub fn functional_floor(input: &DelayInput) -> u32 {
    let d = input.delay_cfg;
    let mut floor = d.min_delay_ms.ms();
    if input.kind.clicks_action_button() {
        floor = floor.max(d.min_button_delay_ms.ms());
    }
    if input.opening_animation {
        floor = floor.max(input.cfg.dealer_first_discard_extra_delay_ms.ms());
    }
    floor
}

/// Sample the base thinking time.
fn base_sample<R: Rng + ?Sized>(input: &DelayInput, rng: &mut R) -> u32 {
    let lo = input.cfg.pre_click_delay_min_ms;
    let hi = input.cfg.pre_click_delay_max_ms.max(lo);

    // Reference behaviour (`autoplay_majsoul.py:156-157`): the first
    // action of a kyoku uses the upper bound as a fixed delay — slightly
    // slower but more human on the opening turn.
    if input.first_action_of_kyoku {
        return hi;
    }

    match input.delay_cfg.distribution {
        DelayDistribution::Uniform => {
            if hi == lo {
                lo
            } else {
                rng.random_range(lo..=hi)
            }
        }
        DelayDistribution::LogNormal => {
            let [mu, sigma] = lognormal_params(input);
            // Invalid parameters from a hand-edited config fall back to
            // the uniform behaviour rather than panicking mid-game. The
            // explicit checks matter: `LogNormal::new` accepts a NaN mu
            // (which would collapse every sample to 0 — silently pinning
            // all delays to the floor) and even a negative sigma.
            if !mu.is_finite() || !sigma.is_finite() || sigma < 0.0 {
                return if hi == lo {
                    lo
                } else {
                    rng.random_range(lo..=hi)
                };
            }
            match rand_distr::LogNormal::new(mu, sigma) {
                Ok(dist) => {
                    let secs: f64 = rng.sample(dist);
                    let ms = (secs * 1000.0).round().clamp(0.0, u32::MAX as f64) as u32;
                    // Loose clamp: allow the tail to exceed the uniform
                    // window (that's the point of a fat tail) but keep it
                    // bounded, and never go implausibly fast.
                    ms.clamp(lo / 2, hi.saturating_mul(4))
                }
                Err(_) => {
                    if hi == lo {
                        lo
                    } else {
                        rng.random_range(lo..=hi)
                    }
                }
            }
        }
    }
}

/// Which calibrated parameter set applies to this decision.
pub fn kind_key(input: &DelayInput) -> &'static str {
    match input.kind {
        DecisionKind::Dahai => {
            if input.is_post_call {
                "post_call_dahai"
            } else if input.is_tsumogiri {
                "dahai_tsumogiri"
            } else {
                "dahai_tedashi"
            }
        }
        DecisionKind::Reach => "reach",
        // Claim-window responses: chi/pon/daiminkan, declining one, and
        // the (rare) kyuushu declaration all share the reaction-window
        // distribution.
        DecisionKind::Chi
        | DecisionKind::Pon
        | DecisionKind::Daiminkan
        | DecisionKind::Pass
        | DecisionKind::Ryukyoku => "claim",
        // Own-turn kan declarations think like a tedashi.
        DecisionKind::Ankan | DecisionKind::Kakan => "dahai_tedashi",
        DecisionKind::Hora => "hora",
        DecisionKind::Kita => "dahai_tsumogiri",
    }
}

/// `[mu, sigma]` for this decision, falling back to `dahai_tedashi`,
/// then to the hardcoded calibrated tedashi values for a hand-edited
/// config with the whole table removed.
fn lognormal_params(input: &DelayInput) -> [f64; 2] {
    let table = &input.delay_cfg.lognormal;
    table
        .get(kind_key(input))
        .or_else(|| table.get("dahai_tedashi"))
        .copied()
        .unwrap_or([0.90, 0.60])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn cfg() -> MajsoulAutoplayConfig {
        MajsoulAutoplayConfig::default()
    }

    fn input<'a>(
        cfg: &'a MajsoulAutoplayConfig,
        delay_cfg: &'a DelayModelConfig,
    ) -> DelayInput<'a> {
        DelayInput {
            kind: DecisionKind::Dahai,
            is_tsumogiri: false,
            is_post_call: false,
            first_action_of_kyoku: false,
            opening_animation: false,
            can_riichi: false,
            in_riichi: false,
            opponent_riichi: false,
            tile_class: None,
            junme: 0,
            legal_action_count: 1,
            probs: None,
            reading: Default::default(),
            budget: None,
            click_overhead_ms: 0,
            cfg,
            delay_cfg,
        }
    }

    /// Delay config in legacy uniform mode (the pre-calibration default).
    fn uniform() -> DelayModelConfig {
        DelayModelConfig {
            distribution: DelayDistribution::Uniform,
            ..Default::default()
        }
    }

    const AKAGI_SEED: u64 = 0xA4A61;

    /// Regression: a NaN mu passes `LogNormal::new` (it only validates
    /// sigma) and used to collapse every sample to 0 — pinning all
    /// delays to the floor. Invalid params must fall back to a uniform
    /// draw inside the configured window instead.
    #[test]
    fn invalid_lognormal_params_fall_back_to_uniform() {
        let c = cfg();
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        for params in [[f64::NAN, 0.5], [0.9, f64::NAN], [0.9, -1.0]] {
            let mut d = DelayModelConfig::default();
            for v in d.lognormal.values_mut() {
                *v = params;
            }
            let i = input(&c, &d);
            for _ in 0..200 {
                let ms = base_sample(&i, &mut r);
                assert!(
                    (c.pre_click_delay_min_ms..=c.pre_click_delay_max_ms).contains(&ms),
                    "params {params:?}: sample {ms} outside the uniform window"
                );
            }
        }
    }

    /// Uniform mode must reproduce the historical behaviour: a uniform
    /// draw inside [min, max].
    #[test]
    fn uniform_mode_matches_legacy() {
        let c = cfg();
        let d = uniform();
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        for _ in 0..1000 {
            let dec = decide(&input(&c, &d), &mut r);
            assert!(
                (c.pre_click_delay_min_ms..=c.pre_click_delay_max_ms)
                    .contains(&dec.total_target_ms),
                "target {} outside legacy window",
                dec.total_target_ms
            );
            assert!(!dec.allow_bank);
        }
    }

    /// The calibrated default: per-kind log-normal whose medians land on
    /// the Throne-room measurements (tedashi ~2.4s, tsumogiri ~1.85s,
    /// claim ~1.3s, post-call ~1.55s).
    #[test]
    fn calibrated_default_matches_measured_medians() {
        let c = cfg();
        let d = DelayModelConfig::default();
        assert_eq!(d.distribution, DelayDistribution::LogNormal);

        let median_for = |set: fn(&mut DelayInput)| {
            let mut r = StdRng::seed_from_u64(AKAGI_SEED);
            let mut samples: Vec<u32> = (0..4000)
                .map(|_| {
                    let mut i = input(&c, &d);
                    set(&mut i);
                    decide(&i, &mut r).total_target_ms
                })
                .collect();
            samples.sort_unstable();
            samples[samples.len() / 2]
        };

        let tedashi = median_for(|_| {});
        assert!(
            (2000..=2800).contains(&tedashi),
            "tedashi median {tedashi} off calibration (e^0.87 ≈ 2390ms)"
        );
        let tsumogiri = median_for(|i| i.is_tsumogiri = true);
        assert!(
            (1400..=2000).contains(&tsumogiri),
            "tsumogiri median {tsumogiri} off calibration (e^0.52 ≈ 1680ms)"
        );
        let claim = median_for(|i| i.kind = DecisionKind::Pass);
        assert!(
            (1100..=1600).contains(&claim),
            "claim median {claim} off calibration (e^0.26 ≈ 1300ms)"
        );
        let post_call = median_for(|i| i.is_post_call = true);
        assert!(
            (1400..=2000).contains(&post_call),
            "post-call median {post_call} off calibration (e^0.52 ≈ 1680ms)"
        );
        assert!(
            tsumogiri < tedashi && claim < post_call && post_call < tedashi,
            "measured ordering must hold: claim < post-call < tedashi"
        );
    }

    /// First action of a kyoku pins the base to the upper bound (legacy).
    #[test]
    fn first_action_uses_upper_bound() {
        let c = cfg();
        let d = DelayModelConfig::default();
        let mut i = input(&c, &d);
        i.first_action_of_kyoku = true;
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(dec.total_target_ms, c.pre_click_delay_max_ms);
    }

    /// Dealer-opening fold-in: animation wait is added on top of the base
    /// and acts as a floor (regression for the folded
    /// `dealer_first_discard_extra_delay_ms` sleep).
    #[test]
    fn opening_animation_adds_and_floors() {
        let c = cfg();
        let d = DelayModelConfig::default();
        let mut i = input(&c, &d);
        i.first_action_of_kyoku = true;
        i.opening_animation = true;
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(
            dec.total_target_ms,
            c.pre_click_delay_max_ms + c.dealer_first_discard_extra_delay_ms.ms()
        );
        assert!(dec.total_target_ms >= functional_floor(&i));
    }

    /// Regression: a decision aimed below the floor lands *on* the floor.
    /// With one floor value every such decision took the same time to the
    /// millisecond (a quarter of all discards in a live session); with the
    /// floor drawn per decision they spread over its range.
    #[test]
    fn decisions_held_up_by_the_floor_do_not_share_one_time() {
        // A base sample of 0: every decision is held up by the floor.
        let c = MajsoulAutoplayConfig {
            pre_click_delay_min_ms: 0,
            pre_click_delay_max_ms: 0,
            ..cfg()
        };
        let d = uniform();
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let totals: Vec<u32> = (0..100)
            .map(|_| {
                let (c, d) = (c.draw(&mut r), d.draw(&mut r));
                decide(&input(&c, &d), &mut r).total_target_ms
            })
            .collect();
        let range = d.min_delay_ms;
        assert!(totals.iter().all(|t| (range.min..=range.max).contains(t)));
        let distinct: std::collections::HashSet<_> = totals.iter().collect();
        assert!(distinct.len() > 50, "{} distinct of 100", distinct.len());
    }

    /// Rule bonuses are additive and off by default.
    #[test]
    fn rule_bonuses_apply_when_configured() {
        let c = cfg();
        let d = DelayModelConfig {
            riichi_extra_ms: 2000,
            kan_extra_ms: 500,
            ..uniform()
        };
        let mut i = input(&c, &d);
        i.kind = DecisionKind::Ankan;
        i.can_riichi = true;
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert!(
            dec.total_target_ms >= c.pre_click_delay_min_ms + 2500,
            "bonuses must be added (got {})",
            dec.total_target_ms
        );
    }

    /// Regression: the built-in model must ignore the bot's probability
    /// distribution entirely. Confidence-based rules (near-tie extra
    /// time, obvious-decision caps) turned into permanent biases on bots
    /// with flat policy probabilities.
    #[test]
    fn probs_do_not_affect_the_decision() {
        let c = cfg();
        let d = uniform();
        for probs in [
            None,
            Some(DecisionProbs {
                top: 0.41,
                second: Some(0.409), // near-tie
                entropy: 0.0,
            }),
            Some(DecisionProbs {
                top: 0.999,
                second: Some(0.001), // obvious
                entropy: 0.0,
            }),
        ] {
            let mut i = input(&c, &d);
            i.probs = probs;
            let mut r = StdRng::seed_from_u64(AKAGI_SEED);
            let dec = decide(&i, &mut r);
            let mut r = StdRng::seed_from_u64(AKAGI_SEED);
            let mut base = input(&c, &d);
            base.probs = None;
            let expected = decide(&base, &mut r);
            assert_eq!(dec, expected, "probs {probs:?} must not change the outcome");
        }
    }

    /// The reading (table facts, second opinion) is for scripts; the
    /// built-in model draws the same with or without it.
    #[test]
    fn reading_does_not_affect_the_built_in_model() {
        let c = cfg();
        let d = uniform();
        let mut i = input(&c, &d);
        i.reading = Reading {
            loose: Some(true),
            runner_up_loose: Some(false),
            safe: Some(false),
            human: Some(HumanView {
                top: 0.9,
                second: Some(0.05),
                chosen: 0.05,
            }),
        };
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        assert_eq!(dec, decide(&input(&c, &d), &mut r));
    }

    // ------------------------------------------------------------------
    // Budget enforcement (soft/hard caps)
    // ------------------------------------------------------------------

    fn tight_budget_cfg() -> MajsoulAutoplayConfig {
        // Force the sampled target far above the caps so the cap is what
        // decides the outcome.
        let mut c = cfg();
        c.pre_click_delay_min_ms = 100_000;
        c.pre_click_delay_max_ms = 100_000;
        c
    }

    /// With `time_add == 0` the target never exceeds the soft cap
    /// (`fixed - safety_margin - click_overhead`).
    #[test]
    fn soft_cap_binds_without_bank() {
        let c = tight_budget_cfg();
        let d = uniform();
        let mut i = input(&c, &d);
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 5000,
            add_ms: 0,
            elapsed_ms: 0,
        });
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(dec.total_target_ms, 5000 - d.safety_margin_ms);
    }

    /// Click-sequence overhead (hover/hold, candidate clicks, riichi
    /// two-step) is reserved out of the window — multi-stage actions must
    /// not systematically overrun.
    #[test]
    fn click_overhead_reserved_from_cap() {
        let c = tight_budget_cfg();
        let d = uniform();
        let mut i = input(&c, &d);
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 5000,
            add_ms: 0,
            elapsed_ms: 0,
        });
        i.click_overhead_ms = 700;
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(dec.total_target_ms, 5000 - d.safety_margin_ms - 700);
    }

    /// With the long-thought gate off, the built-in model never spends
    /// the extra pool: the soft cap binds even against a fat bank.
    #[test]
    fn strict_mode_never_spends_bank() {
        let c = tight_budget_cfg();
        let d = DelayModelConfig {
            bank_on_long_thought: false,
            ..uniform()
        };
        let mut i = input(&c, &d);
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 5000,
            add_ms: 20_000,
            elapsed_ms: 0,
        });
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert!(!dec.allow_bank);
        assert_eq!(dec.total_target_ms, 5000 - d.safety_margin_ms);
    }

    /// Default (calibrated) behaviour: a base sample that naturally
    /// exceeds the soft cap is allowed to spend bank — real players dip
    /// into the pool routinely and it refills every kyoku — still
    /// bounded by the hard cap.
    #[test]
    fn long_thought_spends_bank_by_default() {
        let c = tight_budget_cfg(); // forces a 100s sample
        let d = uniform();
        assert!(d.bank_on_long_thought, "calibrated default must be on");
        let mut i = input(&c, &d);
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 5000,
            add_ms: 20_000,
            elapsed_ms: 0,
        });
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert!(dec.allow_bank, "long thought must unlock the bank");
        assert_eq!(
            dec.total_target_ms,
            5000 - d.safety_margin_ms + 5000u32.min(d.bank_max_single_ms),
            "and stay bounded by the hard cap"
        );
    }

    /// No budget known: the static ceiling trims rule bonuses, but never
    /// binds below the user's own configured base window — a config with
    /// pre_click_delay_max_ms above the cap keeps its legacy behaviour.
    #[test]
    fn static_cap_without_budget() {
        // Bonuses above the cap get trimmed to no_budget_cap_ms.
        let c = cfg(); // max 3000
        let d = DelayModelConfig {
            riichi_extra_ms: 60_000,
            ..uniform()
        };
        let mut i = input(&c, &d);
        i.can_riichi = true;
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(dec.total_target_ms, d.no_budget_cap_ms);

        // A configured base above the cap wins (legacy behaviour keeps).
        let c = tight_budget_cfg(); // min = max = 100_000
        let d = uniform();
        let i = input(&c, &d);
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(
            dec.total_target_ms, 100_000,
            "static cap must not bind below the configured base"
        );

        // 0 disables the static cap entirely.
        let open = DelayModelConfig {
            no_budget_cap_ms: 0,
            riichi_extra_ms: 60_000,
            ..uniform()
        };
        let mut i = input(&c, &open);
        i.can_riichi = true;
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(dec.total_target_ms, 160_000, "0 disables the static cap");
    }

    /// Button-clicking decisions (chi/pon/kan/ron/skip/riichi) carry the
    /// higher button floor — Majsoul renders action buttons later than
    /// hand tiles (discard animation + button pop-in).
    #[test]
    fn button_decisions_use_button_floor() {
        // Force tiny samples so the floor is what decides.
        let mut c = cfg();
        c.pre_click_delay_min_ms = 1;
        c.pre_click_delay_max_ms = 1;
        let d = uniform();

        for kind in [
            DecisionKind::Chi,
            DecisionKind::Pon,
            DecisionKind::Daiminkan,
            DecisionKind::Ankan,
            DecisionKind::Kakan,
            DecisionKind::Hora,
            DecisionKind::Reach,
            DecisionKind::Ryukyoku,
            DecisionKind::Pass,
            DecisionKind::Kita,
        ] {
            let mut i = input(&c, &d);
            i.kind = kind;
            let mut r = StdRng::seed_from_u64(AKAGI_SEED);
            let dec = decide(&i, &mut r);
            assert_eq!(
                dec.total_target_ms,
                d.min_button_delay_ms.ms(),
                "{kind:?} must sit on the button floor"
            );
        }

        // A plain discard keeps the lower tile floor.
        let i = input(&c, &d);
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert_eq!(dec.total_target_ms, d.min_delay_ms.ms());
    }

    /// The functional floor (animation wait) wins over the cap: losing
    /// the click to the animation is worse than shaving headroom.
    #[test]
    fn functional_floor_beats_cap() {
        let c = cfg(); // dealer pad 2000ms default
        let d = DelayModelConfig::default();
        let mut i = input(&c, &d);
        i.opening_animation = true;
        i.budget = Some(BudgetSnapshot {
            fixed_ms: 1500, // window smaller than the animation
            add_ms: 0,
            elapsed_ms: 0,
        });
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let dec = decide(&i, &mut r);
        assert!(
            dec.total_target_ms >= c.dealer_first_discard_extra_delay_ms.ms(),
            "floor must not be undercut by the cap"
        );
    }

    /// Distribution sanity: fixed seed, log-normal median lands near
    /// exp(mu) seconds and stays inside the loose clamp.
    #[test]
    fn lognormal_distribution_sanity() {
        let c = cfg();
        let mut d = DelayModelConfig::default();
        // e^0.6 ≈ 1.82s
        d.lognormal.insert("dahai_tedashi".into(), [0.6, 0.5]);
        let i = input(&c, &d);
        let mut r = StdRng::seed_from_u64(AKAGI_SEED);
        let mut samples: Vec<u32> = (0..10_000)
            .map(|_| decide(&i, &mut r).total_target_ms)
            .collect();
        samples.sort_unstable();
        let med = samples[samples.len() / 2];
        assert!(
            (1500..=2200).contains(&med),
            "log-normal median {med} out of tolerance"
        );
        let lo = c.pre_click_delay_min_ms / 2;
        let hi = c.pre_click_delay_max_ms * 4;
        assert!(*samples.first().unwrap() >= lo);
        assert!(*samples.last().unwrap() <= hi);
    }
}
