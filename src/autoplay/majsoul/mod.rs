//! Majsoul implementation of [`PlatformAutoplay`].
//!
//! Coordinate tables in `coords.rs` are the only Majsoul-specific data;
//! the dispatch logic here translates a bot decision into a [`Step`]
//! sequence using:
//!
//! - The current `legal_actions` from the riichi engine (which buttons
//!   are visible, plus chi/pon/kan candidate enumeration).
//! - The current hand from `GameStateSnapshot` (sort-aware tile lookup).
//! - The dispatch flow from the original Akagi Python autoplay
//!   (`autoplay_majsoul.py`, the main action handler).

pub mod coords;

use crate::autoplay::delay::{self, DecisionKind, DelayInput};
use crate::autoplay::platform::{ActionContext, PlanResult, PlatformAutoplay, Step};
use crate::bridge::majsoul::tile::compare_pai;
use crate::schema::MjaiEvent;
#[cfg(test)]
use coords::TSUMO_SPACE;
use coords::{
    action_button_pos, candidate_pos, get_pai_coord, kan_candidate_pos, MajsoulOpType,
    ACTION_PRIORITY, TILES,
};
use riichienv_core::action::{Action, ActionType};
use riichienv_core::parser::tid_to_mjai;

#[derive(Default)]
pub struct MajsoulAutoplay;

impl MajsoulAutoplay {
    pub fn new() -> Self {
        Self
    }
}

impl PlatformAutoplay for MajsoulAutoplay {
    fn plan(&self, ctx: &ActionContext) -> PlanResult {
        let mut result = PlanResult::default();

        match ctx.action {
            // ----- Dahai (打牌) ---------------------------------------------
            MjaiEvent::Dahai { actor, pai, .. } if *actor == ctx.our_seat => {
                // While in riichi, Majsoul auto-discards, so ours would be a
                // second one. (The riichi-declaring tile goes out inside the
                // Reach plan below, before acceptance, so it is unaffected.)
                if ctx.self_riichi_accepted {
                    // …with one exception: the auto-discard is held back
                    // while an own-draw operation prompt is open — kita on
                    // a drawn North (sanma), a riichi-legal ankan, or a
                    // tsumo agari. Majsoul waits for an answer, so a bot
                    // decision to tsumogiri must decline the prompt via
                    // the X button; the client then discards the draw on
                    // its own. Returning with no click here left the game
                    // hanging until the turn timer and the entire time
                    // bank drained (the server eventually auto-declines).
                    if riichi_prompt_pending(ctx, pai) {
                        push_pre_delay(&mut result.steps, ctx, DecisionKind::Pass, 0);
                        if let Some(button) = action_button_for(MajsoulOpType::None, ctx) {
                            result.steps.push(Step::Click {
                                x_norm: button.0,
                                y_norm: button.1,
                            });
                        }
                    }
                    return result;
                }
                // The dealer-opening hand-sort animation wait (clicks
                // issued during it are dropped) is folded into the delay
                // model as the `opening_animation` functional floor.
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Dahai, 0);
                if let Some(click) = plan_dahai_click(pai, ctx) {
                    result.steps.push(click);
                }
            }

            // ----- Reach (立直) — declaration + tile in one plan ----------
            MjaiEvent::Reach { actor, pai } if *actor == ctx.our_seat => {
                // Majsoul fuses declaring + discarding into one action, so the
                // tile must be known up front. It normally is — the bot fills
                // `Reach.pai` (natively or via the manager's autoplay reach
                // follow-up, #257, which logs when it cannot). A bare reach
                // here means that resolution failed; declare nothing rather
                // than press the button and leave the client sitting on an
                // owed discard until timeout.
                let Some(tile) = pai else {
                    return PlanResult::default();
                };
                // Clicks the riichi tile right after the button, so reserve
                // one extra click of overhead.
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Reach, 1);
                if let Some(button) = action_button_for(MajsoulOpType::Reach, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                } else {
                    // Reach not in legal_actions — bridge desync; bail.
                    return PlanResult::default();
                }
                result.steps.push(Step::Sleep {
                    duration_ms: ctx.cfg.inter_click_delay_ms.ms(),
                });
                if let Some(click) = plan_dahai_click(tile, ctx) {
                    result.steps.push(click);
                }
            }

