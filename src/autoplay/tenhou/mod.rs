//! Tenhou autoplay: drive the client's own input path.
//!
//! Writing frames onto the socket behind the client's back does not work: its
//! receive path deliberately ignores the server's echo of *our* discard
//! (`1==U.a && "D"==c.tag || Nb.cb(c)`) because its own handler already
//! applied it locally. Bypass that handler and the board freezes from the
//! first discard on, while the server plays the hand out without it.
//!
//! So actions are performed the way the user performs them:
//!
//! - **Buttons** — chi, pon, kan, riichi, ron, tsumo, kyuushu, kita and pass
//!   are DOM elements the client routes through a body-level click listener
//!   keyed on `name="c22-<slot>"`. A dispatched click runs the client's own
//!   handler, so nothing can drift. No coordinates involved.
//! - **Discard** — no DOM element exists for it, but it needs no position
//!   either: the client's own discard handler takes a tile *index*, and
//!   [`inject`] rewrites the client script in flight so that handler is
//!   reachable.
//!
//! What is *kept* is the delay model. Acting the instant the bot answers is
//! the most obvious tell there is, so the same human-timing policy runs here
//! (`autoplay::delay`). Tenhou never states its per-turn allowance on the
//! wire, but it has one and enforces it by auto-discarding, so the lobby's
//! standard clock is fed in as the budget — see
//! `tenhou_state::TURN_BASE_MS`.
//!
//! # Guards
//!
//! Every action is gated on a decision window the bridge saw open for our seat
//! (`tenhou_state::DecisionWindow`), so a decision that resolved while we were
//! still "thinking" cannot be answered late into the next one. Buttons carry a
//! second, independent guard for free: if the client is no longer offering
//! one, the selector matches nothing and the click is reported as skipped
//! rather than landing somewhere else.

use crate::autoplay::cdp_input::{self, menu};
use crate::autoplay::delay::{BudgetSnapshot, DecisionKind, DelayInput};
use crate::autoplay::platform::{ActionContext, PlanResult, PlatformAutoplay, Step};
use crate::autoplay::tenhou_state;
use crate::schema::MjaiEvent;
use tracing::{debug, warn};

pub mod inject;

const LOG: &str = "akagi::autoplay::tenhou";

/// How long to wait for the client to finish animating and raise its clock.
///
/// Generous because the animation is the *server's* pace, not ours: against
/// instant opponents Tenhou delivers three seats' actions together and the
/// client plays them out over seconds. Overshooting costs a skipped turn;
/// undershooting costs every turn.
const READY_TIMEOUT_MS: u32 = 8_000;

pub struct TenhouAutoplay;

