//! Majsoul implementation of [`PlatformAutoplay`].
//!
//! Coordinate tables in `coords.rs` are the only Majsoul-specific data;
//! the dispatch logic here translates a bot decision into a [`Step`]
//! sequence using:
//!
//! - The current `legal_actions` from the riichi engine (which buttons
//!   are visible, plus chi/pon/kan candidate enumeration).
//! - The current hand from `GameStateSnapshot` (sort-aware tile lookup).
//! - The reference Akagi flow at
//!   `reference/majsoul/autoplay_majsoul.py:140-281`.

pub mod coords;

use crate::autoplay::platform::{ActionContext, PlanResult, PlatformAutoplay, ReachState, Step};
use crate::bridge::majsoul::tile::compare_pai;
use crate::schema::MjaiEvent;
use coords::{
    action_button_pos, candidate_pos, get_pai_coord, kan_candidate_pos, MajsoulOpType,
    ACTION_PRIORITY, TILES,
};
#[cfg(test)]
use coords::TSUMO_SPACE;
use rand::Rng;
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
                // While in riichi, Majsoul auto-discards. Suppress unless
                // this dahai is the riichi-declaring tile (Path B follow-up).
                if ctx.self_riichi_accepted && ctx.reach_state != ReachState::AwaitingDahai {
                    return result;
                }
                push_random_pre_delay(&mut result.steps, ctx);
                if is_dealer_first_discard(ctx) && ctx.cfg.dealer_first_discard_extra_delay_ms > 0 {
                    // Majsoul plays a hand-sort animation when the dealer
                    // gets dealt all 14 tiles at once; clicks issued during
                    // it are dropped. Pad the wait.
                    result.steps.push(Step::Sleep {
                        duration_ms: ctx.cfg.dealer_first_discard_extra_delay_ms,
                    });
                }
                if let Some(click) = plan_dahai_click(pai, ctx) {
                    result.steps.push(click);
                }
            }

            // ----- Reach (立直) — two paths -------------------------------
            MjaiEvent::Reach { actor, pai } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
                if let Some(button) = action_button_for(MajsoulOpType::Reach, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                } else {
                    // Reach not in legal_actions — bridge desync; bail.
                    return PlanResult::default();
                }

                match pai {
                    // Path A: bot pre-filled the riichi tile.
                    Some(tile) => {
                        result.steps.push(Step::Sleep {
                            duration_ms: ctx.cfg.inter_click_delay_ms,
                        });
                        if let Some(click) = plan_dahai_click(tile, ctx) {
                            result.steps.push(click);
                        }
                    }
                    // Path B: bot needs a synthetic Reach event before it
                    // emits the riichi-declaring dahai. Manager will inject.
                    None => {
                        result.inject_reach_for_followup = true;
                        result.awaiting_riichi_dahai = true;
                    }
                }
            }

            // ----- Chi / Pon / Daiminkan / Ankan / Kakan -------------------
            // (action button + optional candidate disambiguation)
            MjaiEvent::Chi { actor, .. } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
                plan_meld(MajsoulOpType::Chi, ActionType::Chi, &mut result, ctx);
            }
            MjaiEvent::Pon { actor, .. } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
                plan_meld(MajsoulOpType::Pon, ActionType::Pon, &mut result, ctx);
            }
            MjaiEvent::Daiminkan { actor, .. } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
                plan_meld(MajsoulOpType::Daiminkan, ActionType::Daiminkan, &mut result, ctx);
            }
            MjaiEvent::Ankan { actor, .. } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
                plan_kan(MajsoulOpType::Ankan, ActionType::Ankan, &mut result, ctx);
            }
            MjaiEvent::Kakan { actor, .. } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
                plan_kan(MajsoulOpType::Kakan, ActionType::Kakan, &mut result, ctx);
            }

            // ----- Hora — zimo button on own draw, ron on opponent ---------
            MjaiEvent::Hora { actor, .. } if *actor == ctx.our_seat => {
                let op = if hora_is_tsumo(ctx) {
                    MajsoulOpType::Zimo
                } else {
                    MajsoulOpType::Ron
                };
                push_random_pre_delay(&mut result.steps, ctx);
                if let Some(button) = action_button_for(op, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                }
            }

            // ----- Ryukyoku (九種九牌) -------------------------------------
            MjaiEvent::Ryukyoku { .. } => {
                push_random_pre_delay(&mut result.steps, ctx);
                if let Some(button) = action_button_for(MajsoulOpType::Ryukyoku, ctx) {
                    result.steps.push(Step::Click {
                        x_norm: button.0,
                        y_norm: button.1,
                    });
                }
            }

            // ----- Kita (3p 北抜き) ----------------------------------------
            MjaiEvent::Kita { actor, .. } if *actor == ctx.our_seat => {
                push_random_pre_delay(&mut result.steps, ctx);
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
                push_random_pre_delay(&mut result.steps, ctx);
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

fn push_random_pre_delay(steps: &mut Vec<Step>, ctx: &ActionContext) {
    let lo = ctx.cfg.pre_click_delay_min_ms;
    let hi = ctx.cfg.pre_click_delay_max_ms.max(lo);
    let mut delay = if hi == lo {
        lo
    } else {
        rand::rng().random_range(lo..=hi)
    };
    // Reference behaviour (`autoplay_majsoul.py:156-157`): if there's no
    // `last_kawa_tile` (i.e. very first action of a kyoku), use the upper
    // bound as a fixed delay. Slightly slower but more human-like on the
    // opening turn.
    if ctx.last_kawa_tile.is_none() {
        delay = hi;
    }
    steps.push(Step::Sleep { duration_ms: delay });
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
        return Some(Step::Click { x_norm: x, y_norm: y });
    }

    let mut tehai: Vec<String> = ctx.snapshot.players[our_seat].tehai.clone();
    let tsumohai = ctx.last_self_tsumo;

    // Detect tsumohai: hand sizes that include the just-drawn tile are
    // 2/5/8/11/14 (mod 3 = 2). When present and known, separate it out.
    let mut is_tsumohai = false;
    if matches!(tehai.len(), 14 | 11 | 8 | 5 | 2) {
        if let Some(t) = tsumohai {
            if let Some(pos) = tehai.iter().rposition(|x| x == t) {
                tehai.remove(pos);
                is_tsumohai = true;
            }
        }
    }
    tehai.sort_by(|a, b| compare_pai(a, b));

    if is_tsumohai {
        if let Some(t) = tsumohai {
            if pai == t {
                let (x, y) = get_pai_coord(13, tehai.len());
                return Some(Step::Click { x_norm: x, y_norm: y });
            }
        }
    }

    let idx = tehai.iter().position(|x| x == pai)?;
    if idx >= TILES.len() - 1 {
        // No closed-hand slot 13 (only the tsumohai uses that path).
        return None;
    }
    let (x, y) = get_pai_coord(idx, tehai.len());
    Some(Step::Click { x_norm: x, y_norm: y })
}