            // ----- Chi / Pon / Daiminkan / Ankan / Kakan -------------------
            // (action button + optional candidate disambiguation)
            MjaiEvent::Chi { actor, .. } if *actor == ctx.our_seat => {
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Chi, 1);
                plan_meld(MajsoulOpType::Chi, ActionType::Chi, &mut result, ctx);
            }
            MjaiEvent::Pon { actor, .. } if *actor == ctx.our_seat => {
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Pon, 1);
                plan_meld(MajsoulOpType::Pon, ActionType::Pon, &mut result, ctx);
            }
            MjaiEvent::Daiminkan { actor, .. } if *actor == ctx.our_seat => {
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Daiminkan, 1);
                plan_meld(
                    MajsoulOpType::Daiminkan,
                    ActionType::Daiminkan,
                    &mut result,
                    ctx,
                );
            }
            MjaiEvent::Ankan { actor, .. } if *actor == ctx.our_seat => {
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Ankan, 1);
                plan_kan(MajsoulOpType::Ankan, ActionType::Ankan, &mut result, ctx);
            }
            MjaiEvent::Kakan { actor, .. } if *actor == ctx.our_seat => {
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Kakan, 1);
                plan_kan(MajsoulOpType::Kakan, ActionType::Kakan, &mut result, ctx);
            }

            // ----- Hora — zimo button on own draw, ron on opponent ---------
            MjaiEvent::Hora { actor, .. } if *actor == ctx.our_seat => {
                let op = if hora_is_tsumo(ctx) {
                    MajsoulOpType::Zimo
                } else {
                    MajsoulOpType::Ron
                };
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Hora, 0);
                if let Some(button) = action_button_for(op, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                }
            }

            // ----- Ryukyoku (九種九牌) -------------------------------------
            MjaiEvent::Ryukyoku { .. } => {
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Ryukyoku, 0);
                if let Some(button) = action_button_for(MajsoulOpType::Ryukyoku, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                }
            }

            // ----- Kita (3p 北抜き) ----------------------------------------
            MjaiEvent::Kita { actor, .. } if *actor == ctx.our_seat => {
                // On the opening draw of a kyoku, Majsoul plays a tile-
                // dealing animation; clicks issued during it land on the
                // wrong target. Folded into the delay model as
                // `opening_animation` (same wait as dealer first discard).
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Kita, 0);
                if let Some(button) = action_button_for(MajsoulOpType::Nukidora, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                }
            }

            // ----- None — pass / cancel button -----------------------------
            //
            // The bot emits `None` on every mjai event it has nothing to say
            // about — including pure echoes of other players' tsumo/dahai
            // notifies, where Majsoul is showing no buttons at all. Without
            // a gate we'd loop-click the lobby/preview UI's rightmost button
            // on every other-player turn.
            //
            // riichienv only adds `ActionType::Pass` to legal_actions in
            // `Phase::WaitResponse` (`riichienv-core/src/state/legal_actions.rs:249`)
            // — i.e. exactly when Majsoul is showing the Pass button after a
            // claimable discard. Use that as the visibility gate.
            MjaiEvent::None => {
                if !pass_button_visible(ctx) {
                    return result;
                }
                // Extra guard: never click Pass during WaitAct (own draw turn).
                // Stale legal_actions from the previous round can falsely expose
                // Pass before the tracker has processed start_kyoku.
                if ctx.snapshot.phase != crate::game_state::snapshot::Phase::WaitResponse {
                    return result;
                }
                // Extra guard: only click Skip when there's an actual claim option
                // (pon/ron/daiminkan). In 3p there's no chi, so many WaitResponse
                // windows have no claimable actions — Majsoul shows no buttons at all
                // and a ghost click would land on the wrong UI element.
                let has_claim = ctx.legal_actions.iter().any(|a| {
                    matches!(
                        a.action_type,
                        riichienv_core::action::ActionType::Pon
                            | riichienv_core::action::ActionType::Daiminkan
                            | riichienv_core::action::ActionType::Ron
                            | riichienv_core::action::ActionType::Chi
                    )
                });
                if !has_claim {
                    return result;
                }
                push_pre_delay(&mut result.steps, ctx, DecisionKind::Pass, 0);
                if let Some(button) = action_button_for(MajsoulOpType::None, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                }
            }

            // Everything else (StartGame, Tsumo, Dora, ReachAccepted,
            // EndKyoku, EndGame, events from other seats) doesn't drive
            // a click.
            _ => {}
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compute the pre-click "thinking" delay via the delay model
/// (`autoplay::delay`) and push it as the leading `Sleep` step.
///
/// The model returns a target **total** thinking time for the decision
/// window; when the server budget is known, the time already consumed
/// (network, proxy, bot inference) is subtracted so the server-observed
/// interval matches the target rather than exceeding it.
///
/// `extra_clicks` is how many clicks follow the *first* one in this plan
/// (candidate disambiguation, riichi tile). It sizes the click-sequence
/// overhead the budget layer must reserve.
fn push_pre_delay(
    steps: &mut Vec<Step>,
    ctx: &ActionContext,
    kind: DecisionKind,
    extra_clicks: u32,
) {
    let cfg = ctx.cfg;
    let per_click = cfg.hover_delay_ms.ms() + cfg.click_hold_ms.ms();
    let click_overhead_ms = per_click + extra_clicks * (per_click + cfg.inter_click_delay_ms.ms());

    // Nothing has happened on the table yet this kyoku: no discard, and no
    // kita of ours (a dealer can set aside several North tiles before the
    // first discard; only the first of them follows the deal).
    let untouched = ctx.last_kawa_tile.is_none()
        && ctx
            .snapshot
            .players
            .get(ctx.our_seat as usize)
            .is_none_or(|p| p.kita_tiles.is_empty());
    let opening_animation = match kind {
        DecisionKind::Dahai => is_dealer_first_discard(ctx),
        // A kita on the opening draw of a kyoku waits out the same
        // dealing animation as the dealer's first discard.
        DecisionKind::Kita => untouched,
        _ => false,
    };

    // Discard sub-kind: tsumogiri comes straight off the event; a
    // post-call discard is the only dahai decision with no just-drawn
    // tile — the meld consumed the turn's draw. (Hand size cannot tell
    // the cases apart: with the drawn tile in `tehai`, both post-draw
    // and post-call hands are ≡ 2 mod 3.) `drawn_tile` is tracked for
    // the active seat, which we are when discarding; requiring a meld
    // guards states where no draw is tracked for other reasons, e.g.
    // the dealer's opening hand.
    let (is_tsumogiri, is_post_call, tile_class) = match ctx.action {
        MjaiEvent::Dahai { tsumogiri, pai, .. } => {
            let me = ctx.snapshot.players.get(ctx.our_seat as usize);
            (
                *tsumogiri,
                me.is_some_and(|p| !p.melds.is_empty() && p.drawn_tile.is_none()),
                crate::autoplay::delay::TileClass::of_mjai(pai),
            )
        }
        _ => (false, false, None),
    };
    // An opponent riichi turns every decision into a defence read.
    let opponent_riichi = ctx
        .snapshot
        .players
        .iter()
        .any(|p| p.seat != ctx.our_seat && p.riichi_declared);

    // Legacy mode reproduces the historical fixed model: uniform draw,
    // no script consulted. The manager already withholds the script in
    // legacy mode; forcing the distribution here completes the split.
    let legacy = ctx.delay_cfg.mode == crate::config::DelayMode::Legacy;
    let mut delay_cfg = ctx.delay_cfg.clone();
    if legacy {
        delay_cfg.distribution = crate::config::DelayDistribution::Uniform;
    }

    let input = DelayInput {
        kind,
        is_tsumogiri,
        is_post_call,
        first_action_of_kyoku: untouched,
        opening_animation,
        can_riichi: ctx
            .legal_actions
            .iter()
            .any(|a| a.action_type == ActionType::Riichi),
        in_riichi: ctx.self_riichi_accepted,
        opponent_riichi,
        tile_class,
        junme: ctx
            .snapshot
            .players
            .get(ctx.our_seat as usize)
            .map(|p| p.river.len() as u32 + 1)
            .unwrap_or(0),
        legal_action_count: ctx.legal_actions.len(),
        probs: ctx.probs,
        reading: ctx.reading,
        budget: ctx.budget,
        click_overhead_ms,
        cfg,
        delay_cfg: &delay_cfg,
    };
    // User Lua policy first (falls back internally on any failure), then
    // the built-in model. The built-in model's answer is held to the caps
    // and floors here; a script's already holds its own.
    let decision = ctx
        .delay_script
        .filter(|_| !legacy)
        .and_then(|s| s.try_decide(&input))
        .unwrap_or_else(|| delay::decide(&input, &mut rand::rng()));

    // Convert target total time to a sleep: subtract what the window has
    // already consumed AND what the click sequence itself will take —
    // the target is the server-observed total, and hover/hold/candidate
    // clicks all land after this sleep. Without a budget there is no
    // window clock — only the click overhead is deducted. A riichi's
    // declaration and tile go out in one plan, so the whole action is
    // budgeted here as a single interval (the calibration source measures
    // declaration plus tile as one server-observed interval).
    let sleep = match ctx.budget {
        Some(b) => decision
            .total_target_ms
            .saturating_sub(b.elapsed_ms)
            .saturating_sub(click_overhead_ms),
        None => decision.total_target_ms.saturating_sub(click_overhead_ms),
    };
    // The overhead deduction must not undercut UI readiness: the first
    // click still has to land after the functional floor. Time already
    // elapsed in the window counts toward that floor. A script's answer
    // already accounts for that (it owns the floor and sees `click_ms`).
    let elapsed = ctx.budget.map_or(0, |b| b.elapsed_ms);
    let sleep = if decision.from_script {
        sleep
    } else {
        sleep.max(delay::functional_floor(&input).saturating_sub(elapsed))
    };
    steps.push(Step::Sleep { duration_ms: sleep });
}