impl TenhouAutoplay {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TenhouAutoplay {
    fn default() -> Self {
        Self::new()
    }
}

/// Map the bot's action onto the delay model's decision taxonomy. `None` for
/// events that are not ours to perform.
fn decision_kind(action: &MjaiEvent) -> Option<DecisionKind> {
    Some(match action {
        MjaiEvent::Dahai { .. } => DecisionKind::Dahai,
        MjaiEvent::Reach { .. } => DecisionKind::Reach,
        MjaiEvent::Chi { .. } => DecisionKind::Chi,
        MjaiEvent::Pon { .. } => DecisionKind::Pon,
        MjaiEvent::Daiminkan { .. } => DecisionKind::Daiminkan,
        MjaiEvent::Ankan { .. } => DecisionKind::Ankan,
        MjaiEvent::Kakan { .. } => DecisionKind::Kakan,
        MjaiEvent::Hora { .. } => DecisionKind::Hora,
        MjaiEvent::Ryukyoku { .. } => DecisionKind::Ryukyoku,
        MjaiEvent::Kita { .. } => DecisionKind::Kita,
        MjaiEvent::None => DecisionKind::Pass,
        _ => return None,
    })
}

/// Push the pre-send "thinking" pause.
///
/// The model's target is a *total* for the window, and here the window starts
/// when the client does — `Step::AwaitReady` has already waited out the
/// animation — so the target is the sleep with nothing to deduct.
fn push_pre_delay(steps: &mut Vec<Step>, ctx: &ActionContext, kind: DecisionKind) {
    let (is_tsumogiri, tile_class) = match ctx.action {
        MjaiEvent::Dahai { tsumogiri, pai, .. } => {
            (*tsumogiri, crate::autoplay::delay::TileClass::of_mjai(pai))
        }
        _ => (false, None),
    };
    // The only discard with no just-drawn tile is the one a call bought us.
    let is_post_call =
        matches!(ctx.action, MjaiEvent::Dahai { .. }) && ctx.tenhou.is_some_and(|t| !t.is_tsumo);
    let opponent_riichi = ctx
        .snapshot
        .players
        .iter()
        .any(|p| p.seat != ctx.our_seat && p.riichi_declared);

    let legacy = ctx.delay_cfg.mode == crate::config::DelayMode::Legacy;
    let mut delay_cfg = ctx.delay_cfg.clone();
    if legacy {
        delay_cfg.distribution = crate::config::DelayDistribution::Uniform;
    }

    let input = DelayInput {
        kind,
        is_tsumogiri,
        is_post_call,
        first_action_of_kyoku: ctx.last_kawa_tile.is_none(),
        // Nothing is being pressed, so no animation has to finish first.
        opening_animation: false,
        can_riichi: ctx
            .legal_actions
            .iter()
            .any(|a| a.action_type == riichienv_core::action::ActionType::Riichi),
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
        // Tenhou never states its allowance on the wire, but it very much
        // has one: overrun it and the client auto-discards, which is how the
        // first live run lost every turn. Feed the lobby's standard clock in
        // so the model's budget caps apply as they do on Majsoul.
        // Elapsed is zero on purpose: the plan does not run until the
        // client's clock is actually up (`Step::AwaitReady`), and that clock
        // starts then, not when the frame reached us. Deducting the time
        // spent animating would spend a budget that had not started.
        budget: ctx.tenhou.and_then(|t| t.window).map(|_| BudgetSnapshot {
            fixed_ms: tenhou_state::TURN_BASE_MS,
            add_ms: tenhou_state::TURN_BANK_MS,
            elapsed_ms: 0,
        }),
        click_overhead_ms: 0,
        cfg: ctx.cfg,
        delay_cfg: &delay_cfg,
    };

    let decision = ctx
        .delay_script
        .filter(|_| !legacy)
        .and_then(|s| s.try_decide(&input))
        .unwrap_or_else(|| crate::autoplay::delay::decide(&input, &mut rand::rng()));
    // The whole target is the sleep. It is measured from the moment the
    // client became ready, which `Step::AwaitReady` has already waited for —
    // nothing of the turn's own clock has been spent before that point.
    let sleep = decision.total_target_ms;
    if sleep > 0 {
        steps.push(Step::Sleep { duration_ms: sleep });
    }
}

/// Is this mjai tile a red five?
fn is_red(pai: &str) -> bool {
    matches!(pai, "5mr" | "5pr" | "5sr")
}

/// Rank of an mjai number tile within its suit (`1..=9`), honours excluded.
fn rank(pai: &str) -> Option<u32> {
    let body = pai.strip_suffix('r').unwrap_or(pai);
    let mut cs = body.chars();
    let r = cs.next()?.to_digit(10)?;
    matches!(cs.next(), Some('m' | 'p' | 's')).then_some(r)
}

/// Menu slot for a chi.
///
/// Read off the client's menu builder, which walks the three shapes for each
/// of three "which tile is red" cases:
///
/// ```js
/// for(n=0;3>n;++n){ p=(n&1)<<8; q=(n&2)<<7;
///   (h=b[p|cls-2])&&(a=b[q|cls-1])&&f(n?20:21,h[0],a[0]);   // called tile highest
///   (h=b[p|cls-1])&&(a=b[q|cls+1])&&f(n?18:19,h[0],a[0]);   // called tile middle
///   (h=b[p|cls+1])&&(a=b[q|cls+2])&&f(n?16:17,h[0],a[0]); } // called tile lowest
/// ```
///
/// So the odd slot of each pair is the shape made of plain copies and the
/// even one below it is the same shape spending a red five. Either can exist
/// without the other — holding only `5mr`, a `4m` chi is drawn at the even
/// slot alone — so this resolves to exactly one slot and never falls back.
fn chi_slot(pai: &str, consumed: &[String; 2]) -> Option<u8> {
    let called = rank(pai)?;
    let (a, b) = (rank(&consumed[0])?, rank(&consumed[1])?);
    let (lo, hi) = (a.min(b), a.max(b));
    let plain = if called < lo {
        menu::CHI_FIRST + 1 // called tile lowest
    } else if called > hi {
        menu::CHI_FIRST + 5 // called tile highest
    } else {
        menu::CHI_FIRST + 3 // called tile in the middle
    };
    Some(if consumed.iter().any(|c| is_red(c)) {
        plain - 1
    } else {
        plain
    })
}

/// Menu slots for a pon, in preference order.
///
/// The client draws at most two, and they are not "plain" and "red":
///
/// ```js
/// d&1&&(1==a.length&&2==h.length ? (f(14,h[0],h[1]),f(15,a[0],h[0]))  // keep red / spend it
///     : 2<=h.length ? f(15,h[0],h[1])                                  // two plain copies
///     : 1==a.length&&1==h.length && f(15,a[0],h[0]))                   // forced onto the red
/// ```
///
/// Slot 15 is always drawn and takes the red five whenever the hand holds
/// one. Slot 14 exists only when there is a choice — one red and two plain
/// copies — and it is the pair that leaves the red out.
///
/// So a pon that spends a red asks for 15 alone. One that does not asks for
/// 14 first and settles for 15, which is the same pon whenever 14 was not
/// drawn.
fn pon_slots(consumed: &[String; 2]) -> Vec<u8> {
    if consumed.iter().any(|c| is_red(c)) {
        vec![menu::PON]
    } else {
        vec![menu::PON_KEEP_RED, menu::PON]
    }
}

/// Menu slots for a kan, in preference order.
///
/// Kan candidates share three slots and are packed into them densely, in the
/// order the client builds them: every pon we hold the fourth copy of, in the
/// order those melds were called, then every concealed set of four by
/// ascending tile class.
///
/// ```js
/// for(p in U.mc[0]) ... b.push({type:kakan, hai:<copy in hand>})
/// for(h=0;34>h;++h) if(4<=p[h]) b.push({type:ankan, hai:4*h})
/// b.length&&(k[10]=b[0],b.shift()); b.length&&(k[11]=b[0],b.shift()); b.length&&(k[12]=b[0],b.shift())
/// ```
///
/// Reproducing that list is the only way to tell two offered kans apart —
/// the slot carries no tile. When it cannot be reproduced, fall back to the
/// three slots in the order the client fills them, which is right whenever
/// only one kan is on offer (the ordinary case).
fn kan_slots(action: &MjaiEvent, state: &tenhou_state::TenhouState, in_riichi: bool) -> Vec<u8> {
    const FAMILY: [u8; 3] = [menu::KAN_FIRST, menu::KAN_FIRST + 1, menu::KAN_FIRST + 2];
    // Riichi restricts the offer to an ankan of the tile just drawn, and only
    // when it leaves the wait alone, so there is never a second candidate.
    if in_riichi {
        return vec![menu::KAN_FIRST];
    }
    let wanted = match action {
        MjaiEvent::Ankan { consumed, .. } => tile_class(&consumed[0]),
        MjaiEvent::Kakan { pai, .. } => tile_class(pai),
        _ => None,
    };
    match wanted.and_then(|c| kan_candidates(state).iter().position(|k| *k == c)) {
        Some(i) if i < FAMILY.len() => vec![FAMILY[i]],
        _ => FAMILY.to_vec(),
    }
}

/// The tile classes the client will offer a kan for, in the order it offers
/// them. See [`kan_slots`].
fn kan_candidates(state: &tenhou_state::TenhouState) -> Vec<u32> {
    use crate::bridge::tenhou::meld::MeldKind;
    let mut out = Vec::new();
    for m in &state.melds {
        if m.kind != MeldKind::Pon {
            continue;
        }
        let Some(class) = m.tiles.first().map(|t| t / 4) else {
            continue;
        };
        if state.hand.iter().any(|t| t / 4 == class) {
            out.push(class);
        }
    }
    for class in 0..34 {
        if state.hand.iter().filter(|t| **t / 4 == class).count() == 4 {
            out.push(class);
        }
    }
    out
}

/// Tenhou tile class (`0..=33`) for an mjai label. Red fives share the class
/// of their plain copy, which is what a kan is addressed by.
fn tile_class(pai: &str) -> Option<u32> {
    crate::bridge::tenhou::encode::tile_class_public(pai)
}

/// One selector per slot, in the order they should be tried.
///
/// Preference order, not document order: the client appends its buttons
/// highest slot first, so the list has to be walked by the caller rather than
/// handed to `querySelector` as one comma-joined selector.
fn selectors_for(slots: &[u8]) -> Vec<String> {
    slots
        .iter()
        .map(|s| cdp_input::action_button_selector(*s))
        .collect()
}

/// Which button performs `action`, if it is a button at all. Returns the
/// slots to try in preference order — see [`selectors_for`].
fn button_for(
    action: &MjaiEvent,
    state: &tenhou_state::TenhouState,
    in_riichi: bool,
) -> Option<(Vec<String>, String)> {
    let (slots, label) = match action {
        MjaiEvent::Reach { .. } => (vec![menu::RIICHI], "riichi"),
        MjaiEvent::Hora { actor, target, .. } => {
            if actor == target {
                (vec![menu::TSUMO_AGARI], "tsumo")
            } else {
                (vec![menu::RON], "ron")
            }
        }
        MjaiEvent::Ryukyoku { .. } => (vec![menu::KYUUSHU], "kyuushu"),
        MjaiEvent::None => (vec![menu::PASS], "pass"),
        MjaiEvent::Daiminkan { .. } => (vec![menu::DAIMINKAN], "daiminkan"),
        MjaiEvent::Pon { consumed, .. } => (pon_slots(consumed), "pon"),
        MjaiEvent::Chi { pai, consumed, .. } => (vec![chi_slot(pai, consumed)?], "chi"),
        MjaiEvent::Ankan { .. } | MjaiEvent::Kakan { .. } => {
            (kan_slots(action, state, in_riichi), "kan")
        }
        // Kita shares its slots with the sanma menu's other declarations, and
        // the client assigns the North first, so the family in its own fill
        // order resolves to it.
        MjaiEvent::Kita { .. } => (
            (menu::KITA_FIRST..=menu::KITA_FIRST + 4).collect::<Vec<_>>(),
            "kita",
        ),
        _ => return None,
    };
    Some((selectors_for(&slots), label.to_string()))
}

impl PlatformAutoplay for TenhouAutoplay {
    fn plan(&self, ctx: &ActionContext) -> PlanResult {
        let mut result = PlanResult::default();

        let Some(state) = ctx.tenhou else {
            debug!(target: LOG, "no tenhou bridge state yet; skipping");
            return result;
        };
        // A window the bridge never saw open is a window the client is no
        // longer accepting input for. Also the staleness guard: our own
        // discard, or anyone else's action, closes it.
        let Some(window) = state.window else {
            debug!(target: LOG, "no open decision window; skipping {:?}", ctx.action);
            return result;
        };
        let Some(kind) = decision_kind(ctx.action) else {
            return result;
        };

        // Once riichi is accepted the client discards for itself, so ours
        // would be a second one. Claims that survive riichi (ankan, hora) are
        // still ours. The riichi tile arrives before acceptance and so is
        // unaffected.
        if kind == DecisionKind::Dahai && ctx.self_riichi_accepted {
            debug!(target: LOG, "riichi accepted; leaving the discard to the client");
            return result;
        }
        // Declining needs a claim on someone else's discard. Declining an
        // own-turn offer — tsumo agari, riichi, 九種九牌 — is just
        // discarding, and the client draws no pass button for it.
        if kind == DecisionKind::Pass && !window.has_declinable_claim() {
            return result;
        }

        // A riichi is a declaration *and* a discard, and Tenhou will not
        // take one without the other: press the button alone and the client
        // sits on its clock, then completes the riichi *itself* by throwing
        // the drawn tile at expiry. So the tile is resolved before anything
        // is pressed, and a riichi whose discard cannot be performed — a bot
        // that names no tile (nothing prompts a follow-up decision here, see
        // #257), or a tracked hand that has lost the named one — skips the
        // declaration whole. Not declaring costs this turn's riichi; a blind
        // declaration commits the hand to a wait the bot never chose.
        let reach_tile = match ctx.action {
            MjaiEvent::Reach {
                pai: Some(tile), ..
            } => match tile_index_for(state, tile, true) {
                Some(i) => Some(i),
                None => {
                    warn!(
                        target: LOG,
                        "riichi tile {tile} is not in the tracked hand; skipping the \
                         declaration rather than letting the client resolve it blind"
                    );
                    return result;
                }
            },
            MjaiEvent::Reach { pai: None, .. } => {
                warn!(
                    target: LOG,
                    "the bot declared riichi without naming the discard; Tenhou needs \
                     both in one action, so the declaration is skipped and the turn \
                     will time out to a plain tsumogiri"
                );
                return result;
            }
            _ => None,
        };

        let step = match ctx.action {
            MjaiEvent::Dahai { pai, tsumogiri, .. } => {
                // The one action with no button behind it — but still not a
                // position: the client's discard handler is addressed by tile
                // index, which the encoder already resolves. The tsumogiri
                // flag rides along because the index is also the display:
                // throwing the drawn copy is what *makes* it a tsumogiri.
                let Some(tile_index) = tile_index_for(state, pai, *tsumogiri) else {
                    warn!(target: LOG, "discard {pai} is not in the tracked hand; skipping");
                    return result;
                };
                Step::Discard { tile_index }
            }
            other => {
                let Some((selectors, label)) = button_for(other, state, ctx.self_riichi_accepted)
                else {
                    return result;
                };
                Step::DomClick { selectors, label }
            }
        };

        // Wait for the client before thinking, not after: the delay is meant
        // to look like a human deciding, and a human cannot decide before the
        // board has finished moving. This wait precedes the reach button, when
        // the client's clock is still up; the riichi tile that follows the
        // press (below) does not wait again — the declaration takes the clock
        // down and nothing raises it before the tile goes out in the same
        // plan.
        result.steps.push(Step::AwaitReady {
            timeout_ms: READY_TIMEOUT_MS,
        });
        push_pre_delay(&mut result.steps, ctx, kind);
        result.steps.push(step);

        // The declaration went out above; the tile the bot named on the
        // reach completes it in the same plan — nothing else is coming to
        // provide the second half. (Same shape as Mahjong Soul's Path A; the
        // `reach` the server echoes back is not a new decision for the bot,
        // and waiting for one is what let a declared riichi burn its turn.)
        if let Some(tile_index) = reach_tile {
            // Let the client finish taking the declaration before handing
            // it the tile.
            result.steps.push(Step::Sleep {
                duration_ms: ctx.cfg.inter_click_delay_ms,
            });
            result.steps.push(Step::Discard { tile_index });
        }
        result
    }
}

/// Resolve the mjai tile the bot named to the physical copy we hold, reusing
/// the encoder's red-aware, drawn-tile-aware lookup — the index is also the
/// tedashi/tsumogiri display, so the same copy the encoder would name must
/// be the one handed to the client.
fn tile_index_for(
    state: &crate::autoplay::tenhou_state::TenhouState,
    pai: &str,
    tsumogiri: bool,
) -> Option<u32> {
    crate::bridge::tenhou::encode::dahai_index(state.hand_view(), pai, tsumogiri)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn rank_reads_number_tiles_only() {
        assert_eq!(rank("3m"), Some(3));
        assert_eq!(rank("5pr"), Some(5));
        assert_eq!(rank("9s"), Some(9));
        assert_eq!(rank("E"), None);
        assert_eq!(rank("?"), None);
    }

    fn hand(tiles: &[u32]) -> tenhou_state::TenhouState {
        tenhou_state::TenhouState {
            seat: 0,
            hand: tiles.to_vec(),
            melds: Vec::new(),
            is_tsumo: true,
            window: None,
        }
    }

    fn slots(action: &MjaiEvent, st: &tenhou_state::TenhouState) -> Vec<String> {
        button_for(action, st, false).unwrap().0
    }

    /// The client lays chi buttons out by where the called tile sits in the
    /// run: `f(n?20:21,..)` highest, `f(n?18:19,..)` middle, `f(n?16:17,..)`
    /// lowest.
    #[test]
    fn chi_slot_follows_the_called_tile_position() {
        // Called 3m with 4m+5m → the called tile is the lowest of the run.
        assert_eq!(chi_slot("3m", &[s("4m"), s("5m")]), Some(17));
        // Called 4m with 3m+5m → middle.
        assert_eq!(chi_slot("4m", &[s("3m"), s("5m")]), Some(19));
        // Called 5m with 3m+4m → highest.
        assert_eq!(chi_slot("5m", &[s("3m"), s("4m")]), Some(21));
    }

    /// The even slot of each pair is the same shape spending a red five, and
    /// it stands alone: the `n` loop draws it whenever the red copy is held,
    /// whether or not a plain alternative exists. Accepting the odd slot as a
    /// fallback would spend the wrong copy in the case where both are drawn.
    #[test]
    fn a_chi_that_spends_a_red_takes_the_slot_below_its_shape() {
        assert_eq!(chi_slot("3m", &[s("4m"), s("5mr")]), Some(16));
        assert_eq!(chi_slot("4m", &[s("3m"), s("5mr")]), Some(18));
        assert_eq!(chi_slot("6m", &[s("5mr"), s("4m")]), Some(20));
    }

    /// Consumed order must not matter — only where the called tile lands.
    #[test]
    fn chi_slot_is_order_independent() {
        assert_eq!(
            chi_slot("4m", &[s("3m"), s("5m")]),
            chi_slot("4m", &[s("5m"), s("3m")])
        );
    }

    /// Regression (a pon that never landed, then landed on the wrong pair).
    /// Slot 15 is the pon and always exists; slot 14 is drawn only when the
    /// hand holds one red copy and two plain ones, and it is the pair that
    /// leaves the red out. So a pon spending no red prefers 14 and settles
    /// for 15 — which is the same pon whenever 14 was not drawn.
    #[test]
    fn a_pon_that_spends_no_red_prefers_the_pair_that_keeps_it() {
        let st = hand(&[0, 1]);
        let plain = MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: s("8s"),
            consumed: [s("8s"), s("8s")],
        };
        let sel = slots(&plain, &st);
        assert_eq!(sel.len(), 2, "{sel:?}");
        assert!(sel[0].contains("c22-14"), "keep-the-red first: {sel:?}");
        assert!(
            sel[1].contains("c22-15"),
            "then the only pon drawn: {sel:?}"
        );
    }