/// True for the dealer's very first discard of a kyoku — the moment
/// when Mahjong Soul has dealt 14 tiles, played the hand-sort animation,
/// and is showing all 14 tiles continuously on the rack (no tsumohai
/// offset). Detected by: we're oya, our hand size is 14, and we have
/// no discards or melds yet.
fn is_dealer_first_discard(ctx: &ActionContext) -> bool {
    let our_seat = ctx.our_seat as usize;
    let Some(player) = ctx.snapshot.players.get(our_seat) else {
        return false;
    };
    ctx.snapshot.oya == ctx.our_seat
        && player.tehai.len() == 14
        && player.river.is_empty()
        && player.melds.is_empty()
}

/// Plan a chi/pon/daiminkan: action button click + optional candidate
/// disambiguation when multiple consume-tile combinations are legal.
fn plan_meld(
    op: MajsoulOpType,
    at: ActionType,
    result: &mut PlanResult,
    ctx: &ActionContext,
) {
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
                duration_ms: ctx.cfg.inter_click_delay_ms,
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
fn plan_kan(
    op: MajsoulOpType,
    at: ActionType,
    result: &mut PlanResult,
    ctx: &ActionContext,
) {
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
            .map(|t| if t.ends_with('r') { &t[..t.len() - 1] } else { t.as_str() })
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
            duration_ms: ctx.cfg.inter_click_delay_ms,
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
/// legal-action list, normalised to mjai tile strings.
fn collect_candidate_consumes(legal: &[Action], at: ActionType) -> Vec<Vec<String>> {
    legal
        .iter()
        .filter(|a| a.action_type == at)
        .map(|a| {
            let mut tiles: Vec<String> = a.consume_tiles.iter().copied().map(tid_to_mjai).collect();
            tiles.sort_by(|a, b| compare_pai(a, b));
            tiles
        })
        .collect::<Vec<_>>()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoplay::context::CanvasRect;
    use crate::config::MajsoulAutoplayConfig;
    use crate::game_state::snapshot::{GameStateSnapshot, Phase, PlayerSnapshot};

    fn cfg() -> MajsoulAutoplayConfig {
        MajsoulAutoplayConfig {
            pre_click_delay_min_ms: 0,
            pre_click_delay_max_ms: 0,
            inter_click_delay_ms: 0,
            hover_delay_ms: 0,
            click_hold_ms: 0,
            dealer_first_discard_extra_delay_ms: 0,
        }
    }

    fn cfg_with_dealer_delay(ms: u32) -> MajsoulAutoplayConfig {
        let mut c = cfg();
        c.dealer_first_discard_extra_delay_ms = ms;
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

    fn ctx_for<'a>(
        action: &'a MjaiEvent,
        snapshot: &'a GameStateSnapshot,
        legal: &'a [Action],
        last_kawa: Option<&'a str>,
        last_tsumo: Option<&'a str>,
        riichi_accepted: bool,
        reach_state: ReachState,
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
            reach_state,
            num_players: snapshot.num_players,
            cfg: cfg_ref,
        }
    }

    #[test]
    fn dahai_simple_click() {
        // Non-dealer (oya = seat 1, we are seat 0) so the dealer-first-
        // discard layout doesn't apply — this test exercises the normal
        // tsumohai-offset path.
        let snap = snapshot_with_oya(
            0, 1,
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p"],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(
            &act, &snap, &[], Some("1m"), Some("5p"), false, ReachState::Idle, &cfg_ref,
        );
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
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p"],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: true,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(
            &act,
            &snap,
            &[],
            Some("1m"),
            Some("5p"),
            true,
            ReachState::Idle,
            &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(result.steps.is_empty(), "no click while riichi accepted");
    }

    #[test]
    fn dahai_under_riichi_awaiting_riichi_dahai_clicks() {
        // Path B follow-up: even though manager has self_riichi_accepted=false
        // (it only flips on ReachAccepted from server), reach_state being
        // AwaitingDahai means we should click. self_riichi_accepted is
        // expected to be false here too, but the suppression check is
        // ANDed with reach_state — verify both false-cases:
        let snap = snapshot_with_hand(0, vec!["1m", "2m", "3m"]);
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "3m".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        // Hand of 3 tiles is unusual but the Path B click should not be
        // suppressed even when self_riichi_accepted=true happens to be set.
        let ctx = ctx_for(
            &act,
            &snap,
            &[],
            Some("1m"),
            None,
            true,
            ReachState::AwaitingDahai,
            &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        assert!(!result.steps.is_empty(), "Path B follow-up dahai must click");
    }

    #[test]
    fn reach_path_a_two_clicks() {
        let snap = snapshot_with_hand(
            0,
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p"],
        );
        let act = MjaiEvent::Reach {
            actor: 0,
            pai: Some("5p".into()),
        };
        let cfg_ref = cfg();
        // Reach must be in legal_actions for the button position to resolve.
        let legal = vec![Action::new(ActionType::Riichi, None, vec![], Some(0))];
        let ctx = ctx_for(
            &act, &snap, &legal, Some("1m"), Some("5p"), false, ReachState::Idle, &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        // sleep + reach btn + sleep + tile click
        assert_eq!(result.steps.len(), 4);
        assert!(matches!(result.steps[1], Step::Click { .. }));
        assert!(matches!(result.steps[3], Step::Click { .. }));
        assert!(!result.inject_reach_for_followup);
    }

    #[test]
    fn reach_path_b_signals_inject() {
        let snap = snapshot_with_hand(0, vec!["1m"]);
        let act = MjaiEvent::Reach { actor: 0, pai: None };
        let cfg_ref = cfg();
        let legal = vec![Action::new(ActionType::Riichi, None, vec![], Some(0))];
        let ctx = ctx_for(
            &act, &snap, &legal, Some("1m"), None, false, ReachState::Idle, &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        // sleep + reach btn click only
        assert_eq!(result.steps.len(), 2);
        assert!(result.inject_reach_for_followup);
        assert!(result.awaiting_riichi_dahai);
    }

    #[test]
    fn pass_button_clicks_at_slot_zero() {
        // Pass is in legal_actions only when riichienv is in WaitResponse
        // and there's something to claim/pass on. Synthesise that state:
        let snap = snapshot_with_hand(0, vec!["1m"]);
        let act = MjaiEvent::None;
        let cfg_ref = cfg();
        let legal = vec![Action::new(ActionType::Pass, None, vec![], Some(0))];
        let ctx = ctx_for(
            &act, &snap, &legal, Some("1m"), None, false, ReachState::Idle, &cfg_ref,
        );
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
        let ctx = ctx_for(
            &act, &snap, &[], Some("1m"), None, false, ReachState::Idle, &cfg_ref,
        );
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
        let ctx = ctx_for(
            &act, &snap, &legal, Some("1m"), Some("1m"), false, ReachState::Idle, &cfg_ref,
        );
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
            0, 0,
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
                 "1p", "2p", "3p", "4p", "5p"],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        // Even with last_self_tsumo set, dealer-first-discard layout
        // must override the tsumohai-offset path.
        let ctx = ctx_for(
            &act, &snap, &[], None, Some("5p"), false, ReachState::Idle, &cfg_ref,
        );
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
            0, 0,
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
                 "1p", "2p", "3p", "4p", "5p"],
        );
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "1m".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg_with_dealer_delay(2000);
        let ctx = ctx_for(
            &act, &snap, &[], None, None, false, ReachState::Idle, &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        // [pre-delay sleep, dealer-pad sleep, click]
        assert_eq!(result.steps.len(), 3);
        match &result.steps[1] {
            Step::Sleep { duration_ms } => assert_eq!(*duration_ms, 2000),
            _ => panic!("expected sleep step at index 1"),
        }
    }

    #[test]
    fn dealer_second_discard_uses_normal_tsumohai_path() {
        // After dealer's first discard, future turns are 13 closed + 1
        // tsumohai with the standard offset — same as non-dealer.
        let mut snap = snapshot_with_oya(
            0, 0,
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
                 "1p", "2p", "3p", "4p", "5p"],
        );
        // Mark a prior discard so river is non-empty (not first discard).
        snap.players[0].river.push(crate::game_state::snapshot::DiscardEntry {
            tile: "9m".into(),
            tedashi: true,
            is_riichi: false,
        });
        let act = MjaiEvent::Dahai {
            actor: 0,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg();
        let ctx = ctx_for(
            &act, &snap, &[], Some("9m"), Some("5p"), false, ReachState::Idle, &cfg_ref,
        );
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
            1, 0, // we're seat 1, dealer is seat 0
            vec!["1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m",
                 "1p", "2p", "3p", "4p", "5p"],
        );
        let act = MjaiEvent::Dahai {
            actor: 1,
            pai: "5p".into(),
            tsumogiri: false,
        };
        let cfg_ref = cfg_with_dealer_delay(2000);
        let ctx = ctx_for(
            &act, &snap, &[], None, Some("5p"), false, ReachState::Idle, &cfg_ref,
        );
        let result = MajsoulAutoplay::new().plan(&ctx);
        // No dealer pad: just [pre-delay, click].
        assert_eq!(result.steps.len(), 2);
    }

    #[test]
    fn pixel_translation_in_canvas_rect() {
        let rect = CanvasRect { x: 0.0, y: 0.0, width: 1600.0, height: 900.0 };
        // Centre of the canvas at (8.0, 4.5) norm.
        assert_eq!(rect.pixel(8.0, 4.5), (800.0, 450.0));
    }
}
