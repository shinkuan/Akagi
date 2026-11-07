//! Live inference engine: maintains a riichienv-core game state from mjai
//! events and, at our decision points, runs the CNN to pick a **legal** action.
//!
//! Transport-agnostic: it consumes `riichienv_core::replay::MjaiEvent` and
//! returns a schema-agnostic [`BotAction`] (mjai tile strings), which the
//! Akagi-side `NativeBot` maps to Akagi's own mjai event type. This keeps
//! `native_bot` free of any Akagi dependency.
//!
//! `decide()` is read-only with respect to game *state*: our own chosen action
//! is NOT applied here. In Akagi, every action (ours included) echoes back
//! through the mjai bus and is applied via [`Engine::feed`], so applying it in
//! `decide()` too would double-count.

use anyhow::Result;
use riichienv_core::action::{Action, ActionType};
use riichienv_core::parser::tid_to_mjai;
use riichienv_core::replay::MjaiEvent;
use riichienv_core::rule::GameRule;
use riichienv_core::state::GameState;
use riichienv_core::state_3p::GameState3P;

use crate::action_codec::{pick_by_logits, rank_by_logits};
use crate::adapt::{obs_and_legal_3p, obs_and_legal_4p};
use crate::mjai_compat::{parse_line, sanitize_3p};
use crate::model::Model;

/// How many ranked candidates the engine surfaces for the HUD's multi-row
/// recommendation card (top-N by policy probability).
const SHOW_TOP_N: usize = 3;