    /// And a pon that spends the red asks for 15 alone. Slot 14 is never it:
    /// when the client draws 14 at all, 14 is the plain pair.
    #[test]
    fn a_pon_that_spends_a_red_takes_the_pon_slot_alone() {
        let st = hand(&[0, 1]);
        let red = MjaiEvent::Pon {
            actor: 0,
            target: 1,
            pai: s("5m"),
            consumed: [s("5m"), s("5mr")],
        };
        let sel = slots(&red, &st);
        assert_eq!(sel.len(), 1, "{sel:?}");
        assert!(sel[0].contains("c22-15"), "{sel:?}");
    }

    /// Two kans on offer share three slots with nothing on the button to tell
    /// them apart, so the candidate list has to be rebuilt the way the client
    /// builds it: kakan (by meld order) first, then ankan by ascending tile
    /// class. Here the hand holds four 3m and four 7p — the 7p ankan is the
    /// client's second candidate, so slot 11.
    #[test]
    fn a_second_ankan_takes_the_second_kan_slot() {
        // 3m = class 2 → indices 8..11; 7p = class 15 → indices 60..63.
        let st = hand(&[8, 9, 10, 11, 60, 61, 62, 63]);
        let first = MjaiEvent::Ankan {
            actor: 0,
            consumed: [s("3m"), s("3m"), s("3m"), s("3m")],
        };
        assert!(slots(&first, &st)[0].contains("c22-10"));
        let second = MjaiEvent::Ankan {
            actor: 0,
            consumed: [s("7p"), s("7p"), s("7p"), s("7p")],
        };
        let sel = slots(&second, &st);
        assert_eq!(sel.len(), 1, "the candidate was placed exactly: {sel:?}");
        assert!(sel[0].contains("c22-11"), "{sel:?}");
    }

