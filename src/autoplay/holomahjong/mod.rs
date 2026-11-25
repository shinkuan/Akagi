//! HoloMahjong autoplay: think for a human-like interval (the shared delay
//! model), then send the client's own hub call for the decision
//! (`bridge::holomahjong::build`), which the proxy splices onto the battle
//! connection's hub stream. Nothing is clicked: the client is a native Unity
//! app, and it already follows actions it did not send itself — the server
//! plays for a seat whose time runs out, and the client renders those
//! discards like any other.
//!
//! The planner answers only the decision the server is waiting on, the open
//! [`link::Window`], and only with what that window allows: a call goes out
//! as one of the meld strings the server offered, never one composed here.

pub mod link;

use crate::autoplay::platform::ActionContext;
use crate::autoplay::wire::{decision_kind, is_our_decision, pre_delay};
use crate::bridge::holomahjong::build;
use crate::schema::MjaiEvent;
use link::{Table, WindowKind};
use std::time::Instant;

/// Never closer than this to the server's deadline.
const DEADLINE_MARGIN_MS: u32 = 1_500;

/// One decision, ready to send.
#[derive(Debug, Clone, PartialEq)]
pub struct HoloPlan {
    /// The window it answers (its identity).
    pub window: Instant,
    /// Think time to show, from the window's opening, ms.
    pub think_ms: u32,
    /// The latest it may go out, from the window's opening, ms.
    pub limit_ms: u32,
    /// Whether the window is our turn (the server answers a turn at once).
    pub turn: bool,
    /// The hub message.
    pub message: Vec<u8>,
}

