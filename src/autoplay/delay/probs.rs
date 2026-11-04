//! Normalize bot decision metadata into comparable probabilities.
//!
//! The results are exposed to user delay scripts (`ctx.top_prob` /
//! `ctx.second_prob` / `ctx.margin` / `ctx.entropy`); the built-in model
//! ignores them and the bundled default script reads none of them for the
//! think time itself, because measured bot policies can be nearly flat,
//! which turns confidence thresholds into permanent biases.
//! "How sure was the bot" is wanted as `top` / `second` probabilities in
//! `[0, 1]`, but each bot family reports something different:
//!
//! - The built-in native bot emits HUD metadata
//!   `{ show: { items: [{ label, pais, prob, value }] } }` where `prob`
//!   is a raw policy probability (already softmaxed). An item may also
//!   say what it is with `type` — the mjai event it stands for (`dahai`,
//!   `reach`, `pon`, ...) — which is what tells a runner-up discard from a
//!   button (see [`candidates`], [`runner_up`]).
//! - Mortal-style external mjai bots emit `{ q_values: [...] }` — **raw
//!   Q values**, not probabilities. They must be softmaxed before a
//!   threshold like "top > 0.995" means anything.
//!
//! Anything unrecognized normalizes to `None` and the delay model simply
//! skips its probability-based rules. Hard-coding thresholds against raw,
//! bot-specific metadata would silently change meaning per bot — that is
//! exactly what this module exists to prevent.

use serde_json::Value;

/// Top-two candidate probabilities, both in `[0, 1]`, and how spread the
/// whole decision was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionProbs {
    pub top: f64,
    /// `None` when the bot reported a single candidate (forced move).
    pub second: Option<f64>,
    /// Shannon entropy of the reported candidates, in nats: 0 for a forced
    /// move, `ln(n)` for `n` equally likely ones. A bot that lists only its
    /// likeliest candidates leaves some probability unlisted; that rest
    /// counts as one more candidate, which makes this a floor on the true
    /// figure rather than a guess at it.
    pub entropy: f64,
}

/// One candidate as the bot listed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The mjai event the candidate stands for (`dahai`, `reach`, `pon`,
    /// ...), when the bot says. A label is for people and is not read.
    pub kind: Option<String>,
    pub pais: Vec<String>,
    pub prob: f64,
}

