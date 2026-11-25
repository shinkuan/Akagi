//! Shared by the planners that send the client's own protocol calls instead
//! of clicking (Riichi City, HoloMahjong): which decisions are ours to send,
//! and how long to think before sending one.

use crate::autoplay::delay::{self, DecisionKind, DelayInput};
use crate::autoplay::platform::ActionContext;
use crate::config::DelayMode;
use crate::schema::MjaiEvent;

/// Whether the event is a decision by `our_seat` — the only decisions we
/// can transmit. Other seats' actions pass through the bus too and would
/// otherwise be echoed back to the server as if we had made them.
pub fn is_our_decision(ev: &MjaiEvent, our_seat: u8) -> bool {
    let actor = match ev {
        MjaiEvent::Dahai { actor, .. }
        | MjaiEvent::Reach { actor, .. }
        | MjaiEvent::Chi { actor, .. }
        | MjaiEvent::Pon { actor, .. }
        | MjaiEvent::Daiminkan { actor, .. }
        | MjaiEvent::Ankan { actor, .. }
        | MjaiEvent::Kakan { actor, .. }
        | MjaiEvent::Hora { actor, .. }
        | MjaiEvent::Kita { actor, .. } => Some(*actor),
        // `None` is the bot declining *our* call window — ours to send.
        // `Ryukyoku` (kyuushu, `deltas: None`) is the bot declaring on
        // *our* first-turn draw — no actor field, inherently ours.
        MjaiEvent::None | MjaiEvent::Ryukyoku { deltas: None } => None,
        _ => return false,
    };
    actor.is_none_or(|a| a == our_seat)
}

pub fn decision_kind(ev: &MjaiEvent) -> DecisionKind {
    match ev {
        MjaiEvent::Dahai { .. } => DecisionKind::Dahai,
        MjaiEvent::Reach { .. } => DecisionKind::Reach,
        MjaiEvent::Chi { .. } => DecisionKind::Chi,
        MjaiEvent::Pon { .. } => DecisionKind::Pon,
        MjaiEvent::Daiminkan { .. } => DecisionKind::Daiminkan,
        MjaiEvent::Ankan { .. } => DecisionKind::Ankan,
        MjaiEvent::Kakan { .. } => DecisionKind::Kakan,
        MjaiEvent::Hora { .. } => DecisionKind::Hora,
        MjaiEvent::Kita { .. } => DecisionKind::Kita,
        MjaiEvent::Ryukyoku { .. } => DecisionKind::Ryukyoku,
        _ => DecisionKind::Pass,
    }
}

/// Same delay model as the click platforms, minus the click overhead: a
/// frame send costs nothing after the sleep, so the whole target lands in
/// the wait. Contextual flags the frame path cannot observe (tile class,
/// junme, opponent riichi) are left at their neutral values — the
/// distribution shape and caps still apply. The first discard of a kyoku
/// does carry the fresh-hand survey time (`first_action_of_kyoku`), the
/// pacing half of Majsoul's dealer-opening knob — the correctness half
/// (clicks dropped during the sort animation) doesn't apply to frames,
/// where the window-state wait already gates the send. The reading (table
/// facts, second opinion) is the manager's, the same as on the other
/// platforms.
pub fn pre_delay(ctx: &ActionContext, kind: DecisionKind) -> u32 {
    let is_tsumogiri = matches!(
        ctx.action,
        MjaiEvent::Dahai {
            tsumogiri: true,
            ..
        }
    );
    let first_of_kyoku = matches!(ctx.action, MjaiEvent::Dahai { .. })
        && ctx
            .snapshot
            .players
            .get(ctx.our_seat as usize)
            .is_some_and(|p| p.river.is_empty());
    let input = DelayInput {
        kind,
        is_tsumogiri,
        is_post_call: false,
        first_action_of_kyoku: first_of_kyoku,
        opening_animation: false,
        can_riichi: matches!(ctx.action, MjaiEvent::Reach { .. }),
        in_riichi: ctx.self_riichi_accepted,
        opponent_riichi: false,
        tile_class: None,
        junme: 0,
        legal_action_count: ctx.legal_actions.len(),
        probs: ctx.probs,
        reading: ctx.reading,
        budget: ctx.budget,
        click_overhead_ms: 0,
        cfg: ctx.cfg,
        delay_cfg: &ctx.delay_cfg,
    };
    let legacy = ctx.delay_cfg.mode == DelayMode::Legacy;
    let decision = ctx
        .delay_script
        .filter(|_| !legacy)
        .and_then(|s| s.try_decide(&input))
        .unwrap_or_else(|| delay::decide(&input, &mut rand::rng()));
    // A script's answer is final (it owns the floor); the built-in model's
    // is held to the functional floor.
    if decision.from_script {
        decision.total_target_ms
    } else {
        decision
            .total_target_ms
            .max(delay::functional_floor(&input))
    }
}