    /// A kakan comes before every ankan, whatever the tiles are — the client
    /// walks the melds first.
    #[test]
    fn a_kakan_outranks_an_ankan_for_the_first_slot() {
        use crate::bridge::tenhou::meld::{Meld, MeldKind};
        let mut st = hand(&[8, 9, 10, 11, 60]);
        st.melds = vec![Meld {
            kind: MeldKind::Pon,
            target_rel: 1,
            // 7p pon; the fourth copy (index 60) is in hand above.
            tiles: vec![61, 62, 63],
            unused: None,
            r: None,
        }];
        let kakan = MjaiEvent::Kakan {
            actor: 0,
            pai: s("7p"),
            consumed: [s("7p"), s("7p"), s("7p")],
        };
        assert!(slots(&kakan, &st)[0].contains("c22-10"), "kakan is first");
        let ankan = MjaiEvent::Ankan {
            actor: 0,
            consumed: [s("3m"), s("3m"), s("3m"), s("3m")],
        };
        assert!(
            slots(&ankan, &st)[0].contains("c22-11"),
            "the ankan follows it"
        );
    }

    /// A kan we cannot place falls back to the three slots in the order the
    /// client fills them, which is right whenever only one is on offer.
    #[test]
    fn an_unplaceable_kan_falls_back_to_the_family_in_fill_order() {
        let st = hand(&[0]);
        let ankan = MjaiEvent::Ankan {
            actor: 0,
            consumed: [s("3m"), s("3m"), s("3m"), s("3m")],
        };
        let sel = slots(&ankan, &st);
        assert_eq!(sel.len(), 3);
        assert!(
            sel[0].contains("c22-10") && sel[2].contains("c22-12"),
            "{sel:?}"
        );
    }