/// The candidates of a bot's `meta.show.items` that carry a probability,
/// likeliest first. Empty when the bot lists none.
pub fn candidates(meta: Option<&Value>) -> Vec<Candidate> {
    let Some(items) = meta
        .and_then(|m| m.pointer("/show/items"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut found: Vec<Candidate> = items
        .iter()
        .filter_map(|item| {
            let prob = item.get("prob").and_then(Value::as_f64)?;
            (prob.is_finite() && prob >= 0.0).then(|| Candidate {
                kind: item.get("type").and_then(Value::as_str).map(str::to_owned),
                pais: item
                    .get("pais")
                    .and_then(Value::as_array)
                    .map(|pais| {
                        pais.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                prob,
            })
        })
        .collect();
    found.sort_by(|a, b| b.prob.total_cmp(&a.prob));
    found
}

/// The other tile, when the bot's two likeliest candidates are discards of two
/// different tiles and `chosen` is one of them. A red five and a plain one are
/// different tiles here. `None` otherwise — a button, an unnamed candidate, or
/// a choice that is neither of the two.
pub fn runner_up<'a>(candidates: &'a [Candidate], chosen: &str) -> Option<&'a str> {
    let [first, second, ..] = candidates else {
        return None;
    };
    let tile = |c: &'a Candidate| match (c.kind.as_deref(), c.pais.as_slice()) {
        (Some("dahai"), [pai]) => Some(pai.as_str()),
        _ => None,
    };
    let (a, b) = (tile(first)?, tile(second)?);
    match (a == chosen, b == chosen) {
        (true, false) => Some(b),
        (false, true) => Some(a),
        _ => None,
    }
}

impl DecisionProbs {
    /// `top - second`; `None` for a single-candidate decision.
    pub fn margin(&self) -> Option<f64> {
        self.second.map(|s| self.top - s)
    }
}

/// Interpret a bot's `meta` field. Returns `None` when no probability
/// information can be extracted.
pub fn normalize_meta(meta: Option<&Value>) -> Option<DecisionProbs> {
    let meta = meta?;

    // Native-bot HUD metadata: show.items[].prob (raw probabilities).
    if let Some(items) = meta.pointer("/show/items").and_then(Value::as_array) {
        let probs: Vec<f64> = items
            .iter()
            .filter_map(|i| i.get("prob").and_then(Value::as_f64))
            .filter(|p| p.is_finite() && *p >= 0.0)
            .collect();
        if let Some(result) = from_probs(probs) {
            return Some(result);
        }
    }

    // Mortal-style raw Q values: softmax first.
    if let Some(q) = meta.get("q_values").and_then(Value::as_array) {
        let vals: Vec<f64> = q
            .iter()
            .filter_map(Value::as_f64)
            .filter(|v| v.is_finite())
            .collect();
        if !vals.is_empty() {
            return from_probs(softmax(&vals));
        }
    }

    None
}

fn from_probs(mut probs: Vec<f64>) -> Option<DecisionProbs> {
    if probs.is_empty() {
        return None;
    }
    probs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Some(DecisionProbs {
        top: probs[0],
        second: probs.get(1).copied(),
        entropy: entropy(&probs),
    })
}

/// Entropy in nats of `probs`, with whatever they leave short of 1 counted
/// as one further outcome.
fn entropy(probs: &[f64]) -> f64 {
    let listed: f64 = probs.iter().sum();
    let rest = (1.0 - listed).max(0.0);
    // Probabilities that overshoot 1 are a rounding matter; scale them back.
    let scale = if listed > 1.0 { listed } else { 1.0 };
    probs
        .iter()
        .map(|p| p / scale)
        .chain(std::iter::once(rest))
        .filter(|p| *p > 0.0)
        .map(|p| -p * p.ln())
        .sum()
}

/// Numerically stable softmax.
fn softmax(vals: &[f64]) -> Vec<f64> {
    let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = vals.iter().map(|v| (v - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    exps.into_iter().map(|e| e / sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_show_meta_uses_raw_probs() {
        let meta = json!({
            "show": {
                "title": "Akagi",
                "items": [
                    { "label": "Discard", "pais": ["5p"], "prob": 0.62, "value": "62%" },
                    { "label": "Discard", "pais": ["1m"], "prob": 0.30, "value": "30%" },
                    { "label": "Riichi", "prob": 0.08, "value": "8%" },
                ],
            },
        });
        let p = normalize_meta(Some(&meta)).unwrap();
        assert!((p.top - 0.62).abs() < 1e-9);
        assert!((p.second.unwrap() - 0.30).abs() < 1e-9);
        assert!((p.margin().unwrap() - 0.32).abs() < 1e-9);
    }

    /// Raw Q values must be softmaxed, not consumed as probabilities:
    /// equal Qs mean a dead-even decision, whatever their magnitude.
    #[test]
    fn q_values_are_softmaxed() {
        let meta = json!({ "q_values": [5.0, 5.0] });
        let p = normalize_meta(Some(&meta)).unwrap();
        assert!((p.top - 0.5).abs() < 1e-9);
        assert!((p.second.unwrap() - 0.5).abs() < 1e-9);
        assert!(p.margin().unwrap().abs() < 1e-9);

        // A dominant Q value maps to a near-1 probability.
        let meta = json!({ "q_values": [10.0, -10.0, -10.0] });
        let p = normalize_meta(Some(&meta)).unwrap();
        assert!(p.top > 0.995, "dominant Q must normalize near 1");
    }

    #[test]
    fn single_candidate_has_no_margin() {
        let meta = json!({ "q_values": [1.25] });
        let p = normalize_meta(Some(&meta)).unwrap();
        assert!((p.top - 1.0).abs() < 1e-9, "softmax of one value is 1");
        assert!(p.second.is_none());
        assert!(p.margin().is_none());
    }

    #[test]
    fn entropy_measures_how_spread_the_decision_was() {
        let forced = normalize_meta(Some(&json!({ "q_values": [1.25] }))).unwrap();
        assert!(forced.entropy.abs() < 1e-12);
        let even = normalize_meta(Some(&json!({ "q_values": [2.0, 2.0, 2.0, 2.0] }))).unwrap();
        assert!((even.entropy - 4f64.ln()).abs() < 1e-9);
        // A list that stops short of 1 keeps the rest as one more outcome.
        let short = json!({ "show": { "items": [{ "prob": 0.5 }, { "prob": 0.3 }] } });
        let expected = -(0.5f64 * 0.5f64.ln() + 0.3 * 0.3f64.ln() + 0.2 * 0.2f64.ln());
        assert!((normalize_meta(Some(&short)).unwrap().entropy - expected).abs() < 1e-9);
        let sure = json!({ "show": { "items": [{ "prob": 0.97 }, { "prob": 0.02 }] } });
        assert!(normalize_meta(Some(&sure)).unwrap().entropy < 0.2);
    }

    #[test]
    fn candidates_are_read_likeliest_first_with_what_they_are() {
        let meta = json!({ "show": { "items": [
            { "label": "Riichi", "type": "reach", "prob": 0.08 },
            { "label": "Discard", "type": "dahai", "pais": ["5p"], "prob": 0.62 },
            { "label": "Discard", "pais": ["1m"], "prob": 0.30 },
            { "label": "Discard", "type": "dahai", "pais": ["E"], "value": "1%" },
        ] } });
        let found = candidates(Some(&meta));
        assert_eq!(
            found.len(),
            3,
            "an item without a probability is not a candidate"
        );
        assert_eq!(found[0].kind.as_deref(), Some("dahai"));
        assert_eq!(found[0].pais, ["5p"]);
        assert_eq!(found[1].kind, None);
        assert_eq!(found[2].kind.as_deref(), Some("reach"));
        assert!(found[2].pais.is_empty());
        assert!(candidates(None).is_empty());
        assert!(candidates(Some(&json!({ "q_values": [1.0, 2.0] }))).is_empty());
    }

    #[test]
    fn unknown_meta_is_none() {
        assert!(normalize_meta(None).is_none());
        assert!(normalize_meta(Some(&json!({}))).is_none());
        assert!(normalize_meta(Some(&json!({ "confidence": 0.9 }))).is_none());
        // show.items without raw probs (old format) — no fabricated numbers.
        let old = json!({ "show": { "items": [{ "label": "Discard", "value": "62%" }] } });
        assert!(normalize_meta(Some(&old)).is_none());
    }

    fn discard(pai: &str, prob: f64) -> Candidate {
        Candidate {
            kind: Some("dahai".into()),
            pais: vec![pai.into()],
            prob,
        }
    }

    #[test]
    fn the_runner_up_is_the_other_of_two_different_discards() {
        let two = [discard("5p", 0.48), discard("1m", 0.41), discard("E", 0.05)];
        assert_eq!(runner_up(&two, "5p"), Some("1m"));
        assert_eq!(runner_up(&two, "1m"), Some("5p"));
        // The choice is neither of the two likeliest: nothing to compare.
        assert_eq!(runner_up(&two, "E"), None);
        // A red five and a plain one are different tiles.
        assert_eq!(
            runner_up(&[discard("5pr", 0.5), discard("5p", 0.4)], "5p"),
            Some("5pr")
        );
    }

    #[test]
    fn no_runner_up_between_one_tile_or_a_button() {
        // The same tile, from the hand and from the draw.
        assert_eq!(
            runner_up(&[discard("5p", 0.5), discard("5p", 0.4)], "5p"),
            None
        );
        let reach = Candidate {
            kind: Some("reach".into()),
            pais: vec![],
            prob: 0.4,
        };
        assert_eq!(runner_up(&[discard("5p", 0.5), reach], "5p"), None);
        // A candidate that does not say what it is is not assumed a discard.
        let unnamed = Candidate {
            kind: None,
            pais: vec!["1m".into()],
            prob: 0.4,
        };
        assert_eq!(runner_up(&[discard("5p", 0.5), unnamed], "5p"), None);
        assert_eq!(runner_up(&[discard("5p", 0.5)], "5p"), None);
    }
}