/// Plan a hand-tile click for a discard or riichi-declaring discard.
fn plan_dahai_click(pai: &str, ctx: &ActionContext) -> Option<Step> {
    let our_seat = ctx.our_seat as usize;
    if our_seat >= ctx.snapshot.players.len() {
        return None;
    }

    // Dealer's first discard: Majsoul lays all 14 tiles continuously on
    // the rack (sorted) — there's no "tsumohai" gap. Click position is
    // the index in the fully-sorted 14-tile array, using TILES[i]
    // directly (not get_pai_coord, which would add TSUMO_SPACE for i=13).
    if is_dealer_first_discard(ctx) {
        let mut sorted = ctx.snapshot.players[our_seat].tehai.clone();
        sorted.sort_by(|a, b| compare_pai(a, b));
        let idx = sorted.iter().position(|x| x == pai)?;
        let (x, y) = TILES.get(idx).copied()?;
        return Some(Step::Click {
            x_norm: x,
            y_norm: y,
        });
    }

    let tehai: Vec<String> = ctx.snapshot.players[our_seat].tehai.clone();
    // Prefer snapshot drawn_tile (always accurate) over last_self_tsumo
    // (None after pon/chi/kita since no Tsumo event fires for those draws).
    // In 3p: if snapshot_drawn is "N" but last_self_tsumo is set to something
    // else, the snapshot predates the Kita; use last_self_tsumo (rinshan).
    let snapshot_drawn = ctx
        .snapshot
        .players
        .get(our_seat)
        .and_then(|p| p.drawn_tile.as_deref());
    let tsumohai = match (snapshot_drawn, ctx.last_self_tsumo) {
        (Some("N"), Some(rinshan)) => Some(rinshan),
        (Some(t), _) => Some(t),
        (None, fallback) => fallback,
    };

    // Hand sizes that include the just-drawn tile: 2/5/8/11/14 (mod 3 = 2).
    let has_tsumohai = matches!(tehai.len(), 14 | 11 | 8 | 5 | 2) && tsumohai.is_some();

    // Sort the full tehai to match Majsoul visual order.
    // riichienv always sorts hand after every draw, so this is accurate.
    let mut sorted_tehai = tehai.clone();
    sorted_tehai.sort_by(|a, b| compare_pai(a, b));

    if has_tsumohai {
        let t = tsumohai.unwrap();
        if pai == t {
            // Discarding the tsumohai: click the far-right tsumohai slot.
            let (x, y) = get_pai_coord(13, tehai.len() - 1);
            return Some(Step::Click {
                x_norm: x,
                y_norm: y,
            });
        }
        // Discarding a closed-hand tile. The tsumohai sits on the far right
        // visually. Exclude the last occurrence of the tsumohai tile from
        // sorted_tehai, then find pai index in the remaining tiles.
        let tsumo_excl = sorted_tehai.iter().rposition(|x| x.as_str() == t);
        let idx = sorted_tehai
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != tsumo_excl)
            .enumerate()
            .find(|(_, (_, x))| x.as_str() == pai)
            .map(|(visual_idx, _)| visual_idx)?;
        if idx >= TILES.len() - 1 {
            return None;
        }
        let (x, y) = get_pai_coord(idx, tehai.len() - 1);
        return Some(Step::Click {
            x_norm: x,
            y_norm: y,
        });
    }

    // No tsumohai: all tiles closed, click by sorted index directly.
    let idx = sorted_tehai.iter().position(|x| x.as_str() == pai)?;
    if idx >= TILES.len() - 1 {
        return None;
    }
    let (x, y) = get_pai_coord(idx, tehai.len());
    Some(Step::Click {
        x_norm: x,
        y_norm: y,
    })
}
/// True for the dealer's very first discard of a kyoku — the moment
/// when Mahjong Soul has dealt 14 tiles, played the hand-sort animation,
/// and is showing all 14 tiles continuously on the rack (no tsumohai
/// offset). Detected by: we're oya, our hand size is 14, and we have
/// no discards or melds yet.
///
/// 3p caveat: a kita (北抜き) declared on the opening hand keeps the
/// hand at 14 (North removed → rinshan drawn) with an empty river and
/// empty `melds` (the North goes to the kita pool, not `melds`). But
/// after a kita the rinshan sits as a *separated* tsumohai on the far
/// right — the layout is no longer the continuous 14-tile deal. So a
/// non-empty kita pool must fall through to the normal tsumohai-offset
/// path, otherwise every closed tile sorting after the rinshan gets
/// clicked one slot too far right.
fn is_dealer_first_discard(ctx: &ActionContext) -> bool {
    let our_seat = ctx.our_seat as usize;
    let Some(player) = ctx.snapshot.players.get(our_seat) else {
        return false;
    };
    ctx.snapshot.oya == ctx.our_seat
        && player.tehai.len() == 14
        && player.river.is_empty()
        && player.melds.is_empty()
        && player.kita_tiles.is_empty()
}

/// Plan a chi/pon/daiminkan: action button click + optional candidate
/// disambiguation when multiple consume-tile combinations are legal.
fn plan_meld(op: MajsoulOpType, at: ActionType, result: &mut PlanResult, ctx: &ActionContext) {
    let Some(button) = action_button_for(op, ctx) else {
        return;
    };
    result.steps.push(Step::Click {
        x_norm: button.0,
        y_norm: button.1,
    });

    let consumed: Vec<String> = match ctx.action {
        MjaiEvent::Chi { consumed, .. } => consumed.to_vec(),
        MjaiEvent::Pon { consumed, .. } => consumed.to_vec(),
        MjaiEvent::Daiminkan { consumed, .. } => consumed.to_vec(),
        _ => return,
    };

    let candidates = collect_candidate_consumes(ctx.legal_actions, at);
    if candidates.len() <= 1 {
        return; // single option → Majsoul auto-confirms
    }
    let mut sorted_consumed = consumed;
    sorted_consumed.sort_by(|a, b| compare_pai(a, b));
    if let Some(idx) = candidates
        .iter()
        .position(|c| same_consumed(c, &sorted_consumed))
    {
        if let Some(p) = candidate_pos(idx, candidates.len()) {
            result.steps.push(Step::Sleep {
                duration_ms: ctx.cfg.inter_click_delay_ms.ms(),
            });
            result.steps.push(Step::Click {
                x_norm: p.0,
                y_norm: p.1,
            });
        }
    }
}

/// Plan an ankan/kakan: kan button click + optional kan-row candidate.
///
/// Special case: when both ankan and kakan are simultaneously legal,
/// Majsoul shows ONE kan button whose candidate row contains the union
/// of both, ordered `[kakan…, ankan…]`. The candidate index for the
/// bot's chosen tile is computed against the unified list.
fn plan_kan(op: MajsoulOpType, at: ActionType, result: &mut PlanResult, ctx: &ActionContext) {
    let Some(button) = action_button_for(op, ctx) else {
        return;
    };
    result.steps.push(Step::Click {
        x_norm: button.0,
        y_norm: button.1,
    });

    // Collect both ankan and kakan candidates. When the bot is doing
    // a kakan, kakan candidates are listed first; for ankan, ankan
    // first. Reference: `autoplay_majsoul.py:262-280`.
    let kakans = collect_candidate_consumes(ctx.legal_actions, ActionType::Kakan);
    let ankans = collect_candidate_consumes(ctx.legal_actions, ActionType::Ankan);
    let unified: Vec<Vec<String>> = kakans.iter().chain(ankans.iter()).cloned().collect();
    if unified.len() <= 1 {
        return; // single option → Majsoul auto-confirms
    }

    // Identify the consumed tile of the bot's action.
    let consumed_pai = match ctx.action {
        MjaiEvent::Ankan { consumed, .. } => consumed.first().cloned(),
        MjaiEvent::Kakan { pai, .. } => Some(pai.clone()),
        _ => None,
    };
    let Some(consumed_pai) = consumed_pai else {
        return;
    };
    // Strip the red-five marker for matching (kan candidate row uses the
    // base tile name; the engine's consume_tiles include the red-five).
    let base = if consumed_pai.ends_with('r') {
        consumed_pai[..consumed_pai.len() - 1].to_string()
    } else {
        consumed_pai
    };

    // Find the candidate index by matching on the first non-red-five
    // tile of each candidate's consume list. For ankan all four are
    // copies of the same suit/rank; for kakan the consume is a triplet
    // of the same tile.
    let idx = unified.iter().position(|c| {
        let any = c
            .iter()
            .map(|t| {
                if t.ends_with('r') {
                    &t[..t.len() - 1]
                } else {
                    t.as_str()
                }
            })
            .next();
        any.map(|t| t == base).unwrap_or(false)
    });
    let Some(idx) = idx else {
        return;
    };

    if let Some(p) = kan_candidate_pos(idx, unified.len()) {
        // Suppress unused-variable warning when the action type is unused
        // — kept in the signature so future logic can branch on it.
        let _ = at;
        result.steps.push(Step::Sleep {
            duration_ms: ctx.cfg.inter_click_delay_ms.ms(),
        });
        result.steps.push(Step::Click {
            x_norm: p.0,
            y_norm: p.1,
        });
    }
}

/// Look up the on-screen position of the action button for `op`, given
/// the currently-legal actions.
fn action_button_for(op: MajsoulOpType, ctx: &ActionContext) -> Option<(f64, f64)> {
    let ops = legal_op_set(ctx.legal_actions, ctx.snapshot, ctx.our_seat);
    action_button_pos(&ops, op)
}

/// Build the deduplicated set of Majsoul op-types currently legal,
/// always including [`MajsoulOpType::None`] (the "pass / cancel"
/// button is always shown when any decision is required).
fn legal_op_set(
    legal: &[Action],
    snapshot: &crate::game_state::snapshot::GameStateSnapshot,
    our_seat: u8,
) -> Vec<MajsoulOpType> {
    use std::collections::HashSet;
    let mut set: HashSet<MajsoulOpType> = HashSet::new();

    // Pass is always present alongside any prompt.
    set.insert(MajsoulOpType::None);

    let mut hora_seen_tsumo = false;
    let mut hora_seen_ron = false;

    for a in legal {
        match a.action_type {
            ActionType::Discard => { /* no button */ }
            ActionType::Tsumo => {
                hora_seen_tsumo = true;
                set.insert(MajsoulOpType::Zimo);
            }
            ActionType::Ron => {
                hora_seen_ron = true;
                set.insert(MajsoulOpType::Ron);
            }
            other => {
                if let Some(op) = MajsoulOpType::from_engine(other) {
                    if op != MajsoulOpType::None {
                        set.insert(op);
                    }
                }
            }
        }
    }

    // 3p Nukidora isn't always exposed via legal_actions in the engine
    // (the Python reference checks tehai_vec34 for an N tile directly).
    // Mirror that: if we have N tiles in hand and the kita meld is
    // legal in the rules path, surface the button. Conservative: only
    // add when not already set and we're playing 3p.
    if snapshot.num_players == 3 {
        if let Some(player) = snapshot.players.get(our_seat as usize) {
            if player.tehai.iter().any(|t| t == "N") {
                set.insert(MajsoulOpType::Nukidora);
            }
        }
    }

    let _ = (hora_seen_tsumo, hora_seen_ron);
    let mut v: Vec<MajsoulOpType> = set.into_iter().collect();
    v.sort_by_key(|op| ACTION_PRIORITY[*op as usize]);
    v
}