    #[test]
    fn hora_splits_tsumo_and_ron_buttons() {
        let st = hand(&[0]);
        let tsumo = MjaiEvent::Hora {
            actor: 1,
            target: 1,
            deltas: None,
            ura_markers: None,
        };
        assert!(slots(&tsumo, &st)[0].contains("c22-0"));
        let ron = MjaiEvent::Hora {
            actor: 1,
            target: 2,
            deltas: None,
            ura_markers: None,
        };
        assert!(slots(&ron, &st)[0].contains("c22-1"));
    }

    /// Kita shares its slots with the sanma menu's other declarations, and
    /// the client assigns the North first, so the family is tried in the
    /// order the client fills it rather than left to the document.
    #[test]
    fn kita_is_tried_in_the_order_the_client_fills_it() {
        let st = hand(&[0]);
        let kita = MjaiEvent::Kita {
            actor: 0,
            pai: Some(s("N")),
        };
        let sel = slots(&kita, &st);
        let want: Vec<String> = (5..=9).map(|n| format!("c22-{n}")).collect();
        for (got, expect) in sel.iter().zip(&want) {
            assert!(got.contains(expect.as_str()), "{sel:?}");
        }
    }

    /// In riichi the client offers an ankan only of the tile just drawn, and
    /// only when the wait is unchanged, so there is never a second candidate
    /// to place — and the hand's other concealed sets must not be counted.
    #[test]
    fn a_kan_declared_in_riichi_takes_the_first_slot() {
        let st = hand(&[8, 9, 10, 11, 60, 61, 62, 63]);
        let ankan = MjaiEvent::Ankan {
            actor: 0,
            consumed: [s("7p"), s("7p"), s("7p"), s("7p")],
        };
        let sel = button_for(&ankan, &st, true).unwrap().0;
        assert_eq!(sel.len(), 1);
        assert!(sel[0].contains("c22-10"), "{sel:?}");
    }

