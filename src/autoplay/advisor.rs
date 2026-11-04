//! A second opinion on each decision, from the built-in bot.
//!
//! Adapted from Washizu (<https://github.com/nanomena/Washizu>, Apache-2.0).
//!
//! The built-in bot imitates people (`native_bot`): its policy is, near
//! enough, the share of players who make each move in a position. That is no
//! reason to play its move — the active bot decides, and nothing here can
//! change what it decided. It is a measure of how *obvious* the active bot's
//! move would be to a person, which is what a thinking time ought to follow:
//! where players split — pushing or folding, picking a tile to fold with,
//! choosing between shapes — a person takes time, and where everyone does the
//! same thing nobody does.
//!
//! A bot's own probabilities cannot stand in for this. How spread they are
//! says how the *bot* weighs its options, which need not be how hard the
//! position is for a person.
//!
//! The advisor follows the game on the same events as the active bot and is
//! asked only when a decision is being paced. It is an extra: an event it
//! cannot follow, a decision point it does not recognise, or a panic inside
//! the engine ends its part in the game and nothing else.

use crate::autoplay::delay::HumanView;
use crate::game_state::convert;
use crate::schema::MjaiEvent;
use native_bot::engine::{BotAction, Engine};
use std::panic::{catch_unwind, AssertUnwindSafe};
use tracing::warn;

/// The built-in bot, following one game.
#[derive(Default)]
pub struct Advisor {
    engine: Option<Engine>,
    /// Events seen since it was last asked; fed in when it is asked next.
    pending: Vec<MjaiEvent>,
}

impl Advisor {
    /// Take note of a game event. A `start_game` that names our seat begins
    /// following, with the 4p or 3p model as the table needs; one that does
    /// not (an observed game) and `end_game` stop.
    pub fn note(&mut self, ev: &MjaiEvent) {
        match ev {
            MjaiEvent::StartGame {
                id, num_players, ..
            } => {
                self.pending.clear();
                self.engine = id.and_then(|seat| {
                    native_bot::defaults::engine(*num_players, seat)
                        .map_err(|e| warn!("autoplay: no second opinion for this game: {e:#}"))
                        .ok()
                });
            }
            MjaiEvent::EndGame { .. } => {
                self.engine = None;
                self.pending.clear();
            }
            _ => {}
        }
        if self.engine.is_some() {
            self.pending.push(ev.clone());
        }
    }

    /// How people would see `action`, the move the active bot has just
    /// chosen. `None` when the advisor is not following the game or is not at
    /// the decision the action answers.
    pub fn view(&mut self, action: &MjaiEvent) -> Option<HumanView> {
        let engine = self.engine.as_mut()?;
        let pending = std::mem::take(&mut self.pending);
        let decided = catch_unwind(AssertUnwindSafe(|| {
            for ev in &pending {
                if let Ok(Some(event)) = convert::to_riichienv(ev) {
                    engine.feed(event);
                }
            }
            engine.decide()
        }));
        match decided {
            Ok(Ok(Some(decision))) => view_of(&decision.candidates, action),
            Ok(Ok(None)) => None,
            Ok(Err(e)) => {
                warn!("autoplay: the second opinion failed and is off for this game: {e:#}");
                self.engine = None;
                None
            }
            Err(_) => {
                warn!("autoplay: the second opinion panicked and is off for this game");
                self.engine = None;
                None
            }
        }
    }
}

/// Whether `action` is made on our own turn (as opposed to answering someone
/// else's tile).
fn on_own_turn(action: &MjaiEvent) -> bool {
    match action {
        MjaiEvent::Dahai { .. }
        | MjaiEvent::Reach { .. }
        | MjaiEvent::Ankan { .. }
        | MjaiEvent::Kakan { .. }
        | MjaiEvent::Kita { .. }
        | MjaiEvent::Ryukyoku { .. } => true,
        MjaiEvent::Hora { actor, target, .. } => actor == target,
        _ => false,
    }
}