/// Pull all consume-tile combinations for one action type out of the
/// legal-action list, normalised to mjai tile strings and reduced to
/// what Majsoul actually renders. The engine enumerates one action per
/// physical tile copy — a hand holding two identical 6s yields two
/// `[4s, 6s]` chi entries — while the on-screen candidate row is
/// deduplicated by tile kind. The row is laid out left-to-right in
/// ascending tile order with a red five directly left of its normal
/// five, so after sorting and deduping, an index into the returned
/// list is the on-screen slot index.
fn collect_candidate_consumes(legal: &[Action], at: ActionType) -> Vec<Vec<String>> {
    let mut candidates: Vec<Vec<String>> = legal
        .iter()
        .filter(|a| a.action_type == at)
        .map(|a| {
            let mut tiles: Vec<String> = a.consume_tiles.iter().copied().map(tid_to_mjai).collect();
            tiles.sort_by(|a, b| compare_pai(a, b));
            tiles
        })
        .collect();
    candidates.sort_by(|a, b| compare_consumed(a, b));
    candidates.dedup();
    candidates
}

/// Lexicographic order over consumed-tile lists in canonical mjai tile
/// order (`compare_pai`: red five sorts before its normal five), ties
/// broken by length. Matches Majsoul's left-to-right candidate order.
fn compare_consumed(a: &[String], b: &[String]) -> std::cmp::Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = compare_pai(x, y);
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