    /// Regression (a declared riichi that never discarded): Tenhou takes the
    /// declaration and the tile as two inputs and sits on its clock until it
    /// has both. Nothing prompts a second bot decision — the `reach` the
    /// server echoes back is not one — so the tile the bot named on the reach
    /// has to go out in the same plan.
    #[test]
    fn a_riichi_declares_and_discards_in_one_plan() {
        use crate::autoplay::platform::Step;
        let st = tenhou_state::TenhouState {
            seat: 0,
            // 0..=3 are the four copies of 1m; 4..=7 are 2m.
            hand: vec![0, 4, 8],
            melds: Vec::new(),
            is_tsumo: true,
            window: Some(tenhou_state::DecisionWindow {
                ops: tenhou_state::OP_REACH,
                opened_at: std::time::Instant::now(),
            }),
        };
        let action = MjaiEvent::Reach {
            actor: 0,
            pai: Some("2m".into()),
        };
        let cfg = crate::config::MajsoulAutoplayConfig::default();
        let snap = snapshot_fixture();
        let ctx = ActionContext {
            action: &action,
            snapshot: &snap,
            legal_actions: &[],
            our_seat: 0,
            last_kawa_tile: None,
            last_self_tsumo: None,
            self_riichi_accepted: false,
            num_players: 4,
            cfg: &cfg,
            delay_cfg: crate::config::DelayModelConfig::default(),
            budget: None,
            probs: None,
            reading: Default::default(),
            delay_script: None,
            tenhou: Some(&st),
        };
        let plan = TenhouAutoplay::new().plan(&ctx);

        let pressed = plan
            .steps
            .iter()
            .position(|s| matches!(s, Step::DomClick { label, .. } if label == "riichi"))
            .unwrap_or_else(|| panic!("riichi must be pressed: {:?}", plan.steps));
        let discarded = plan
            .steps
            .iter()
            .position(|s| matches!(s, Step::Discard { tile_index: 4 }))
            .unwrap_or_else(|| panic!("the named tile must follow: {:?}", plan.steps));
        assert!(
            pressed < discarded,
            "declare, then discard: {:?}",
            plan.steps
        );
    }