/// A schema-agnostic bot reply, ready to be mapped to Akagi's `MjaiEvent`.
/// All tiles are mjai strings (e.g. `"5mr"`, `"P"`).
#[derive(Debug, Clone, PartialEq)]
pub enum BotAction {
    Dahai {
        pai: String,
        tsumogiri: bool,
    },
    /// Riichi declaration; `pai` is the predicted riichi discard (mjai must
    /// carry it or autoplay stalls). Always non-empty on [`Decision::action`] —
    /// `decide` drops a riichi it can't name a discard for. Runner-up
    /// `candidates` rows carry an empty `pai`: they only decorate the HUD, and
    /// predicting a discard for each would run the model once per row.
    Reach {
        pai: String,
    },
    Pon {
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Chi {
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Daiminkan {
        target: u8,
        pai: String,
        consumed: Vec<String>,
    },
    Ankan {
        consumed: Vec<String>,
    },
    Kakan {
        pai: String,
        consumed: Vec<String>,
    },
    /// Ron or tsumo (both are mjai `hora`); `target` is the loser (self for
    /// tsumo, the discarder for ron).
    Hora {
        target: u8,
    },
    /// Nine-terminals abortive draw (mjai `ryukyoku`).
    Kyushu,
    /// Kita / nukidora (sanma).
    Kita,
    /// No action this turn.
    Pass,
}

/// Our seat's decision at a given point.
pub struct Decision {
    pub action: BotAction,
    /// Top legal actions ranked by policy probability (best first). `action`
    /// is `candidates[0].0`; the rest are the runner-up recommendations the HUD
    /// shows as a top-N card. Probabilities are a softmax over the legal set.
    pub candidates: Vec<(BotAction, f32)>,
    /// Raw action logits (indexed by the mode's action space).
    pub logits: Vec<f32>,
    /// The full legal set (before the top-N cut in `candidates`) held exactly
    /// one action, so this "decision" is forced — any policy, local or remote,
    /// can only play `action`. Callers that pay per query (the online API) use
    /// this to answer locally instead of spending a call on a foregone move.
    pub forced: bool,
}

enum Backend {
    Four {
        state: Box<GameState>,
        model: Model,
    },
    Three {
        state: Box<GameState3P>,
        model: Model,
    },
}

pub struct Engine {
    backend: Backend,
    seat: u8,
    num_players: u8,
}

impl Engine {
    /// Construct for `num_players`, loading weights from a safetensors buffer.
    pub fn new(model_bytes: Vec<u8>, num_players: u8, seat: u8) -> Result<Self> {
        let rule = GameRule::default_tenhou();
        let model = Model::from_safetensors(model_bytes, num_players)?;
        let backend = if num_players == 3 {
            Backend::Three {
                state: Box::new(GameState3P::new(0, true, None, 0, rule)),
                model,
            }
        } else {
            Backend::Four {
                state: Box::new(GameState::new(0, true, None, 0, rule)),
                model,
            }
        };
        Ok(Self {
            backend,
            seat,
            num_players,
        })
    }

    /// Reset to a fresh game while keeping the loaded weights.
    pub fn reset(&mut self) {
        let rule = GameRule::default_tenhou();
        match &mut self.backend {
            Backend::Four { state, .. } => **state = GameState::new(0, true, None, 0, rule),
            Backend::Three { state, .. } => **state = GameState3P::new(0, true, None, 0, rule),
        }
    }

    pub fn seat(&self) -> u8 {
        self.seat
    }

    pub fn set_seat(&mut self, seat: u8) {
        self.seat = seat;
    }

    pub fn num_players(&self) -> u8 {
        self.num_players
    }

    /// Drive one already-parsed mjai event through the engine.
    ///
    /// Sanma events are sanitized first: Tenhou sanma logs carry 4-element
    /// `scores`/`tehais` arrays that a 3-seat `GameState3P` would index out of
    /// bounds on. Akagi's live bridge builds its events itself and never trips
    /// this, but the extractor's logs do.
    ///
    /// This can only fix what survives parsing. Raw JSONL needs
    /// [`Engine::feed_line`] (or [`crate::mjai_compat::parse_line`]): a sanma
    /// log's `nukidora` has to be renamed to `kita` *before* serde sees it, or
    /// it deserializes into `MjaiEvent::Other` and the event is lost.
    pub fn feed(&mut self, ev: MjaiEvent) {
        // The chankan check needs the kakan's actor/tile after the event is
        // applied — `apply_mjai_event` consumes it.
        let opponent_kakan = match &ev {
            MjaiEvent::Kakan { actor, pai } if *actor as u8 != self.seat => {
                Some((*actor as u8, pai.clone()))
            }
            _ => None,
        };
        match &mut self.backend {
            Backend::Four { state, .. } => {
                state.apply_mjai_event(ev);
                if let Some((actor, pai)) = opponent_kakan {
                    crate::chankan::open_on_kakan(state, actor, &pai, self.seat);
                }
            }
            Backend::Three { state, .. } => {
                let mut ev = ev;
                sanitize_3p(&mut ev);
                state.apply_mjai_event(ev);
                if let Some((actor, pai)) = opponent_kakan {
                    crate::chankan::open_on_kakan_3p(state, actor, &pai, self.seat);
                }
            }
        }
    }

    /// Drive one raw mjai JSONL line through the engine, applying both
    /// compatibility fixups. Returns `false` for a line the engine doesn't model
    /// (blank, malformed, or an event type riichienv has no variant for), which
    /// callers replaying a log simply skip.
    pub fn feed_line(&mut self, line: &str) -> bool {
        match parse_line(line, self.num_players) {
            Some(ev) => {
                self.feed(ev);
                true
            }
            None => false,
        }
    }

    /// Decide our action at the current state. `None` if we currently have no
    /// legal action (not our turn / nothing to respond to).
    pub fn decide(&mut self) -> Result<Option<Decision>> {
        let seat = self.seat;
        let (mut ranked, logits, last_discarder, drawn, reach_pai, forced) = match &mut self.backend
        {
            Backend::Four { state, model } => {
                // `last_discard` is `(discarder_pid, tile)`.
                let last_discarder = state.last_discard.map(|(pid, _tile)| pid);
                let drawn = state.drawn_tile;
                let (obs, legal) = obs_and_legal_4p(state, seat);
                if legal.is_empty() {
                    return Ok(None);
                }
                // Forced ⇔ the *legal* set is a singleton — `ranked` is cut to
                // SHOW_TOP_N below, so its length can't be used for this.
                let forced = legal.len() == 1;
                let logits = model.forward_logits(&obs)?;
                let ranked = rank_by_logits(&legal, &logits, 4, SHOW_TOP_N);
                let Some((top, _)) = ranked.first() else {
                    return Ok(None);
                };
                let reach_pai = if top.action_type == ActionType::Riichi {
                    predict_reach_discard(state, model, seat, 4)
                } else {
                    None
                };
                (ranked, logits, last_discarder, drawn, reach_pai, forced)
            }
            Backend::Three { state, model } => {
                let last_discarder = state.last_discard.map(|(pid, _tile)| pid);
                let drawn = state.drawn_tile;
                let (obs, legal) = obs_and_legal_3p(state, seat);
                if legal.is_empty() {
                    return Ok(None);
                }
                let forced = legal.len() == 1;
                let logits = model.forward_logits(&obs)?;
                let ranked = rank_by_logits(&legal, &logits, 3, SHOW_TOP_N);
                let Some((top, _)) = ranked.first() else {
                    return Ok(None);
                };
                let reach_pai = if top.action_type == ActionType::Riichi {
                    predict_reach_discard_3p(state, model, seat)
                } else {
                    None
                };
                (ranked, logits, last_discarder, drawn, reach_pai, forced)
            }
        };

        // An mjai `reach` must name the discard or autoplay stalls (Majsoul fuses
        // declaring and discarding into one click). If the riichi-discard
        // prediction failed, drop riichi from the ranking rather than emit a
        // reach we cannot complete — the next-best action is a plain discard.
        if reach_pai.is_none()
            && matches!(ranked.first(), Some((a, _)) if a.action_type == ActionType::Riichi)
        {
            ranked.retain(|(a, _)| a.action_type != ActionType::Riichi);
        }
        if ranked.is_empty() {
            return Ok(None);
        }

        let candidates = build_candidates(&ranked, seat, last_discarder, drawn, reach_pai);
        let action = candidates[0].0.clone();
        Ok(Some(Decision {
            action,
            candidates,
            logits,
            forced,
        }))
    }

    /// The tile the local model would discard if it declared riichi right now,
    /// as an mjai string. Used by the API-backed runner as a fallback for the
    /// reach two-step (declare → discard) when the remote follow-up call fails.
    /// `None` if there is no riichi-legal discard from the current state.
    pub fn reach_discard(&mut self) -> Option<String> {
        let seat = self.seat;
        match &mut self.backend {
            Backend::Four { state, model } => {
                predict_reach_discard(state, model, seat, 4).map(tid_to_mjai)
            }
            Backend::Three { state, model } => {
                predict_reach_discard_3p(state, model, seat).map(tid_to_mjai)
            }
        }
    }
}

/// Map a ranked `(Action, prob)` list into displayable `(BotAction, prob)`
/// candidates. The riichi-discard prediction is applied only to the top action
/// (index 0) — predicting it for every runner-up would run the model N extra
/// times for tiles that only decorate an alternative row.
fn build_candidates(
    ranked: &[(Action, f32)],
    seat: u8,
    last_discarder: Option<u8>,
    drawn: Option<u8>,
    reach_pai: Option<u8>,
) -> Vec<(BotAction, f32)> {
    ranked
        .iter()
        .enumerate()
        .map(|(i, (a, p))| {
            let rp = if i == 0 { reach_pai } else { None };
            (build_bot_action(a, seat, last_discarder, drawn, rp), *p)
        })
        .collect()
}

/// Predict the tile we'd discard on a riichi declaration, by advancing a clone
/// past the reach and asking the model for the (riichi-legal) discard.
fn predict_reach_discard(state: &GameState, model: &Model, seat: u8, np: u8) -> Option<u8> {
    let mut clone = state.clone();
    clone.apply_mjai_event(MjaiEvent::Reach {
        actor: seat as usize,
    });
    let (obs, legal) = obs_and_legal_4p(&mut clone, seat);
    if legal.is_empty() {
        return None;
    }
    let logits = model.forward_logits(&obs).ok()?;
    let a = pick_by_logits(&legal, &logits, np)?;
    a.tile
}

fn predict_reach_discard_3p(state: &GameState3P, model: &Model, seat: u8) -> Option<u8> {
    let mut clone = state.clone();
    clone.apply_mjai_event(MjaiEvent::Reach {
        actor: seat as usize,
    });
    let (obs, legal) = obs_and_legal_3p(&mut clone, seat);
    if legal.is_empty() {
        return None;
    }
    let logits = model.forward_logits(&obs).ok()?;
    let a = pick_by_logits(&legal, &logits, 3)?;
    a.tile
}

/// Turn a riichienv `Action` (plus the little bit of table context the reply
/// needs) into a [`BotAction`].
///
/// `last_discarder` is a **seat id**, taken from `GameState::last_discard.0`.
/// It becomes the `target` of every claim (pon/chi/daiminkan) and of a ron —
/// mjai consumers use it to identify the losing seat, so feeding a tile id here
/// produces an out-of-range seat.
fn build_bot_action(
    a: &Action,
    seat: u8,
    last_discarder: Option<u8>,
    drawn: Option<u8>,
    reach_pai: Option<u8>,
) -> BotAction {
    let consumed = |a: &Action| a.consume_tiles.iter().map(|&t| tid_to_mjai(t)).collect();
    let target = last_discarder.unwrap_or(0);
    match a.action_type {
        ActionType::Discard => match a.tile {
            Some(t) => BotAction::Dahai {
                pai: tid_to_mjai(t),
                tsumogiri: drawn == Some(t),
            },
            None => BotAction::Pass,
        },
        ActionType::Riichi => BotAction::Reach {
            pai: reach_pai.map(tid_to_mjai).unwrap_or_default(),
        },
        ActionType::Pon => BotAction::Pon {
            target,
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Chi => BotAction::Chi {
            target,
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Daiminkan => BotAction::Daiminkan {
            target,
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Ankan => BotAction::Ankan {
            consumed: consumed(a),
        },
        ActionType::Kakan => BotAction::Kakan {
            pai: a.tile.map(tid_to_mjai).unwrap_or_default(),
            consumed: consumed(a),
        },
        ActionType::Tsumo => BotAction::Hora { target: seat },
        ActionType::Ron => BotAction::Hora {
            target: last_discarder.unwrap_or(seat),
        },
        ActionType::KyushuKyuhai => BotAction::Kyushu,
        ActionType::Kita => BotAction::Kita,
        ActionType::Pass => BotAction::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riichienv_core::replay::MjaiEvent;

    fn ev(line: &str) -> MjaiEvent {
        serde_json::from_str(line).expect("valid mjai event")
    }

    fn act(kind: ActionType, tile: Option<u8>, consumed: Vec<u8>) -> Action {
        Action::new(kind, tile, consumed, Some(0))
    }

    // ---------- `build_bot_action` field mapping (pure) ----------

    /// Every claim and a ron carry the **discarder's seat** as `target`.
    #[test]
    fn claims_and_ron_target_the_discarder_seat() {
        let discarder = Some(3u8);
        let seat = 0u8;

        let pon = build_bot_action(
            &act(ActionType::Pon, Some(4), vec![5, 6]),
            seat,
            discarder,
            None,
            None,
        );
        assert_eq!(
            pon,
            BotAction::Pon {
                target: 3,
                pai: "2m".into(),
                consumed: vec!["2m".into(), "2m".into()],
            }
        );

        let chi = build_bot_action(
            &act(ActionType::Chi, Some(4), vec![8, 12]),
            seat,
            discarder,
            None,
            None,
        );
        assert!(matches!(chi, BotAction::Chi { target: 3, .. }));

        let kan = build_bot_action(
            &act(ActionType::Daiminkan, Some(4), vec![5, 6, 7]),
            seat,
            discarder,
            None,
            None,
        );
        assert!(matches!(kan, BotAction::Daiminkan { target: 3, .. }));

        let ron = build_bot_action(
            &act(ActionType::Ron, None, vec![]),
            seat,
            discarder,
            None,
            None,
        );
        assert_eq!(ron, BotAction::Hora { target: 3 });
    }

    /// A tsumo is a self-target hora; a ron with no recorded discarder degrades
    /// to self rather than an out-of-range seat.
    #[test]
    fn tsumo_targets_self() {
        let seat = 2u8;
        let tsumo = build_bot_action(
            &act(ActionType::Tsumo, None, vec![]),
            seat,
            Some(1),
            None,
            None,
        );
        assert_eq!(tsumo, BotAction::Hora { target: seat });

        let ron_no_discarder =
            build_bot_action(&act(ActionType::Ron, None, vec![]), seat, None, None, None);
        assert_eq!(ron_no_discarder, BotAction::Hora { target: seat });
    }

    #[test]
    fn discard_marks_tsumogiri_only_for_the_drawn_tile() {
        let drawn = Some(88u8); // 5sr
        let a = act(ActionType::Discard, Some(88), vec![]);
        assert_eq!(
            build_bot_action(&a, 0, None, drawn, None),
            BotAction::Dahai {
                pai: "5sr".into(),
                tsumogiri: true
            }
        );
        let a = act(ActionType::Discard, Some(0), vec![]);
        assert_eq!(
            build_bot_action(&a, 0, None, drawn, None),
            BotAction::Dahai {
                pai: "1m".into(),
                tsumogiri: false
            }
        );
    }

    #[test]
    fn closed_calls_and_terminals_map_without_a_target() {
        assert_eq!(
            build_bot_action(
                &act(ActionType::Ankan, None, vec![0, 1, 2, 3]),
                0,
                Some(2),
                None,
                None
            ),
            BotAction::Ankan {
                consumed: vec!["1m".into(), "1m".into(), "1m".into(), "1m".into()],
            }
        );
        assert!(matches!(
            build_bot_action(
                &act(ActionType::Kakan, Some(0), vec![1]),
                0,
                Some(2),
                None,
                None
            ),
            BotAction::Kakan { .. }
        ));
        assert_eq!(
            build_bot_action(
                &act(ActionType::KyushuKyuhai, None, vec![]),
                0,
                None,
                None,
                None
            ),
            BotAction::Kyushu
        );
        assert_eq!(
            build_bot_action(&act(ActionType::Kita, None, vec![]), 0, None, None, None),
            BotAction::Kita
        );
        assert_eq!(
            build_bot_action(&act(ActionType::Pass, None, vec![]), 0, None, None, None),
            BotAction::Pass
        );
    }

    #[test]
    fn riichi_carries_the_predicted_discard() {
        let a = act(ActionType::Riichi, None, vec![]);
        assert_eq!(
            build_bot_action(&a, 0, None, None, Some(36)),
            BotAction::Reach { pai: "1p".into() }
        );
    }

    // ---------- end-to-end: the seat/tile confusion `decide()` used to have ----------

    /// Regression: `GameState::last_discard` is `(discarder_pid, tile)`, but
    /// `decide()` destructured it as `(_, pid)` and handed the **tile id** to
    /// `build_bot_action` as the discarder's seat. Every pon/chi/kan/ron reply
    /// then carried a `target` of 0..=135 instead of a seat number, which lands
    /// straight on Akagi's mjai bus.
    #[test]
    fn claim_target_is_a_seat_not_a_tile_id() {
        // Our seat holds two 9s (tile ids 104..107), so seat 1's 9s discard opens
        // a pon window. 9s is tile34 26 — a tile id far outside the seat range,
        // so a regressed `target` can never coincide with the right answer.
        let ours = r#"["9s","9s","1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p"]"#;
        let other = r#"["1m","2m","3m","4m","5m","6m","7m","8m","9m","1p","2p","3p","4p"]"#;
        let mut eng = crate::defaults::engine(4, 0).expect("bundled 4p weights");
        eng.feed(ev(r#"{"type":"start_game","names":["a","b","c","d"]}"#));
        eng.feed(ev(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"2m","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[25000,25000,25000,25000],
                 "tehais":[{ours},{other},{other},{other}]}}"#
        )));
        eng.feed(ev(r#"{"type":"tsumo","actor":1,"pai":"9s"}"#));
        eng.feed(ev(
            r#"{"type":"dahai","actor":1,"pai":"9s","tsumogiri":true}"#,
        ));

        let d = eng
            .decide()
            .expect("forward pass")
            .expect("a pon window is a decision point");

        let mut saw_pon = false;
        for (a, _) in &d.candidates {
            match a {
                BotAction::Pon { target, .. }
                | BotAction::Chi { target, .. }
                | BotAction::Daiminkan { target, .. }
                | BotAction::Hora { target } => {
                    assert_eq!(
                        *target, 1,
                        "target must be the discarder's seat, got {target}"
                    );
                    saw_pon |= matches!(a, BotAction::Pon { .. });
                }
                _ => {}
            }
        }
        assert!(
            saw_pon,
            "fixture invalid: no pon offered on the 9s discard ({:?})",
            d.candidates.iter().map(|(a, _)| a).collect::<Vec<_>>()
        );
    }

    /// Regression: `feed_line` must apply the pre-parse `nukidora`→`kita` rename.
    /// Parsed straight through serde, a Tenhou sanma nukidora becomes
    /// `MjaiEvent::Other` and is dropped — the engine's kita counts would drift
    /// silently away from the real table.
    #[test]
    fn feed_line_applies_the_nukidora_rename() {
        let h = r#"["1m","9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","N","N"]"#;
        let mut eng = crate::defaults::engine(3, 0).expect("bundled 3p weights");
        assert!(eng.feed_line(r#"{"type":"start_game","names":["a","b","c",""]}"#));
        assert!(eng.feed_line(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],
                 "tehais":[{h},{h},{h},{h}]}}"#
        )));
        assert!(eng.feed_line(r#"{"type":"tsumo","actor":0,"pai":"9p"}"#));
        assert!(
            eng.feed_line(r#"{"type":"nukidora","actor":0}"#),
            "a nukidora must reach the engine, not be skipped as an unknown type"
        );

        // Unmodelled / blank / malformed lines are skipped, not applied.
        assert!(!eng.feed_line(""));
        assert!(!eng.feed_line("{oops"));
        assert!(!eng.feed_line(r#"{"type":"some_future_event"}"#));
    }

    /// A sanma engine must survive a Tenhou-style 4-seat `start_kyoku` (the
    /// extractor sanitizes these; `feed` used to pass them straight through to a
    /// 3-seat state, which indexes out of bounds).
    #[test]
    fn feed_sanitizes_four_seat_sanma_start_kyoku() {
        let h = r#"["1m","9m","1p","2p","3p","4p","5p","6p","7p","8p","9p","1s","2s"]"#;
        let mut eng = crate::defaults::engine(3, 0).expect("bundled 3p weights");
        eng.feed(ev(r#"{"type":"start_game","names":["a","b","c","d"]}"#));
        eng.feed(ev(&format!(
            r#"{{"type":"start_kyoku","bakaze":"E","dora_marker":"1s","kyoku":1,"honba":0,
                 "kyotaku":0,"oya":0,"scores":[35000,35000,35000,0],
                 "tehais":[{h},{h},{h},{h}]}}"#
        )));
        eng.feed(ev(r#"{"type":"tsumo","actor":0,"pai":"9p"}"#));
        let d = eng.decide().expect("forward pass");
        assert!(d.is_some(), "our own draw is a decision point");
    }
}