/// `Ok(None)` when the reply is not ours to send or needs nothing sent;
/// `Err` when it is ours but the open window cannot take it.
pub fn plan(ctx: &ActionContext, table: &Table) -> Result<Option<HoloPlan>, String> {
    if !is_our_decision(ctx.action, ctx.our_seat) {
        return Ok(None);
    }
    let Some(window) = &table.window else {
        // The bot answers every hand's closing events with `none`.
        return match ctx.action {
            MjaiEvent::None => Ok(None),
            _ => Err("no decision is open".into()),
        };
    };
    if window.answered {
        return Ok(None);
    }
    let seat = table.seat.ok_or("our seat is unknown")?;
    let hand = ctx
        .snapshot
        .players
        .get(ctx.our_seat as usize)
        .map(|p| p.tehai.as_slice())
        .unwrap_or_default();
    let message = build::encode(ctx.action, window, seat, hand)?;
    Ok(Some(HoloPlan {
        window: window.opened_at,
        think_ms: pre_delay(ctx, decision_kind(ctx.action)),
        limit_ms: window.total_ms.saturating_sub(DEADLINE_MARGIN_MS),
        turn: matches!(window.kind, WindowKind::Turn { .. }),
        message,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DelayModelConfig, MajsoulAutoplayConfig};
    use crate::game_state::snapshot::{GameStateSnapshot, Phase, PlayerSnapshot};
    use link::Window;

    struct Fixture {
        cfg: MajsoulAutoplayConfig,
        snapshot: GameStateSnapshot,
    }

    impl Fixture {
        fn new(hand: &[&str]) -> Self {
            let player = |seat: u8, tehai: Vec<String>| PlayerSnapshot {
                seat,
                tehai,
                melds: Vec::new(),
                river: Vec::new(),
                score: 25000,
                riichi_declared: false,
                riichi_stage: false,
                double_riichi: false,
                riichi_declaration_index: None,
                kita_tiles: Vec::new(),
                drawn_tile: None,
            };
            Self {
                cfg: MajsoulAutoplayConfig::default(),
                snapshot: GameStateSnapshot {
                    bakaze: "E".into(),
                    kyoku: 1,
                    honba: 0,
                    kyotaku: 0,
                    oya: 0,
                    current_player: 1,
                    turn_count: 0,
                    phase: Phase::WaitAct,
                    is_done: false,
                    num_players: 4,
                    players: (0..4)
                        .map(|s| {
                            player(
                                s,
                                if s == 1 {
                                    hand.iter().map(|t| t.to_string()).collect()
                                } else {
                                    Vec::new()
                                },
                            )
                        })
                        .collect(),
                    dora_markers: Vec::new(),
                    our_seat: Some(1),
                },
            }
        }

        fn ctx<'a>(&'a self, action: &'a MjaiEvent) -> ActionContext<'a> {
            ActionContext {
                action,
                snapshot: &self.snapshot,
                legal_actions: &[],
                our_seat: 1,
                last_kawa_tile: None,
                last_self_tsumo: None,
                self_riichi_accepted: false,
                num_players: 4,
                cfg: &self.cfg,
                delay_cfg: DelayModelConfig::default(),
                budget: None,
                probs: None,
                reading: Default::default(),
                delay_script: None,
                tenhou: None,
            }
        }
    }

    fn table(kind: WindowKind) -> Table {
        Table {
            seat: Some(3),
            window: Some(Window {
                opened_at: Instant::now(),
                kind,
                chi: Vec::new(),
                pon: Vec::new(),
                kan: Vec::new(),
                kita: Vec::new(),
                can_win: false,
                base_ms: 20000,
                bank_ms: 30000,
                total_ms: 50000,
                answered: false,
            }),
        }
    }

    fn turn(drawn: &str) -> Table {
        table(WindowKind::Turn {
            drawn: Some(drawn.into()),
        })
    }

    #[test]
    fn our_discard_is_planned_for_the_open_turn_with_our_wire_seat() {
        let f = Fixture::new(&["1m", "9p"]);
        let ev = MjaiEvent::Dahai {
            actor: 1,
            pai: "9p".into(),
            tsumogiri: true,
        };
        let t = turn("9p");
        let plan = plan(&f.ctx(&ev), &t).unwrap().expect("a plan");
        let msg = crate::bridge::holomahjong::hub::decode(&plan.message).unwrap();
        assert_eq!(
            msg.body,
            serde_json::json!([3, "p9_", 0]),
            "wire seat 3, not actor 1"
        );
        assert_eq!(plan.window, t.window.as_ref().unwrap().opened_at);
        assert_eq!(plan.limit_ms, 50000 - DEADLINE_MARGIN_MS);
        assert!(plan.turn && plan.think_ms > 0);
    }

    #[test]
    fn other_seats_and_answered_windows_send_nothing() {
        let f = Fixture::new(&["1m"]);
        let theirs = MjaiEvent::Dahai {
            actor: 2,
            pai: "1m".into(),
            tsumogiri: false,
        };
        assert_eq!(plan(&f.ctx(&theirs), &turn("1m")), Ok(None));

        let ours = MjaiEvent::Dahai {
            actor: 1,
            pai: "1m".into(),
            tsumogiri: true,
        };
        let mut t = turn("1m");
        t.window.as_mut().unwrap().answered = true;
        assert_eq!(
            plan(&f.ctx(&ours), &t),
            Ok(None),
            "the client answered first"
        );
    }

    #[test]
    fn a_decline_with_nothing_open_is_quiet_but_a_move_is_not() {
        let f = Fixture::new(&["1m"]);
        let closed = Table {
            seat: Some(3),
            window: None,
        };
        assert_eq!(plan(&f.ctx(&MjaiEvent::None), &closed), Ok(None));
        let ours = MjaiEvent::Dahai {
            actor: 1,
            pai: "1m".into(),
            tsumogiri: true,
        };
        assert!(plan(&f.ctx(&ours), &closed).is_err());
    }

    #[test]
    fn a_pass_answers_a_claim() {
        let f = Fixture::new(&[]);
        let plan = plan(&f.ctx(&MjaiEvent::None), &table(WindowKind::Claim))
            .unwrap()
            .expect("a skip");
        assert!(!plan.turn);
        let msg = crate::bridge::holomahjong::hub::decode(&plan.message).unwrap();
        assert_eq!(
            msg.method,
            crate::bridge::holomahjong::hub::SELECT_ACTION_SKIP
        );
    }
}