    /// Regression (blind riichi): a bot that declares riichi without naming
    /// the tile leaves a declaration Tenhou cannot complete — nothing prompts
    /// a follow-up decision, and at clock expiry the *client* finishes the
    /// riichi by throwing the drawn tile, committing the hand to a wait the
    /// bot never chose. The plan must not press anything at all.
    #[test]
    fn a_riichi_with_no_tile_named_is_not_declared() {
        let st = tenhou_state::TenhouState {
            seat: 0,
            hand: vec![0, 4, 8],
            melds: Vec::new(),
            is_tsumo: true,
            window: Some(tenhou_state::DecisionWindow {
                ops: tenhou_state::OP_REACH,
                opened_at: std::time::Instant::now(),
            }),
        };
        let action = MjaiEvent::Reach {
            actor: 0,
            pai: None,
        };
        let cfg = crate::config::MajsoulAutoplayConfig::default();
        let snap = snapshot_fixture();
        let ctx = ActionContext {
            action: &action,
            snapshot: &snap,
            legal_actions: &[],
            our_seat: 0,
            last_kawa_tile: None,
            last_self_tsumo: None,
            self_riichi_accepted: false,
            num_players: 4,
            cfg: &cfg,
            delay_cfg: crate::config::DelayModelConfig::default(),
            budget: None,
            probs: None,
            reading: Default::default(),
            delay_script: None,
            tenhou: Some(&st),
        };
        let plan = TenhouAutoplay::new().plan(&ctx);
        assert!(
            plan.steps.is_empty(),
            "a riichi that cannot be completed must not be started: {:?}",
            plan.steps
        );
    }

    /// The same refusal when the hand cannot produce the named tile: a
    /// declaration whose discard the client would have to resolve itself is
    /// worse than no declaration.
    #[test]
    fn a_riichi_whose_tile_is_not_held_is_not_declared() {
        let st = tenhou_state::TenhouState {
            seat: 0,
            hand: vec![0, 4, 8],
            melds: Vec::new(),
            is_tsumo: true,
            window: Some(tenhou_state::DecisionWindow {
                ops: tenhou_state::OP_REACH,
                opened_at: std::time::Instant::now(),
            }),
        };
        let action = MjaiEvent::Reach {
            actor: 0,
            pai: Some("9s".into()),
        };
        let cfg = crate::config::MajsoulAutoplayConfig::default();
        let snap = snapshot_fixture();
        let ctx = ActionContext {
            action: &action,
            snapshot: &snap,
            legal_actions: &[],
            our_seat: 0,
            last_kawa_tile: None,
            last_self_tsumo: None,
            self_riichi_accepted: false,
            num_players: 4,
            cfg: &cfg,
            delay_cfg: crate::config::DelayModelConfig::default(),
            budget: None,
            probs: None,
            reading: Default::default(),
            delay_script: None,
            tenhou: Some(&st),
        };
        let plan = TenhouAutoplay::new().plan(&ctx);
        assert!(plan.steps.is_empty(), "{:?}", plan.steps);
    }

    /// Regression: the plan's discard must name the drawn copy for a
    /// tsumogiri. The index is also the display — Tenhou shows a tsumogiri
    /// exactly when the thrown index is the drawn one — and the ordinary
    /// lookup prefers the higher index, which here is the *held* copy.
    #[test]
    fn a_tsumogiri_plan_discards_the_drawn_copy() {
        use crate::autoplay::platform::Step;
        let st = tenhou_state::TenhouState {
            seat: 0,
            // Two plain 5m: 19 held, 17 drawn last (the tail).
            hand: vec![19, 17],
            melds: Vec::new(),
            is_tsumo: true,
            window: Some(tenhou_state::DecisionWindow {
                ops: 0,
                opened_at: std::time::Instant::now(),
            }),
        };
        let action = MjaiEvent::Dahai {
            actor: 0,
            pai: "5m".into(),
            tsumogiri: true,
        };
        let cfg = crate::config::MajsoulAutoplayConfig::default();
        let snap = snapshot_fixture();
        let ctx = ActionContext {
            action: &action,
            snapshot: &snap,
            legal_actions: &[],
            our_seat: 0,
            last_kawa_tile: None,
            last_self_tsumo: None,
            self_riichi_accepted: false,
            num_players: 4,
            cfg: &cfg,
            delay_cfg: crate::config::DelayModelConfig::default(),
            budget: None,
            probs: None,
            reading: Default::default(),
            delay_script: None,
            tenhou: Some(&st),
        };
        let plan = TenhouAutoplay::new().plan(&ctx);
        assert!(
            plan.steps
                .iter()
                .any(|s| matches!(s, Step::Discard { tile_index: 17 })),
            "must throw the drawn copy: {:?}",
            plan.steps
        );
    }