/// Equality on consumed-tile lists. Both sides expected pre-sorted with
/// `compare_pai`, but use a length-aware element check so the comparison
/// doesn't silently succeed on mismatched lengths.
fn same_consumed(a: &[String], b: &[String]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

/// True when the bot's hora is on its own draw — in mjai both tsumo
/// agari and ron are emitted as `MjaiEvent::Hora`, but Majsoul's button
/// position differs. Distinguish by consulting the engine's legal
/// actions: if Tsumo is legal for our seat, the agari is on our draw.
fn hora_is_tsumo(ctx: &ActionContext) -> bool {
    ctx.legal_actions
        .iter()
        .any(|a| a.action_type == ActionType::Tsumo)
}

/// True iff Majsoul is currently showing the Pass button — that is, we
/// are in `Phase::WaitResponse` for our seat and have at least one
/// claim option (or just the bare Pass entry that riichienv always
/// emits in WaitResponse).
fn pass_button_visible(ctx: &ActionContext) -> bool {
    ctx.legal_actions
        .iter()
        .any(|a| a.action_type == ActionType::Pass)
}

/// True when our own riichi draw has an operation prompt open — the
/// engine offers something beyond the forced tsumogiri. While such a
/// prompt is showing, Majsoul does NOT auto-discard; it waits for an
/// answer, so a bot decision to tsumogiri must be executed by clicking
/// the X (decline), never by doing nothing.
///
/// `dahai_pai` is the tile the bot wants to tsumogiri — during riichi
/// this is always the drawn tile. It gates the kita case: the engine
/// generates a Kita action for *every* North in hand, including one
/// locked into the riichi'd hand's structure where Majsoul shows no
/// prompt (riichi only allows pulling the just-drawn North). Ankan and
/// tsumo need no such gate — the engine only emits them for the drawn
/// tile.
fn riichi_prompt_pending(ctx: &ActionContext, dahai_pai: &str) -> bool {
    ctx.legal_actions.iter().any(|a| match a.action_type {
        ActionType::Kita => dahai_pai == "N",
        ActionType::Ankan | ActionType::Tsumo => true,
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::context::CanvasRect;
    use crate::config::MajsoulAutoplayConfig;
    use crate::game_state::snapshot::{GameStateSnapshot, Phase, PlayerSnapshot};
    use coords::CANDIDATES;

    fn cfg() -> MajsoulAutoplayConfig {
        MajsoulAutoplayConfig {
            pre_click_delay_min_ms: 0,
            pre_click_delay_max_ms: 0,
            inter_click_delay_ms: crate::config::MsRange::fixed(0),
            hover_delay_ms: crate::config::MsRange::fixed(0),
            click_hold_ms: crate::config::MsRange::fixed(0),
            verify_input_ms: crate::config::MsRange::fixed(0),
            click_retries: 0,
            reload_after_failures: 0,
            dealer_first_discard_extra_delay_ms: crate::config::MsRange::fixed(0),
        }
    }

    fn cfg_with_dealer_delay(ms: u32) -> MajsoulAutoplayConfig {
        let mut c = cfg();
        c.dealer_first_discard_extra_delay_ms = crate::config::MsRange::fixed(ms);
        c
    }

    fn snapshot_with_oya(seat: u8, oya: u8, tehai: Vec<&str>) -> GameStateSnapshot {
        let mut s = snapshot_with_hand(seat, tehai);
        s.oya = oya;
        s
    }

    fn snapshot_with_hand(seat: u8, tehai: Vec<&str>) -> GameStateSnapshot {
        let players = (0..4u8)
            .map(|i| PlayerSnapshot {
                seat: i,
                tehai: if i == seat {
                    tehai.iter().map(|s| s.to_string()).collect()
                } else {
                    Vec::new()
                },
                melds: Vec::new(),
                river: Vec::new(),
                score: 25000,
                riichi_declared: false,
                riichi_stage: false,
                double_riichi: false,
                riichi_declaration_index: None,
                kita_tiles: Vec::new(),
                drawn_tile: None,
            })
            .collect();
        GameStateSnapshot {
            bakaze: "E".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            current_player: seat,
            turn_count: 0,
            phase: Phase::WaitAct,
            is_done: false,
            num_players: 4,
            players,
            dora_markers: Vec::new(),
            our_seat: Some(seat),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ctx_for<'a>(
        action: &'a MjaiEvent,
        snapshot: &'a GameStateSnapshot,
        legal: &'a [Action],
        last_kawa: Option<&'a str>,
        last_tsumo: Option<&'a str>,
        riichi_accepted: bool,
        cfg_ref: &'a MajsoulAutoplayConfig,
    ) -> ActionContext<'a> {
        ActionContext {
            action,
            snapshot,
            legal_actions: legal,
            our_seat: snapshot.our_seat.unwrap_or(0),
            last_kawa_tile: last_kawa,
            last_self_tsumo: last_tsumo,
            self_riichi_accepted: riichi_accepted,
            num_players: snapshot.num_players,
            cfg: cfg_ref,
            delay_cfg: crate::config::DelayModelConfig::default(),
            budget: None,
            probs: None,
            reading: Default::default(),
            delay_script: None,
            tenhou: None,
        }
    }

    #[test]
    fn dahai_simple_click() {
        // Non-dealer (oya = seat 1, we are seat 0) so the dealer-first-
        // discard layout doesn't apply — this test exercises the normal
        // tsumohai-offset path.
        let snap = snapshot_with_oya(
            0,
            1,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &[], Some("1m"), Some("5p"), false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        // sleep + click
        assert_eq!(result.steps.len(), 2);
        match &result.steps[1] {
            Step::Click { x_norm, .. } => {
                // Tsumohai (5p) → idx 13, with TSUMO_SPACE offset.
                let expected = TILES[13].0 + TSUMO_SPACE;
                assert!(
                    (*x_norm - expected).abs() < 1e-9,
                    "expected tsumohai at {expected}, got {x_norm}"
                );
            }
            _ => panic!("second step should be a click"),
        }
    }

    #[test]
    fn dahai_suppressed_under_riichi() {
        let snap = snapshot_with_hand(
            0,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: true,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &[], Some("1m"), Some("5p"), true, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(result.steps.is_empty(), "no click while riichi accepted");
    }

    /// Regression (2026-08-22, West 1): a bot `hora` on a robbed kan must
    /// click the Ron button. The engine's legal set used to be empty after
    /// an opponent kakan, so the button never resolved and the window hung
    /// until a human pressed Ron.
    #[test]
    fn chankan_hora_clicks_the_ron_button() {
        let mut snap = snapshot_with_hand(
            0,
            vec![
                "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5p", "6p", "7p", "5s",
            ],
        );
        snap.phase = Phase::WaitResponse;
        let act = MjaiEvent::Hora {
            actor: 0,
            target: 1,
            deltas: None,
            ura_markers: None,
        };
        // What the tracker now offers on a robable kakan: the ron plus the
        // always-present pass.
        let legal = vec![
            Action::new(ActionType::Ron, Some(88), vec![], Some(0)), // 5s
            Action::new(ActionType::Pass, None, vec![], Some(0)),
        ];
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &legal, Some("4z"), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert_eq!(result.steps.len(), 2, "sleep + ron click, got {result:?}");
        match &result.steps[1] {
            Step::Click { x_norm, y_norm } => {
                // [Ron, Pass] sorted by priority → Ron in slot 1, one left
                // of the always-rightmost pass.
                assert_eq!(*x_norm, 8.637_5);
                assert_eq!(*y_norm, 7.0);
            }
            _ => panic!("second step should be a click"),
        }
    }

    /// The decline side of the same window: a bot `none` on a robbed kan
    /// must click the pass button rather than leave the client hanging.
    #[test]
    fn chankan_decline_clicks_the_pass_button() {
        let mut snap = snapshot_with_hand(
            0,
            vec![
                "2m", "3m", "4m", "5m", "6m", "7m", "2p", "3p", "4p", "5p", "6p", "7p", "5s",
            ],
        );
        snap.phase = Phase::WaitResponse;
        let act = MjaiEvent::None;
        let legal = vec![
            Action::new(ActionType::Ron, Some(88), vec![], Some(0)), // 5s
            Action::new(ActionType::Pass, None, vec![], Some(0)),
        ];
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &legal, Some("4z"), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert_eq!(result.steps.len(), 2, "sleep + pass click, got {result:?}");
        match &result.steps[1] {
            Step::Click { x_norm, y_norm } => {
                assert_eq!(*x_norm, 10.875);
                assert_eq!(*y_norm, 7.0);
            }
            _ => panic!("second step should be a click"),
        }
    }

    /// Regression: in sanma, drawing a North while in riichi opens the
    /// kita prompt and Majsoul holds the auto-discard until it is
    /// answered. The bot deciding to tsumogiri the North must decline
    /// the prompt via the X button — the old unconditional suppression
    /// returned no steps and the game hung until the turn timer and the
    /// entire time bank drained.
    #[test]
    fn dahai_of_drawn_north_under_riichi_declines_kita_prompt() {
        let snap = snapshot_3p_with_kita(
            0,
            1,
            vec![
                "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "5s", "5s", "E", "E", "N",
            ],
            vec![],
            Some("N"),
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "N".into(),
            tsumogiri: true,
        };
        let cfg_ref = cfg();
        // What the engine offers on a drawn North during riichi: the
        // forced tsumogiri plus the kita (N = tile ids 120..=123).
        let legal = vec![
            Action::new(ActionType::Discard, Some(120), vec![], Some(0)),
            Action::new(ActionType::Kita, Some(120), vec![], Some(0)),
        ];
        let ctx = ctx_for(&act, &snap, &legal, Some("1z"), Some("N"), true, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert_eq!(result.steps.len(), 2, "sleep + X click, got {result:?}");
        match &result.steps[1] {
            Step::Click { x_norm, y_norm } => {
                // The X (pass / cancel) is always the rightmost button —
                // slot 0 — regardless of what else is on the row.
                assert_eq!(*x_norm, 10.875);
                assert_eq!(*y_norm, 7.0);
            }
            _ => panic!("expected a click on the X button"),
        }
    }

    /// A structural North locked inside a riichi'd hand makes the engine
    /// offer Kita on every subsequent draw, but Majsoul only prompts
    /// when the drawn tile itself is the North — an ordinary tsumogiri
    /// must stay suppressed (a ghost X click would land on empty UI).
    #[test]
    fn dahai_under_riichi_with_hand_north_but_other_draw_stays_suppressed() {
        let snap = snapshot_3p_with_kita(
            0,
            1,
            vec![
                "1m", "2m", "3m", "4p", "5p", "6p", "7s", "8s", "9s", "5s", "5s", "N", "N", "5p",
            ],
            vec![],
            Some("5p"),
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: true,
        };
        let cfg_ref = cfg();
        let legal = vec![
            Action::new(ActionType::Discard, Some(53), vec![], Some(0)),
            Action::new(ActionType::Kita, Some(120), vec![], Some(0)),
        ];
        let ctx = ctx_for(&act, &snap, &legal, Some("1z"), Some("5p"), true, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            result.steps.is_empty(),
            "no prompt on screen — stay suppressed, got {result:?}"
        );
    }

    /// The same trap with a riichi-legal ankan (3p and 4p alike): the
    /// kan prompt holds the auto-discard, so a bot decision to keep the
    /// hand closed must decline via the X button.
    #[test]
    fn dahai_under_riichi_with_ankan_prompt_declines_via_x() {
        let snap = snapshot_with_oya(
            0,
            1,
            vec![
                "1m", "1m", "1m", "4p", "5p", "6p", "7s", "8s", "9s", "5s", "5s", "E", "E", "1m",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: true,
        };
        let cfg_ref = cfg();
        let legal = vec![
            Action::new(ActionType::Discard, Some(3), vec![], Some(0)),
            Action::new(ActionType::Ankan, Some(0), vec![0, 1, 2, 3], Some(0)),
        ];
        let ctx = ctx_for(&act, &snap, &legal, Some("1z"), Some("1m"), true, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert_eq!(result.steps.len(), 2, "sleep + X click, got {result:?}");
        match &result.steps[1] {
            Step::Click { x_norm, y_norm } => {
                assert_eq!(*x_norm, 10.875);
                assert_eq!(*y_norm, 7.0);
            }
            _ => panic!("expected a click on the X button"),
        }
    }

    /// Regression: `is_post_call` was derived from `hand_len % 3 == 1`,
    /// which is never true — with the drawn tile inside `tehai`, both
    /// post-draw and post-call hands are ≡ 2 (mod 3) — so the calibrated
    /// post-call distribution and the script's `ctx.post_call` never
    /// fired. The real signal is a melded hand with no tracked draw.
    #[test]
    fn post_call_discard_is_detected_by_missing_draw() {
        use crate::game_state::snapshot::{MeldKind, MeldSnapshot};

        let assert_script = |expected: bool| {
            crate::autoplay::delay::DelayScript::compile(
                &format!(
                    "function decide_delay(ctx)
                       assert(ctx.post_call == {expected}, 'post_call must be {expected}')
                       return {{ delay_ms = 3456 }}
                     end"
                ),
                "test",
            )
            .unwrap()
        };

        // Post-call: one meld, no drawn tile (the pon consumed the draw).
        let mut snap = snapshot_with_oya(
            0,
            1,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p",
            ],
        );
        snap.players[0].melds.push(MeldSnapshot {
            kind: MeldKind::Pon,
            tiles: vec!["P".into(), "P".into(), "P".into()],
            from_who: 1,
            called_tile: Some("P".into()),
        });
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "2p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        let script = assert_script(true);
        let mut ctx = ctx_for(&act, &snap, &[], Some("1m"), None, false, &cfg_ref);
        ctx.delay_script = Some(&script);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            matches!(
                result.steps.first(),
                Some(Step::Sleep { duration_ms: 3456 })
            ),
            "script must see post_call=true (a fallback means its assert fired): {:?}",
            result.steps.first()
        );

        // Post-draw discard on the same melded hand: draw tracked.
        snap.players[0].drawn_tile = Some("2p".into());
        let script = assert_script(false);
        let mut ctx = ctx_for(&act, &snap, &[], Some("1m"), None, false, &cfg_ref);
        ctx.delay_script = Some(&script);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            matches!(
                result.steps.first(),
                Some(Step::Sleep { duration_ms: 3456 })
            ),
            "script must see post_call=false: {:?}",
            result.steps.first()
        );
    }

    /// Regression: the click sequence (hover + hold + candidate clicks)
    /// lands *after* the pre-click sleep, so it must be deducted from
    /// the target — otherwise every action systematically overruns the
    /// modelled server-observed total by the click overhead.
    #[test]
    fn click_overhead_is_deducted_from_sleep() {
        let script = crate::autoplay::delay::DelayScript::compile(
            "function decide_delay(ctx) return { delay_ms = 3000 } end",
            "test",
        )
        .unwrap();
        let snap = snapshot_with_hand(0, vec!["1m", "2m", "3m"]);
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "3m".into(),
            tsumogiri: false,
        };
        let mut cfg_ref = cfg();
        cfg_ref.hover_delay_ms = crate::config::MsRange::fixed(200);
        cfg_ref.click_hold_ms = crate::config::MsRange::fixed(100); // single-click dahai → 300ms overhead
        let mut ctx = ctx_for(&act, &snap, &[], Some("1m"), None, false, &cfg_ref);
        ctx.delay_script = Some(&script);

        // No budget clock: target minus the click overhead.
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            matches!(
                result.steps.first(),
                Some(Step::Sleep { duration_ms: 2700 })
            ),
            "no-budget sleep must be target − overhead: {:?}",
            result.steps.first()
        );

        // With a budget clock: elapsed time comes off as well.
        ctx.budget = Some(crate::autoplay::delay::BudgetSnapshot {
            fixed_ms: 60_000,
            add_ms: 0,
            elapsed_ms: 500,
        });
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            matches!(
                result.steps.first(),
                Some(Step::Sleep { duration_ms: 2200 })
            ),
            "budgeted sleep must be target − elapsed − overhead: {:?}",
            result.steps.first()
        );
    }

    /// The built-in model's target is held to the floor after the overhead
    /// deduction: a target sitting on the functional floor keeps its full
    /// pre-click pause.
    #[test]
    fn overhead_deduction_keeps_ui_floor() {
        let snap = snapshot_with_hand(0, vec!["1m", "2m", "3m"]);
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "3m".into(),
            tsumogiri: false,
        };
        let mut cfg_ref = cfg();
        cfg_ref.hover_delay_ms = crate::config::MsRange::fixed(200);
        cfg_ref.click_hold_ms = crate::config::MsRange::fixed(100);
        let ctx = ctx_for(&act, &snap, &[], Some("1m"), None, false, &cfg_ref);
        let floor = ctx.delay_cfg.min_delay_ms.ms();

        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            matches!(
                result.steps.first(),
                Some(Step::Sleep { duration_ms }) if *duration_ms == floor
            ),
            "sleep must not dip below the UI-readiness floor: {:?}",
            result.steps.first()
        );
    }

    /// A script's answer is final: it owns the floor (and sees the presses'
    /// time as `click_ms`), so its target less the overhead is the sleep.
    #[test]
    fn a_scripts_answer_is_slept_as_is() {
        let script = crate::autoplay::delay::DelayScript::compile(
            "function decide_delay(ctx) return { delay_ms = 1000 + ctx.click_ms } end",
            "test",
        )
        .unwrap();
        let snap = snapshot_with_hand(0, vec!["1m", "2m", "3m"]);
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "3m".into(),
            tsumogiri: false,
        };
        let mut cfg_ref = cfg();
        cfg_ref.hover_delay_ms = crate::config::MsRange::fixed(200);
        cfg_ref.click_hold_ms = crate::config::MsRange::fixed(100);
        let mut ctx = ctx_for(&act, &snap, &[], Some("1m"), None, false, &cfg_ref);
        ctx.delay_cfg.min_delay_ms = crate::config::MsRange::fixed(5000);
        ctx.delay_script = Some(&script);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            matches!(
                result.steps.first(),
                Some(Step::Sleep { duration_ms: 1000 })
            ),
            "no floor added to a script's answer: {:?}",
            result.steps.first()
        );
    }

    #[test]
    fn reach_path_a_two_clicks() {
        let snap = snapshot_with_hand(
            0,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Reach {
            actor: 0,
            pai: Some("5p".into()),
        };
        let cfg_ref = cfg();
        // Reach must be in legal_actions for the button position to resolve.
        let legal = vec![Action::new(ActionType::Riichi, None, vec![], Some(0))];
        let ctx = ctx_for(&act, &snap, &legal, Some("1m"), Some("5p"), false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        // sleep + reach btn + sleep + tile click
        assert_eq!(result.steps.len(), 4);
        assert!(matches!(result.steps[1], Step::Click { .. }));
        assert!(matches!(result.steps[3], Step::Click { .. }));
    }

    /// A reach that still names no tile (the manager's follow-up failed to
    /// resolve it, #257) declares nothing — pressing the button alone would
    /// leave the client owing a discard until timeout.
    #[test]
    fn reach_without_pai_declines() {
        let snap = snapshot_with_hand(0, vec!["1m"]);
        let act = MjaiEvent::Reach {
            actor: 0,
            pai: None,
        };
        let cfg_ref = cfg();
        let legal = vec![Action::new(ActionType::Riichi, None, vec![], Some(0))];
        let ctx = ctx_for(&act, &snap, &legal, Some("1m"), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            result.steps.is_empty(),
            "a tile-less reach must not press anything"
        );
    }

    #[test]
    fn pass_button_clicks_at_slot_zero() {
        // Pass is in legal_actions only when riichienv is in WaitResponse
        // and there's an actual claim option. Synthesise that state:
        let mut snap = snapshot_with_hand(0, vec!["1m"]);
        snap.phase = Phase::WaitResponse;
        let act = MjaiEvent::None;
        let cfg_ref = cfg();
        let legal = vec![
            Action::new(ActionType::Pass, None, vec![], Some(0)),
            Action::new(ActionType::Pon, None, vec![], Some(0)),
        ];
        let ctx = ctx_for(&act, &snap, &legal, Some("1m"), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert_eq!(result.steps.len(), 2);
        match &result.steps[1] {
            Step::Click { x_norm, y_norm } => {
                // ACTIONS[0] is the pass slot (rightmost top row).
                assert_eq!(*x_norm, 10.875);
                assert_eq!(*y_norm, 7.0);
            }
            _ => panic!("expected click"),
        }
    }

    #[test]
    fn none_does_not_click_when_no_pass_in_legal_actions() {
        // Bot emits None on every event from other players (purely
        // informational echoes). Without the gate we'd loop-click the
        // cancel button on every other-player turn.
        let snap = snapshot_with_hand(0, vec!["1m"]);
        let act = MjaiEvent::None;
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &[], Some("1m"), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            result.steps.is_empty(),
            "None must not click when Pass is not in legal_actions"
        );
    }

    #[test]
    fn none_does_not_click_during_our_discard_turn() {
        // WaitAct phase: legal_actions has Discard but no Pass — a bot
        // emitting None here is buggy, but the gate must hold.
        let snap = snapshot_with_hand(0, vec!["1m", "2m"]);
        let act = MjaiEvent::None;
        let cfg_ref = cfg();
        let legal = vec![
            Action::new(ActionType::Discard, Some(0), vec![], Some(0)),
            Action::new(ActionType::Discard, Some(4), vec![], Some(0)),
        ];
        let ctx = ctx_for(&act, &snap, &legal, Some("1m"), Some("1m"), false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(
            result.steps.is_empty(),
            "None must not click during a discard turn (no Pass button shown)"
        );
    }

    #[test]
    fn dealer_first_discard_uses_continuous_layout_no_tsumo_offset() {
        // Dealer with 14 tiles, empty river, no melds. Mahjong Soul lays
        // all 14 sorted on the rack — no tsumohai gap. Discarding the
        // sorted-last tile (5p) must click TILES[13] directly, NOT
        // TILES[13] + TSUMO_SPACE.
        let snap = snapshot_with_oya(
            0,
            0,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        // Even with last_self_tsumo set, dealer-first-discard layout
        // must override the tsumohai-offset path.
        let ctx = ctx_for(&act, &snap, &[], None, Some("5p"), false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert_eq!(result.steps.len(), 2);
        match &result.steps[1] {
            Step::Click { x_norm, .. } => {
                assert!(
                    (*x_norm - TILES[13].0).abs() < 1e-9,
                    "expected raw TILES[13] (no TSUMO_SPACE), got {x_norm}"
                );
            }
            _ => panic!("expected click"),
        }
    }

    #[test]
    fn dealer_first_discard_pads_extra_delay() {
        let snap = snapshot_with_oya(
            0,
            0,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg_with_dealer_delay(2000);
        let ctx = ctx_for(&act, &snap, &[], None, None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        // The dealer-pad wait is folded into the single pre-delay sleep:
        // [pre-delay sleep (>= animation pad), click]. With the zeroed
        // test config the sleep is exactly the pad.
        assert_eq!(result.steps.len(), 2);
        match &result.steps[0] {
            Step::Sleep { duration_ms } => assert_eq!(*duration_ms, 2000),
            _ => panic!("expected sleep step at index 0"),
        }
    }

    #[test]
    fn dealer_second_discard_uses_normal_tsumohai_path() {
        // After dealer's first discard, future turns are 13 closed + 1
        // tsumohai with the standard offset — same as non-dealer.
        let mut snap = snapshot_with_oya(
            0,
            0,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        // Mark a prior discard so river is non-empty (not first discard).
        snap.players[0]
            .river
            .push(crate::game_state::snapshot::DiscardEntry {
                tile: "9m".into(),
                tedashi: true,
                is_riichi: false,
                called: false,
            });
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &[], Some("9m"), Some("5p"), false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        match &result.steps.last().unwrap() {
            Step::Click { x_norm, .. } => {
                // Tsumohai click → TILES[13] + TSUMO_SPACE.
                let expected = TILES[13].0 + TSUMO_SPACE;
                assert!((*x_norm - expected).abs() < 1e-9);
            }
            _ => panic!("expected click"),
        }
    }

    #[test]
    fn non_dealer_first_discard_does_not_pad_or_relayout() {
        // Non-dealer's first turn: 14 tiles too, but the layout is
        // 13 closed + 1 tsumohai with TSUMO_SPACE — Majsoul does not
        // run the dealer-only sort animation.
        let snap = snapshot_with_oya(
            1,
            0, // we're seat 1, dealer is seat 0
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 1,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg_with_dealer_delay(2000);
        let ctx = ctx_for(&act, &snap, &[], None, Some("5p"), false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        // No dealer pad: just [pre-delay, click].
        assert_eq!(result.steps.len(), 2);
    }

    /// The delay model returns a target *total* thinking time; the sleep
    /// step must be target minus what the decision window has already
    /// consumed, saturating at zero — never a negative-wrapped huge sleep.
    #[test]
    fn pre_delay_deducts_budget_elapsed() {
        let snap = snapshot_with_oya(
            0,
            1,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        // Deterministic target: min == max == 1000ms, uniform mode.
        let mut cfg_ref = cfg();
        cfg_ref.pre_click_delay_min_ms = 1000;
        cfg_ref.pre_click_delay_max_ms = 1000;

        let sleep_with_elapsed = |elapsed_ms: u32| {
            let mut ctx = ctx_for(&act, &snap, &[], Some("1m"), Some("5p"), false, &cfg_ref);
            ctx.delay_cfg = crate::config::DelayModelConfig {
                distribution: crate::config::DelayDistribution::Uniform,
                ..Default::default()
            };
            ctx.budget = Some(crate::autoplay::delay::BudgetSnapshot {
                fixed_ms: 5000,
                add_ms: 0,
                elapsed_ms,
            });
            let result = MajsoulAutoplay::new().plan(&ctx);
            match result.steps[0] {
                Step::Sleep { duration_ms } => duration_ms,
                _ => panic!("expected leading sleep"),
            }
        };

        assert_eq!(sleep_with_elapsed(0), 1000);
        assert_eq!(sleep_with_elapsed(400), 600, "elapsed must be deducted");
        assert_eq!(sleep_with_elapsed(5000), 0, "sleep must saturate at zero");
    }

    /// The Path-B riichi follow-up dahai runs inside the same still-open
    /// Legacy mode must reproduce the historical fixed model even when
    /// the config still carries log-normal parameters: the distribution
    /// is forced to uniform and the sleep is exactly min==max.
    #[test]
    fn legacy_mode_forces_uniform() {
        let snap = snapshot_with_oya(
            0,
            1,
            vec![
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p",
            ],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let mut cfg_ref = cfg();
        cfg_ref.pre_click_delay_min_ms = 1234;
        cfg_ref.pre_click_delay_max_ms = 1234;
        let mut ctx = ctx_for(&act, &snap, &[], Some("1m"), Some("5p"), false, &cfg_ref);
        // Config keeps LogNormal + calibrated table, but mode is legacy.
        ctx.delay_cfg.mode = crate::config::DelayMode::Legacy;
        let result = MajsoulAutoplay::new().plan(&ctx);
        match result.steps[0] {
            Step::Sleep { duration_ms } => assert_eq!(duration_ms, 1234),
            _ => panic!("expected leading sleep"),
        }
    }

    /// Build a 3-player snapshot for our seat with optional kita pool and
    /// an explicit drawn (tsumohai) tile.
    fn snapshot_3p_with_kita(
        seat: u8,
        oya: u8,
        tehai: Vec<&str>,
        kita: Vec<&str>,
        drawn: Option<&str>,
    ) -> GameStateSnapshot {
        let players = (0..3u8)
            .map(|i| PlayerSnapshot {
                seat: i,
                tehai: if i == seat {
                    tehai.iter().map(|s| s.to_string()).collect()
                } else {
                    Vec::new()
                },
                melds: Vec::new(),
                river: Vec::new(),
                score: 35000,
                riichi_declared: false,
                riichi_stage: false,
                double_riichi: false,
                riichi_declaration_index: None,
                kita_tiles: if i == seat {
                    kita.iter().map(|s| s.to_string()).collect()
                } else {
                    Vec::new()
                },
                drawn_tile: if i == seat {
                    drawn.map(|s| s.to_string())
                } else {
                    None
                },
            })
            .collect();
        GameStateSnapshot {
            bakaze: "E".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya,
            current_player: seat,
            turn_count: 0,
            phase: Phase::WaitAct,
            is_done: false,
            num_players: 3,
            players,
            dora_markers: Vec::new(),
            our_seat: Some(seat),
        }
    }

    /// Only a dealer's first opening kita follows the deal. A second one is
    /// a fresh window and must not wait the deal out again.
    #[test]
    fn only_the_first_opening_kita_waits_out_the_deal() {
        let mut cfg_ref = cfg();
        cfg_ref.dealer_first_discard_extra_delay_ms = crate::config::MsRange::fixed(5000);
        let act = MjaiEvent::Kita {
            actor: 0,
            pai: Some("N".into()),
        };
        let legal = vec![Action::new(ActionType::Kita, Some(120), vec![], Some(0))];
        let sleep_with = |kita: Vec<&str>| {
            let tehai = vec![
                "1m", "9m", "1p", "2p", "3p", "4s", "5s", "6s", "E", "E", "S", "P", "N", "N",
            ];
            let snap = snapshot_3p_with_kita(0, 0, tehai, kita, Some("N"));
            let mut ctx = ctx_for(&act, &snap, &legal, None, Some("N"), false, &cfg_ref);
            ctx.delay_cfg.mode = crate::config::DelayMode::Legacy;
            ctx.delay_cfg.min_delay_ms = crate::config::MsRange::fixed(0);
            ctx.delay_cfg.min_button_delay_ms = crate::config::MsRange::fixed(0);
            MajsoulAutoplay::new()
                .plan(&ctx)
                .steps
                .iter()
                .find_map(|s| match s {
                    Step::Sleep { duration_ms } => Some(*duration_ms),
                    _ => None,
                })
                .expect("the kita is paced")
        };
        assert_eq!(sleep_with(vec![]), 5000, "the first kita follows the deal");
        assert_eq!(sleep_with(vec!["N"]), 0, "the second does not");
    }

    #[test]
    fn dealer_opening_kita_then_discard_is_not_off_by_one() {
        // 3p regression: dealer (oya == our seat) declares kita on the
        // opening hand, draws a rinshan replacement, then discards. The
        // North goes to the kita pool — NOT to `melds` and NOT to the
        // river — so `is_dealer_first_discard`'s naive (oya + 14 tiles +
        // empty river + empty melds) test still matched, forcing the
        // "continuous 14-tile" dealer layout. But after a kita the rinshan
        // sits as a separated tsumohai on the far right, so every closed
        // tile that sorts *after* the rinshan was clicked one slot too far
        // right. Repro: rinshan "1m" sorts first, so discarding the
        // rightmost closed tile "5p" must land on TILES[12] (its true
        // visual slot), not TILES[13] (sorted-14 index, one tile right).
        let tehai = vec![
            "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "1m",
        ];
        let snap = snapshot_3p_with_kita(0, 0, tehai, vec!["N"], Some("1m"));
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(
            &act,
            &snap,
            &[],
            None,       // opening turn: no kawa tile yet
            Some("1m"), // rinshan replacement is the live tsumohai
            false,
            &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        match result.steps.last().unwrap() {
            Step::Click { x_norm, .. } => {
                assert!(
                    (*x_norm - TILES[12].0).abs() < 1e-9,
                    "post-kita discard must click the true visual slot TILES[12] \
                     ({}), got {x_norm} (TILES[13]={} would be one tile too far right)",
                    TILES[12].0,
                    TILES[13].0,
                );
            }
            other => panic!("expected click, got {other:?}"),
        }
    }

    #[test]
    fn pixel_translation_in_canvas_rect() {
        let rect = CanvasRect {
            x: 0.0,
            y: 0.0,
            width: 1600.0,
            height: 900.0,
        };
        // Centre of the canvas at (8.0, 4.5) norm.
        assert_eq!(rect.pixel(8.0, 4.5), (800.0, 450.0));
    }

    // ----- Chi/pon candidate-slot regressions ---------------------------
    //
    // The engine enumerates one chi/pon action per physical tile copy,
    // but Majsoul renders a deduplicated, sorted candidate row. The
    // planner must map the bot's consume list onto the rendered row, not
    // the raw engine list (issues #138 / #235 / #239).

    /// Tile ids: suit base (m=0, p=36, s=72) + (rank-1)*4 + copy; copy 0
    /// of a five is the red five.
    fn chi_action(tile: u8, c1: u8, c2: u8) -> Action {
        Action::new(ActionType::Chi, Some(tile), vec![c1, c2], Some(0))
    }

    fn candidate_clicks(legal: &[Action], consumed: [&str; 2], pai: &str) -> Vec<(f64, f64)> {
        let snap = snapshot_with_hand(0, vec!["1m"]);
        let act = MjaiEvent::Chi {
            actor: 0,
            target: 3,
            pai: pai.into(),
            consumed: [consumed[0].to_string(), consumed[1].to_string()],
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, legal, Some(pai), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        result
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Click { x_norm, y_norm } => Some((*x_norm, *y_norm)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn chi_candidate_dedups_duplicate_copies_issue_138() {
        // Hand 3s 4s 6s 6s 7s, opponent discards 5s. The engine lists 5
        // combinations (two 6s copies duplicate [4s,6s] and [6s,7s]);
        // Majsoul renders 3 boxes. Choosing [3s,4s] must click the
        // leftmost of a 3-slot row, not slot 1 of a phantom 5-slot row
        // (which lands left of the whole row and hangs autoplay).
        let legal = [
            chi_action(89, 80, 84), // [3s,4s]
            chi_action(89, 84, 92), // [4s,6s] (first 6s)
            chi_action(89, 84, 93), // [4s,6s] (second 6s)
            chi_action(89, 92, 96), // [6s,7s] (first 6s)
            chi_action(89, 93, 96), // [6s,7s] (second 6s)
        ];
        let clicks = candidate_clicks(&legal, ["3s", "4s"], "5s");
        assert_eq!(clicks.len(), 2, "button click + candidate click");
        assert_eq!(clicks[1], CANDIDATES[3], "idx 0 of 3 → slot 3");
    }

    #[test]
    fn chi_candidate_slot_matches_screen_row_issue_235() {
        // Hand 2s 3s 4s 5s 6s 6s, opponent discards 0s. Engine lists
        // [3s,4s], [4s,6s], [4s,6s]; the screen shows two boxes. The
        // bot's [4s,6s] is the right box of a 2-slot row.
        let legal = [
            chi_action(88, 80, 84), // [3s,4s]
            chi_action(88, 84, 92), // [4s,6s] (first 6s)
            chi_action(88, 84, 93), // [4s,6s] (second 6s)
        ];
        let clicks = candidate_clicks(&legal, ["4s", "6s"], "5sr");
        assert_eq!(clicks.len(), 2);
        assert_eq!(clicks[1], CANDIDATES[6], "idx 1 of 2 → slot 6");
    }

    #[test]
    fn chi_single_kind_after_dedup_skips_candidate_click() {
        // Hand 4s 6s 6s, opponent discards 5s. The engine lists [4s,6s]
        // twice (two 6s copies) but only one distinct combination
        // exists, so Majsoul auto-confirms without showing a candidate
        // row — the plan must stop at the button click instead of
        // clicking a phantom two-slot row.
        let legal = [
            chi_action(89, 84, 92), // [4s,6s] (first 6s)
            chi_action(89, 84, 93), // [4s,6s] (second 6s)
        ];
        let clicks = candidate_clicks(&legal, ["4s", "6s"], "5s");
        assert_eq!(clicks.len(), 1, "button click only, no candidate row");
    }

    #[test]
    fn chi_red_five_variants_stay_distinct_and_sort_left() {
        // Hand 2s 3s 5s 0s 6s, opponent discards 4s. All five rendered
        // combinations are distinct kinds — dedup must not collapse the
        // red-five variants — and each red variant sits directly left
        // of its normal counterpart:
        //   [2s,3s] [3s,5sr] [3s,5s] [5sr,6s] [5s,6s]
        let legal = [
            chi_action(85, 76, 80), // [2s,3s]
            chi_action(85, 80, 88), // [3s,5sr]
            chi_action(85, 80, 89), // [3s,5s]
            chi_action(85, 88, 92), // [5sr,6s]
            chi_action(85, 89, 92), // [5s,6s]
        ];
        let with_normal = candidate_clicks(&legal, ["3s", "5s"], "4s");
        assert_eq!(with_normal[1], CANDIDATES[5], "idx 2 of 5 → slot 5");
        let with_red = candidate_clicks(&legal, ["3s", "5sr"], "4s");
        assert_eq!(with_red[1], CANDIDATES[3], "idx 1 of 5 → slot 3");
    }

    #[test]
    fn pon_candidate_dedups_red_five_pairs() {
        // Hand 5p 5p 0p, opponent discards 5p. The engine enumerates 3
        // pairs from the three copies; Majsoul shows two boxes with the
        // red pair on the left. Keeping the red five ([5p,5p]) is the
        // right box.
        let legal = [
            Action::new(ActionType::Pon, Some(55), vec![53, 54], Some(0)), // [5p,5p]
            Action::new(ActionType::Pon, Some(55), vec![53, 52], Some(0)), // [5pr,5p]
            Action::new(ActionType::Pon, Some(55), vec![54, 52], Some(0)), // [5pr,5p]
        ];
        let snap = snapshot_with_hand(0, vec!["1m"]);
        let act = MjaiEvent::Pon {
            actor: 0,
            target: 3,
            pai: "5p".into(),
            consumed: ["5p".into(), "5p".into()],
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(&act, &snap, &legal, Some("5p"), None, false, &cfg_ref);
        let result = MajsoulAutoplay::new().plan(&ctx);
        let clicks: Vec<(f64, f64)> = result
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Click { x_norm, y_norm } => Some((*x_norm, *y_norm)),
                _ => None,
            })
            .collect();
        assert_eq!(clicks.len(), 2);
        assert_eq!(clicks[1], CANDIDATES[6], "idx 1 of 2 → slot 6");
    }
}