/// Whether the built-in bot's `candidate` is the move `action`. Calls are
/// matched by what they are, not by which tiles they take: "a chi here" is
/// what people are being counted on.
fn same_move(candidate: &BotAction, action: &MjaiEvent) -> bool {
    match (candidate, action) {
        (BotAction::Dahai { pai, .. }, MjaiEvent::Dahai { pai: played, .. }) => pai == played,
        (BotAction::Reach { .. }, MjaiEvent::Reach { .. })
        | (BotAction::Pon { .. }, MjaiEvent::Pon { .. })
        | (BotAction::Chi { .. }, MjaiEvent::Chi { .. })
        | (BotAction::Daiminkan { .. }, MjaiEvent::Daiminkan { .. })
        | (BotAction::Ankan { .. }, MjaiEvent::Ankan { .. })
        | (BotAction::Kakan { .. }, MjaiEvent::Kakan { .. })
        | (BotAction::Hora { .. }, MjaiEvent::Hora { .. })
        | (BotAction::Kyushu, MjaiEvent::Ryukyoku { .. })
        | (BotAction::Kita, MjaiEvent::Kita { .. })
        | (BotAction::Pass, MjaiEvent::None) => true,
        _ => false,
    }
}

/// Read the built-in bot's ranked `candidates` against the move that was
/// actually chosen.
fn view_of(candidates: &[(BotAction, f32)], action: &MjaiEvent) -> Option<HumanView> {
    let (_, top) = candidates.first()?;
    // The advisor sees the same events as the active bot but not at the same
    // instant: if it is looking at a different kind of decision — a discard
    // where the action answers a call, or the reverse — it is a step behind,
    // and its numbers are about something else.
    let discarding = candidates
        .iter()
        .any(|(c, _)| matches!(c, BotAction::Dahai { .. }));
    if discarding != on_own_turn(action) {
        return None;
    }
    let listed: f64 = candidates.iter().map(|(_, p)| f64::from(*p)).sum();
    let matched: f64 = candidates
        .iter()
        .filter(|(c, _)| same_move(c, action))
        .map(|(_, p)| f64::from(*p))
        .sum();
    // Only the likeliest few are listed. A move that is not among them has at
    // most the last listed share, and at most what the listed ones leave.
    let chosen = if matched > 0.0 {
        matched
    } else {
        let last = candidates.last().map_or(0.0, |(_, p)| f64::from(*p));
        last.min((1.0 - listed).max(0.0)) / 2.0
    };
    Some(HumanView {
        top: f64::from(*top),
        second: candidates.get(1).map(|(_, p)| f64::from(*p)),
        chosen: chosen.clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dahai(pai: &str) -> BotAction {
        BotAction::Dahai {
            pai: pai.into(),
            tsumogiri: false,
        }
    }

    fn played(pai: &str) -> MjaiEvent {
        MjaiEvent::Dahai {
            actor: 0,
            pai: pai.into(),
            tsumogiri: false,
        }
    }

    #[test]
    fn the_view_is_the_share_of_people_who_make_the_move() {
        let ranked = [(dahai("9m"), 0.6), (dahai("1s"), 0.3), (dahai("E"), 0.05)];
        let obvious = view_of(&ranked, &played("9m")).unwrap();
        assert!((obvious.top - 0.6).abs() < 1e-6 && (obvious.chosen - 0.6).abs() < 1e-6);
        assert!((obvious.second.unwrap() - 0.3).abs() < 1e-6);
        let second = view_of(&ranked, &played("1s")).unwrap();
        assert!((second.chosen - 0.3).abs() < 1e-6);
    }

    #[test]
    fn a_move_people_rarely_make_gets_what_the_list_leaves_at_most() {
        let ranked = [(dahai("9m"), 0.6), (dahai("1s"), 0.3), (dahai("E"), 0.05)];
        // 5% is left unlisted and the last listed has 5%: at most 5%, taken as half.
        let rare = view_of(&ranked, &played("5p")).unwrap();
        assert!((rare.chosen - 0.025).abs() < 1e-6, "{rare:?}");
        // Nothing left over: people do not make this move.
        let full = [(dahai("9m"), 0.7), (dahai("1s"), 0.3)];
        assert!(view_of(&full, &played("5p")).unwrap().chosen.abs() < 1e-6);
    }

    #[test]
    fn calls_are_counted_by_what_they_are() {
        let window = [
            (BotAction::Pass, 0.7),
            (
                BotAction::Pon {
                    target: 1,
                    pai: "5p".into(),
                    consumed: vec!["5p".into(), "5pr".into()],
                },
                0.3,
            ),
        ];
        let pass = view_of(&window, &MjaiEvent::None).unwrap();
        assert!((pass.chosen - 0.7).abs() < 1e-6);
    }

    /// The advisor is asked when a reply arrives, not when its events do: a
    /// view of a different kind of decision is a view of something else.
    #[test]
    fn a_view_of_another_kind_of_decision_is_no_view() {
        let window = [(BotAction::Pass, 0.7), (BotAction::Kita, 0.3)];
        assert!(view_of(&window, &played("9m")).is_none());
        let turn = [(dahai("9m"), 0.6), (dahai("1s"), 0.4)];
        assert!(view_of(&turn, &MjaiEvent::None).is_none());
        assert!(view_of(&[], &played("9m")).is_none());
    }

    fn start_game() -> MjaiEvent {
        start_game_of(4, Some(0))
    }

    fn start_game_of(num_players: u8, id: Option<u8>) -> MjaiEvent {
        MjaiEvent::StartGame {
            names: (0..num_players).map(|i| format!("p{i}")).collect(),
            kyoku_first: None,
            aka_flag: None,
            id,
            num_players,
            game_meta: None,
        }
    }

    fn start_kyoku() -> MjaiEvent {
        start_kyoku_of(
            4,
            [
                "1m", "1m", "3m", "5m", "7m", "9m", "2p", "4p", "6p", "8p", "1s", "3s", "5s",
            ],
        )
    }

    /// Our hand (seat 0, dealer) and hidden ones for the other seats.
    fn start_kyoku_of(num_players: u8, ours: [&str; 13]) -> MjaiEvent {
        let hand: Vec<String> = ours.iter().map(|s| s.to_string()).collect();
        let hidden = vec!["?".to_string(); 13];
        let mut tehais = vec![hand];
        tehais.extend((1..num_players).map(|_| hidden.clone()));
        let start = if num_players == 3 { 35000 } else { 25000 };
        MjaiEvent::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "2m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            scores: vec![start; num_players as usize],
            tehais,
            num_players,
        }
    }

    /// The whole way through the real engine: it follows a game from its
    /// events and has a view of a discard made on our first draw.
    #[test]
    fn it_follows_a_game_and_has_a_view_of_our_discard() {
        let mut advisor = Advisor::default();
        // Not following anything yet.
        assert!(advisor.view(&played("9m")).is_none());
        for ev in [
            start_game(),
            start_kyoku(),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "E".into(),
            },
        ] {
            advisor.note(&ev);
        }
        let started = std::time::Instant::now();
        let view = advisor.view(&played("E")).expect("a view of our discard");
        eprintln!("second opinion took {:?}: {view:?}", started.elapsed());
        assert!(view.top > 0.0);
        assert!(view.top <= 1.0);
        assert!((0.0..=view.top + 1e-6).contains(&view.chosen));
        // The game ends; so does the following.
        advisor.note(&serde_json::from_str(r#"{"type":"end_game"}"#).unwrap());
        assert!(advisor.view(&played("E")).is_none());
    }

    /// Sanma is followed with the 3p model, not turned off: the built-in bot
    /// plays three-player games as well.
    #[test]
    fn it_follows_a_three_player_game() {
        let mut advisor = Advisor::default();
        for ev in [
            start_game_of(3, Some(0)),
            start_kyoku_of(
                3,
                [
                    "1m", "9m", "1p", "2p", "3p", "5p", "7p", "9p", "1s", "3s", "5s", "7s", "9s",
                ],
            ),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "E".into(),
            },
        ] {
            advisor.note(&ev);
        }
        let view = advisor
            .view(&played("E"))
            .expect("a view of our sanma discard");
        assert!(view.top > 0.0 && view.top <= 1.0, "{view:?}");
    }

    /// A game that does not name our seat is only watched: nothing to
    /// follow, and whatever was followed before is dropped.
    #[test]
    fn an_observed_game_is_not_followed() {
        let mut advisor = Advisor::default();
        advisor.note(&start_game());
        advisor.note(&start_game_of(4, None));
        for ev in [
            start_kyoku(),
            MjaiEvent::Tsumo {
                actor: 0,
                pai: "E".into(),
            },
        ] {
            advisor.note(&ev);
        }
        assert!(advisor.view(&played("E")).is_none());
        assert!(advisor.pending.is_empty(), "nothing is kept for no engine");
    }
}
