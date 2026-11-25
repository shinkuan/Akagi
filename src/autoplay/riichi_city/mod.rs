//! Riichi City autoplay planner: think for a human-like interval (the
//! shared delay model), then one `SendFrame` carrying the encoded action
//! (`bridge::riichi_city::build`). No page exists to click — the Windows
//! client is reached through its protocol.

use crate::autoplay::platform::{ActionContext, PlanResult, PlatformAutoplay, Step};
use crate::autoplay::wire::{decision_kind, is_our_decision, pre_delay};
use crate::bridge::riichi_city::build;

pub mod round_advance;

#[derive(Default)]
pub struct RiichiCityAutoplay;

impl RiichiCityAutoplay {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformAutoplay for RiichiCityAutoplay {
    fn plan(&self, ctx: &ActionContext) -> PlanResult {
        // Only our own decisions have an uplink frame; everything else
        // (other seats, transitions) is `None` from `encode_action`.
        if !is_our_decision(ctx.action, ctx.our_seat) {
            return PlanResult::default();
        }
        // A win names its tile: the drawn tile for a tsumo, the claimed
        // discard for a ron. Discards and calls carry rack positions, so
        // pass our tehai and the held draw through (see
        // `build::encode_action_with`). The snapshot's `drawn_tile` — not
        // `last_self_tsumo`, which stays stale through a post-call
        // discard — says whether a draw is actually in the rack now.
        let player = ctx.snapshot.players.get(ctx.our_seat as usize);
        let hand = player.map(|p| p.tehai.clone());
        let drawn = player.and_then(|p| p.drawn_tile.clone());
        let Some(frame) = build::encode_action_with(
            ctx.action,
            drawn.as_deref(),
            ctx.last_kawa_tile,
            hand.as_deref(),
        ) else {
            return PlanResult::default();
        };
        let kind = decision_kind(ctx.action);
        let steps = vec![
            Step::Sleep {
                duration_ms: pre_delay(ctx, kind),
            },
            Step::SendFrame(frame),
        ];
        PlanResult { steps }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::platform::ActionContext;
    use crate::bridge::riichi_city::packet::WPacket;
    use crate::config::{DelayModelConfig, MajsoulAutoplayConfig};
    use crate::game_state::snapshot::{GameStateSnapshot, Phase};
    use crate::schema::MjaiEvent;

    /// Owns the values an `ActionContext` borrows, so tests can build one
    /// without fighting lifetimes.
    struct CtxFixture {
        cfg: MajsoulAutoplayConfig,
        delay_cfg: DelayModelConfig,
        snapshot: GameStateSnapshot,
    }

    impl CtxFixture {
        fn new() -> Self {
            Self {
                cfg: MajsoulAutoplayConfig::default(),
                delay_cfg: DelayModelConfig::default(),
                snapshot: GameStateSnapshot {
                    bakaze: "E".into(),
                    kyoku: 1,
                    honba: 0,
                    kyotaku: 0,
                    oya: 0,
                    current_player: 0,
                    turn_count: 0,
                    phase: Phase::WaitAct,
                    is_done: false,
                    num_players: 4,
                    players: Vec::new(),
                    dora_markers: Vec::new(),
                    our_seat: Some(0),
                },
            }
        }

        fn ctx<'a>(&'a self, action: &'a MjaiEvent) -> ActionContext<'a> {
            ActionContext {
                action,
                snapshot: &self.snapshot,
                legal_actions: &[],
                our_seat: 0,
                last_kawa_tile: None,
                last_self_tsumo: None,
                self_riichi_accepted: false,
                num_players: 4,
                cfg: &self.cfg,
                delay_cfg: self.delay_cfg.clone(),
                budget: None,
                probs: None,
                reading: Default::default(),
                delay_script: None,
                tenhou: None,
            }
        }
    }

    #[test]
    fn discard_plan_is_delay_then_frame() {
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: "5mr".into(),
            tsumogiri: false,
        };
        let f = CtxFixture::new();
        let plan = RiichiCityAutoplay::new().plan(&f.ctx(&ev));
        assert_eq!(plan.steps.len(), 2);
        assert!(matches!(plan.steps[0], Step::Sleep { .. }));
        let Step::SendFrame(frame) = &plan.steps[1] else {
            panic!("expected a frame step");
        };
        let pkts = WPacket::parse_frame(frame);
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].body["cmd"], "req_game_action");
    }

    #[test]
    fn other_seats_actions_produce_no_plan() {
        let ev = MjaiEvent::Dahai {
            actor: 2,
            pai: "1p".into(),
            tsumogiri: true,
        };
        let f = CtxFixture::new();
        assert!(RiichiCityAutoplay::new().plan(&f.ctx(&ev)).steps.is_empty());
    }

    #[test]
    fn delay_is_never_zero_ms() {
        // The functional floor applies even with the fastest config.
        let ev = MjaiEvent::Dahai {
            actor: 0,
            pai: "1p".into(),
            tsumogiri: true,
        };
        let f = CtxFixture::new();
        let plan = RiichiCityAutoplay::new().plan(&f.ctx(&ev));
        let Step::Sleep { duration_ms } = plan.steps[0] else {
            panic!("expected sleep");
        };
        assert!(duration_ms > 0);
    }

    /// A kyuushu declaration is ours to send (no actor field — it can
    /// only come from the bot's own decision) and plans like any button
    /// decision: think, then frame. Regression for the live timeout where
    /// the declaration fell through `is_our_decision` and the server's
    /// countdown had to auto-discard for us.
    #[test]
    fn kyuushu_declaration_plans_a_frame() {
        let ev = MjaiEvent::Ryukyoku { deltas: None };
        let f = CtxFixture::new();
        let plan = RiichiCityAutoplay::new().plan(&f.ctx(&ev));
        assert_eq!(plan.steps.len(), 2, "sleep then frame");
        let Step::SendFrame(frame) = &plan.steps[1] else {
            panic!("expected a frame step");
        };
        let pkts = WPacket::parse_frame(frame);
        assert_eq!(pkts[0].body["data"]["action"], 12);
    }

    /// An observed exhaustive draw (deltas present) is not a decision —
    /// it must not plan, let alone send.
    #[test]
    fn observed_draw_does_not_plan() {
        let ev = MjaiEvent::Ryukyoku {
            deltas: Some(vec![1500, -500, -500, -500]),
        };
        let f = CtxFixture::new();
        assert!(RiichiCityAutoplay::new().plan(&f.ctx(&ev)).steps.is_empty());
    }
}