    fn snapshot_fixture() -> crate::game_state::snapshot::GameStateSnapshot {
        crate::game_state::snapshot::GameStateSnapshot {
            num_players: 4,
            players: Vec::new(),
            bakaze: "E".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            current_player: 0,
            turn_count: 0,
            phase: crate::game_state::snapshot::Phase::WaitAct,
            is_done: false,
            dora_markers: Vec::new(),
            our_seat: Some(0),
        }
    }

    /// Every plan waits for the client before it thinks: the delay is meant
    /// to look like a human deciding, and a human cannot decide before the
    /// board has stopped moving.
    #[test]
    fn every_plan_waits_for_the_client_first() {
        use crate::autoplay::platform::Step;
        let st = tenhou_state::TenhouState {
            seat: 0,
            hand: vec![0, 4, 8],
            melds: Vec::new(),
            is_tsumo: true,
            window: Some(tenhou_state::DecisionWindow {
                ops: tenhou_state::OP_CHI,
                opened_at: std::time::Instant::now(),
            }),
        };
        let action = MjaiEvent::None;
        let cfg = crate::config::MajsoulAutoplayConfig::default();
        let snap = crate::game_state::snapshot::GameStateSnapshot {
            num_players: 4,
            players: Vec::new(),
            bakaze: "E".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya: 0,
            current_player: 0,
            turn_count: 0,
            phase: crate::game_state::snapshot::Phase::WaitAct,
            is_done: false,
            dora_markers: Vec::new(),
            our_seat: Some(0),
        };
        let ctx = ActionContext {
            action: &action,
            snapshot: &snap,
            legal_actions: &[],
            our_seat: 0,
            last_kawa_tile: None,
            last_self_tsumo: None,
            self_riichi_accepted: false,
            num_players: 4,
            cfg: &cfg,
            delay_cfg: crate::config::DelayModelConfig::default(),
            budget: None,
            probs: None,
            reading: Default::default(),
            delay_script: None,
            tenhou: Some(&st),
        };
        let plan = TenhouAutoplay::new().plan(&ctx);
        assert!(
            matches!(plan.steps.first(), Some(Step::AwaitReady { .. })),
            "readiness must come first, got {:?}",
            plan.steps
        );
    }

    /// A discard has no button — it is the one action that needs a position.
    #[test]
    fn discard_has_no_button() {
        let st = hand(&[0]);
        assert!(button_for(
            &MjaiEvent::Dahai {
                actor: 0,
                pai: s("1m"),
                tsumogiri: false
            },
            &st,
            false
        )
        .is_none());
    }

    #[test]
    fn simple_declarations_map_to_their_slots() {
        let st = hand(&[0]);
        for (action, slot) in [
            (
                MjaiEvent::Reach {
                    actor: 0,
                    pai: None,
                },
                "c22-2",
            ),
            (MjaiEvent::Ryukyoku { deltas: None }, "c22-3"),
            (MjaiEvent::None, "c22-4"),
            (
                MjaiEvent::Daiminkan {
                    actor: 0,
                    target: 1,
                    pai: s("1m"),
                    consumed: [s("1m"), s("1m"), s("1m")],
                },
                "c22-13",
            ),
        ] {
            let sel = slots(&action, &st);
            assert_eq!(sel.len(), 1, "{action:?} → {sel:?}");
            assert!(sel[0].contains(slot), "{action:?} → {sel:?}");
        }
    }
}

#[cfg(test)]
mod budget_tests {
    use crate::autoplay::tenhou_state::{DecisionWindow, TURN_BANK_MS, TURN_BASE_MS};
    use std::time::Instant;

    /// The allowance must leave room to act inside one Tenhou turn. The first
    /// live run ran to the no-budget ceiling and lost every decision to the
    /// client's auto-discard, so the base has to be well under that.
    #[test]
    fn turn_allowance_is_shorter_than_the_no_budget_ceiling() {
        let no_budget_ceiling = crate::config::DelayModelConfig::default().no_budget_cap_ms;
        assert!(
            TURN_BASE_MS < no_budget_ceiling,
            "a turn ({TURN_BASE_MS}ms) must be shorter than the uncapped ceiling \
             ({no_budget_ceiling}ms), or the model plans past the whole turn"
        );
        // The bank only ever extends the window, so it must not be the
        // thing that pushes the plan past the ceiling either.
        assert!(TURN_BASE_MS + TURN_BANK_MS >= no_budget_ceiling.min(15_000));
    }

    /// The window's own clock is what the budget is measured against.
    #[test]
    fn elapsed_advances_with_the_window() {
        let w = DecisionWindow {
            ops: 0,
            opened_at: Instant::now() - std::time::Duration::from_millis(1500),
        };
        assert!(w.elapsed_ms() >= 1500, "got {}", w.elapsed_ms());
    }
}
