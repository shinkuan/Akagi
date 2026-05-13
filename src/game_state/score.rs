//! Thin wrappers over `riichienv_core::score` and `HandEvaluator`.
//!
//! The intent is to give the rest of Akagi (and eventually IPC commands)
//! a stable surface that doesn't leak the upstream crate's exact types.
//! When riichienv tweaks its API in a future bump, this is the only
//! file that has to change.

use anyhow::{Context, Result};
use riichienv_core::hand_evaluator::HandEvaluator;
use riichienv_core::hand_evaluator_3p::HandEvaluator3P;
use riichienv_core::parser::tid_to_mjai;
pub use riichienv_core::score::Score;
use riichienv_core::state::GameState;
use riichienv_core::state_3p::GameState3P;
use riichienv_core::types::{Conditions, Wind};

use crate::schema::HoraScoreInfo;

/// Map a tile index in 34-space (0..34) to the mjai string. Unlike
/// `riichienv_core::parser::tid_to_mjai` (which works in 136-space and
/// turns the red-five sentinels 16/52/88 into `"5mr"`), this never
/// emits red-five suffixes — `get_waits_u8` returns 34-space indices
/// where reds aren't distinguished.
fn tid34_to_mjai(idx: u8) -> String {
    const HONORS: [&str; 7] = ["E", "S", "W", "N", "P", "F", "C"];
    if idx < 27 {
        let suit = ["m", "p", "s"][(idx / 9) as usize];
        format!("{}{}", (idx % 9) + 1, suit)
    } else {
        HONORS[(idx - 27) as usize].to_string()
    }
}

/// Calculate hand score for a given (han, fu, oya?, tsumo?, honba, num_players) tuple.
///
/// `num_players` is 3 for sanma or 4 for yonma. Affects honba payment splits
/// and (in 3p) tsumo payer count.
pub fn calculate_score(
    han: u8,
    fu: u8,
    is_oya: bool,
    is_tsumo: bool,
    honba: u32,
    num_players: u8,
) -> Score {
    riichienv_core::score::calculate_score(han, fu, is_oya, is_tsumo, honba, num_players)
}

/// Evaluate a 4-player ron/tsumo claim by `actor` against the live engine
/// state. Returns `None` when the actor's hand isn't a winning shape, when
/// the winning tile can't be inferred from `state` (no recent
/// discard / draw), or when `actor` is out of bounds.
///
/// The actor's hand is taken from `state.players[actor].hand` (136-space),
/// melds from `state.players[actor].melds`. The winning tile is
/// `state.last_discard.0` for ron and `state.drawn_tile` for tsumo. Dora
/// indicators come from `state.wall.dora_indicators`; ura dora is left
/// empty (only revealed after the platform accepts the win).
///
/// `Conditions` mirrors how `state/mod.rs` builds them on real wins:
/// haitei/houtei from `wall.drawable_count` + `is_rinshan_flag`,
/// rinshan from `is_rinshan_flag`, ippatsu from `players[actor].ippatsu_cycle`,
/// chankan from `pending_kan.is_some()` (ron only).
///
/// `tsumo_first_turn` (tenhou/chiihou) is derived from observable state
/// rather than `state.is_first_turn` — riichienv-core 0.4.8's mjai-event
/// handler initializes that flag to `true` on `start_kyoku` and never
/// resets it on `Dahai` (only the replay-mode `apply_log_action` does), so
/// trusting it would classify every menzen tsumo as
/// tenhou/chiihou yakuman.
pub fn evaluate_hora_4p(state: &GameState, actor: u8, is_tsumo: bool) -> Option<HoraScoreInfo> {
    let actor_idx = actor as usize;
    if actor_idx >= state.players.len() {
        return None;
    }
    let player = &state.players[actor_idx];

    let win_tile_136 = if is_tsumo {
        state.drawn_tile?
    } else {
        // last_discard is (actor, tile) — second slot is the tile.
        state.last_discard.map(|(_, t)| t)?
    };

    let evaluator = HandEvaluator::new(player.hand.clone(), player.melds.clone());

    let first_turn =
        is_tsumo && player.discards.is_empty() && state.players.iter().all(|p| p.melds.is_empty());

    let conditions = Conditions {
        tsumo: is_tsumo,
        riichi: player.riichi_declared,
        double_riichi: player.double_riichi_declared,
        ippatsu: player.ippatsu_cycle,
        haitei: is_tsumo && state.wall.drawable_count == 0 && !state.is_rinshan_flag,
        houtei: !is_tsumo && state.wall.drawable_count == 0 && !state.is_rinshan_flag,
        rinshan: is_tsumo && state.is_rinshan_flag,
        chankan: !is_tsumo && state.pending_kan.is_some(),
        tsumo_first_turn: first_turn,
        player_wind: Wind::from((actor + 4 - state.oya) % 4),
        round_wind: Wind::from(state.round_wind),
        riichi_sticks: state.riichi_sticks,
        honba: state.honba as u32,
        kita_count: 0,
        is_sanma: false,
        num_players: 4,
    };

    let result = evaluator.calc(
        win_tile_136,
        state.wall.dora_indicators.clone(),
        Vec::new(),
        Some(conditions),
    );
    if !result.is_win {
        return None;
    }
    let points = if is_tsumo {
        let oya_seat = state.oya;
        if actor == oya_seat {
            // Dealer tsumo: each of 3 ko pays `tsumo_agari_ko`.
            result.tsumo_agari_ko.saturating_mul(3)
        } else {
            // Non-dealer tsumo: 1 oya pay + 2 ko pays.
            result
                .tsumo_agari_oya
                .saturating_add(result.tsumo_agari_ko.saturating_mul(2))
        }
    } else {
        result.ron_agari
    };
    Some(HoraScoreInfo {
        points,
        han: result.han,
        fu: result.fu,
        yakuman: result.yakuman,
        win_tile: tid_to_mjai(win_tile_136),
    })
}

/// 3-player variant. Tsumo splits across 2 ko (or 2 ko for dealer in 3p).
/// Same `last_discard` / `is_first_turn` workarounds as the 4p variant —
/// see its doc comment for rationale.
///
/// Uses `HandEvaluator3P` (not the 4p `HandEvaluator`) so that
/// `conditions.kita_count` feeds into nukidora han, 3p-specific yaku rules
/// in `yaku_3p` apply, and the underlying `calculate_score` call runs with
/// `num_players = 3` — which makes ron honba pay the correct 200 per stick
/// (2 ko × 100) instead of the 4p figure of 300.
pub fn evaluate_hora_3p(state: &GameState3P, actor: u8, is_tsumo: bool) -> Option<HoraScoreInfo> {
    let actor_idx = actor as usize;
    if actor_idx >= state.players.len() {
        return None;
    }
    let player = &state.players[actor_idx];

    let win_tile_136 = if is_tsumo {
        state.drawn_tile?
    } else {
        // last_discard is (actor, tile) — second slot is the tile.
        state.last_discard.map(|(_, t)| t)?
    };

    let evaluator = HandEvaluator3P::new(player.hand.clone(), player.melds.clone());

    let first_turn =
        is_tsumo && player.discards.is_empty() && state.players.iter().all(|p| p.melds.is_empty());

    let conditions = Conditions {
        tsumo: is_tsumo,
        riichi: player.riichi_declared,
        double_riichi: player.double_riichi_declared,
        ippatsu: player.ippatsu_cycle,
        haitei: is_tsumo && state.wall.drawable_count == 0 && !state.is_rinshan_flag,
        houtei: !is_tsumo && state.wall.drawable_count == 0 && !state.is_rinshan_flag,
        rinshan: is_tsumo && state.is_rinshan_flag,
        chankan: !is_tsumo && state.pending_kan.is_some(),
        tsumo_first_turn: first_turn,
        player_wind: Wind::from((actor + 3 - state.oya) % 3),
        round_wind: Wind::from(state.round_wind),
        riichi_sticks: state.riichi_sticks,
        honba: state.honba as u32,
        kita_count: player.kita_tiles.len() as u8,
        is_sanma: true,
        num_players: 3,
    };

    let result = evaluator.calc(
        win_tile_136,
        state.wall.dora_indicators.clone(),
        Vec::new(),
        Some(conditions),
    );
    if !result.is_win {
        return None;
    }
    let points = if is_tsumo {
        let oya_seat = state.oya;
        if actor == oya_seat {
            // Dealer tsumo (3p): 2 ko pay.
            result.tsumo_agari_ko.saturating_mul(2)
        } else {
            // Non-dealer tsumo (3p): 1 oya pay + 1 ko pay.
            result.tsumo_agari_oya.saturating_add(result.tsumo_agari_ko)
        }
    } else {
        result.ron_agari
    };
    Some(HoraScoreInfo {
        points,
        han: result.han,
        fu: result.fu,
        yakuman: result.yakuman,
        win_tile: tid_to_mjai(win_tile_136),
    })
}

/// Tenpai check + waits for a hand string in `riichienv` MPSZ notation
/// (e.g. `"123m456p789s1122z"`, `"(p123m)456p789s1122z"` for melds).
///
/// Returns the wait list as mjai tile strings (`"1m"`, `"E"`, `"5pr"`),
/// or `Ok(vec![])` if the hand is not tenpai.
pub fn waits_for(hand_text: &str) -> Result<Vec<String>> {
    let evaluator = HandEvaluator::hand_from_text(hand_text)
        .with_context(|| format!("parse hand: {hand_text:?}"))?;
    if !evaluator.is_tenpai() {
        return Ok(Vec::new());
    }
    Ok(evaluator
        .get_waits_u8()
        .into_iter()
        .map(tid34_to_mjai)
        .collect())
}

/// Tenpai check only, when the wait list isn't needed.
pub fn is_tenpai(hand_text: &str) -> Result<bool> {
    let evaluator = HandEvaluator::hand_from_text(hand_text)
        .with_context(|| format!("parse hand: {hand_text:?}"))?;
    Ok(evaluator.is_tenpai())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a closed-hand GameState where seat `actor` is in chiitoitsu
    /// tenpai for `8s` (tile id 100, 34-space index 25). Used by the
    /// `evaluate_hora_4p_*` tests below — we set `drawn_tile` (tsumo) or
    /// `last_discard` (ron) to a second copy of 8s (id 101) after running
    /// `start_kyoku`-equivalent setup.
    ///
    /// Tile IDs are in 136-space; we pick the lowest copy of each tile to
    /// keep things deterministic and avoid red-five sentinels (16/52/88).
    /// Encoding cheat-sheet: `id / 4` gives the 34-space index — manzu 0-8
    /// (1m..9m), pinzu 9-17 (1p..9p), souzu 18-26 (1s..9s), honors 27-33.
    /// So 100/4=25 is 8s, not 7s.
    ///
    /// Pushes one fake non-actor discard so the `tsumo_first_turn`
    /// workaround in `evaluate_hora_4p` evaluates to `false` — these tests
    /// want to exercise mid-round chiitoitsu scoring, not tenhou/chiihou
    /// yakuman. (See `evaluate_hora_4p_tsumo_first_turn_*` for tests that
    /// actually exercise the first-turn path.)
    fn chiitoitsu_state(actor: u8, oya: u8) -> GameState {
        let rule = riichienv_core::rule::GameRule::default_tenhou();
        let mut s = GameState::new(0, true, None, 0, rule);
        s.oya = oya;
        s.round_wind = 0; // East
        s.honba = 0;
        s.riichi_sticks = 0;
        s.is_first_turn = false;

        // 11m 22m 33p 44p 66s 77s + tenpai-on-8s (13 tiles).
        let hand = vec![
            0, 1, // 1m 1m
            4, 5, // 2m 2m
            44, 45, // 3p 3p
            48, 49, // 4p 4p
            92, 93, // 6s 6s
            96, 97,  // 7s 7s
            100, // 8s (lone — chiitoitsu wait)
        ];
        s.players[actor as usize].hand = hand;

        // Push a sentinel discard from a different seat so the first-turn
        // workaround sees a non-empty river somewhere and skips
        // tenhou/chiihou.
        let other = (actor + 1) % 4;
        s.players[other as usize].discards.push(0); // 1m

        s
    }

    #[test]
    fn evaluate_hora_4p_tsumo_chiitoitsu_dealer() {
        // Dealer (oya=0) tsumo on 8s, chiitoitsu shape. Closed hand →
        // chiitoitsu (2 han) + menzen tsumo (1 han) at the very least.
        // Score must be a positive multiple of 100 with han ≥ 1.
        let mut s = chiitoitsu_state(0, 0);
        s.drawn_tile = Some(101); // 8s (second copy)
        s.players[0].hand.push(101);

        let info = evaluate_hora_4p(&s, 0, true).expect("winning shape");
        assert!(info.han >= 1, "han = {}", info.han);
        assert!(info.points > 0, "points = {}", info.points);
        assert!(info.points % 100 == 0, "points = {}", info.points);
    }

    #[test]
    fn evaluate_hora_4p_ron_chiitoitsu_non_dealer() {
        // Non-dealer (oya=1) ron on 8s discarded by seat 1. Chiitoitsu → 2 han.
        // riichienv stores last_discard as (actor, tile) — discarder first.
        let mut s = chiitoitsu_state(0, 1);
        s.last_discard = Some((1, 101)); // discarder = seat 1, tile = 8s

        let info = evaluate_hora_4p(&s, 0, false).expect("winning shape");
        assert!(info.han >= 2, "han = {}", info.han);
        assert!(info.fu >= 20, "fu = {}", info.fu);
        assert!(info.points > 0, "points = {}", info.points);
        assert_eq!(info.win_tile, "8s", "win tile must come from tile slot");
    }

    /// Regression: `evaluate_hora_4p` previously read `last_discard.0` as
    /// the win tile, but riichienv stores it as `(actor, tile)`. With seat
    /// 1 discarding 8s, the bug extracted seat=1 → win_tile_34=0 → "1m",
    /// the hand-evaluator failed to find a winning shape, and the function
    /// returned `None`. The frontend then showed no score for ron actions.
    #[test]
    fn evaluate_hora_4p_ron_uses_tile_not_discarder_seat() {
        let mut s = chiitoitsu_state(0, 1);
        // Discarder seat (1) and the tile (8s = 101) deliberately differ.
        // If we ever regress to reading the actor slot, win_tile_34 would
        // be 0 (1m) and we'd get None back.
        s.last_discard = Some((1, 101));
        let info = evaluate_hora_4p(&s, 0, false)
            .expect("winning shape — tile slot must be read for the ron tile");
        assert_eq!(info.win_tile, "8s");
    }

    /// Regression: every menzen tsumo used to score as tenhou/chiihou
    /// yakuman (32000 non-dealer / 48000 dealer) because riichienv-core
    /// 0.4.8's `apply_mjai_event` initializes `is_first_turn = true` on
    /// `start_kyoku` and never flips it back to `false` (only the
    /// replay-mode `apply_log_action` does). We now derive the first-turn
    /// condition from `players[].discards` + `players[].melds` instead of
    /// trusting the stuck flag.
    #[test]
    fn evaluate_hora_4p_tsumo_not_first_turn_when_actor_has_discarded() {
        // Non-dealer (seat 0, oya=1) chiitoitsu tsumo on 7s. The actor's
        // own river is non-empty, so it can't possibly be chiihou.
        let mut s = chiitoitsu_state(0, 1);
        s.players[0].discards.push(0); // actor has already discarded
        s.is_first_turn = true; // simulate the riichienv flag being stuck
        s.drawn_tile = Some(101);
        s.players[0].hand.push(101);

        let info = evaluate_hora_4p(&s, 0, true).expect("winning shape");
        assert!(!info.yakuman, "must not score as chiihou yakuman");
        assert!(
            info.points < 32_000,
            "non-dealer mid-round chiitoitsu tsumo cannot reach 32000 — got {}",
            info.points,
        );
    }

    /// Tenhou/chiihou should still apply when the state genuinely shows a
    /// first-turn tsumo: nobody has discarded or called yet.
    #[test]
    fn evaluate_hora_4p_tsumo_first_turn_triggers_chiihou() {
        // chiitoitsu_state pushes a sentinel discard for the workaround;
        // strip every river so the workaround sees a true first turn.
        let mut s = chiitoitsu_state(0, 1);
        for p in &mut s.players {
            p.discards.clear();
        }
        s.drawn_tile = Some(101);
        s.players[0].hand.push(101);

        let info = evaluate_hora_4p(&s, 0, true).expect("winning shape");
        assert!(info.yakuman, "non-dealer first-turn tsumo is chiihou");
        // Chiihou (single yakuman) non-dealer = 16000 + 8000*2 = 32000.
        assert_eq!(info.points, 32_000);
    }

    /// Any open meld anywhere on the table breaks first-turn yakuman.
    #[test]
    fn evaluate_hora_4p_tsumo_first_turn_blocked_by_any_meld() {
        use riichienv_core::types::{Meld, MeldType};
        let mut s = chiitoitsu_state(0, 1);
        for p in &mut s.players {
            p.discards.clear();
        }
        // A pon by some other seat — calls have happened, so chiihou is off.
        s.players[2].melds.push(Meld {
            meld_type: MeldType::Pon,
            tiles: vec![0, 1, 2],
            opened: true,
            from_who: -1,
            called_tile: Some(0),
        });
        s.drawn_tile = Some(101);
        s.players[0].hand.push(101);

        let info = evaluate_hora_4p(&s, 0, true).expect("winning shape");
        assert!(!info.yakuman, "calls present → no chiihou");
    }

    #[test]
    fn evaluate_hora_4p_returns_none_when_not_a_win() {
        // Default GameState has empty hands → not a winning shape.
        let rule = riichienv_core::rule::GameRule::default_tenhou();
        let mut s = GameState::new(0, true, None, 0, rule);
        s.drawn_tile = Some(0);
        assert!(evaluate_hora_4p(&s, 0, true).is_none());
    }

    #[test]
    fn evaluate_hora_4p_returns_none_when_no_win_tile() {
        // Winning shape in hand but `drawn_tile` is None and `last_discard`
        // is None — we can't infer the winning tile.
        let s = chiitoitsu_state(0, 0);
        assert!(evaluate_hora_4p(&s, 0, true).is_none());
        assert!(evaluate_hora_4p(&s, 0, false).is_none());
    }

    #[test]
    fn evaluate_hora_4p_returns_none_for_oob_actor() {
        let mut s = chiitoitsu_state(0, 0);
        s.drawn_tile = Some(101);
        assert!(evaluate_hora_4p(&s, 99, true).is_none());
    }

    #[test]
    fn ron_3han_30fu_non_dealer_no_honba() {
        let s = calculate_score(3, 30, false, false, 0, 4);
        // 30 fu × 2^(2+3) = 960 base; non-dealer ron = ceil_100(960*4) = 3900.
        assert_eq!(s.total, 3900);
        assert_eq!(s.pay_ron, 3900);
    }

    #[test]
    fn mangan_dealer_tsumo() {
        // 5 han = mangan; dealer tsumo = 4000 from each ko = 12000.
        let s = calculate_score(5, 30, true, true, 0, 4);
        assert_eq!(s.total, 12_000);
        assert_eq!(s.pay_tsumo_ko, 4_000);
    }

    #[test]
    fn honba_adds_300_to_ron_4p() {
        let base = calculate_score(2, 30, false, false, 0, 4);
        let with_honba = calculate_score(2, 30, false, false, 1, 4);
        // 4p ron honba = 300 added to ron pay + total.
        assert_eq!(with_honba.total, base.total + 300);
        assert_eq!(with_honba.pay_ron, base.pay_ron + 300);
    }

    #[test]
    fn waits_finds_chiitoitsu_tanki() {
        // 6 pairs + a lone 7m → chiitoitsu tenpai on 7m. Standard
        // interpretations also yield 1m and 4m via overlapping runs
        // (11+234+234+567+56 waits on 4m, etc.), so assert membership
        // rather than exact equality.
        let waits = waits_for("1122334455667m").unwrap();
        assert!(waits.contains(&"7m".into()), "got {waits:?}");
    }

    #[test]
    fn waits_returns_empty_for_non_tenpai() {
        // Random shanten ≥ 1 hand.
        let waits = waits_for("123456789m12345p").unwrap();
        assert!(waits.is_empty());
    }

    #[test]
    fn is_tenpai_consistent_with_waits() {
        let hand = "1122334455667m";
        assert!(is_tenpai(hand).unwrap());
        assert!(!waits_for(hand).unwrap().is_empty());
    }

    /// Regression for the Tenhou hora-points mismatch (issue: dealer
    /// riichi+ippatsu+dora+aka was scored as 8300 instead of 12600). Drives
    /// the live `GameTracker` (replay path through `apply_mjai_event`) to
    /// reproduce the exact state at bot-decision time, then evaluates the
    /// hora score. Pre-fix the test fails with `han=3, points=8300` because
    /// riichienv-core 0.4.8's `apply_mjai_event(ReachAccepted)` doesn't set
    /// `ippatsu_cycle = true`; the tracker now patches that lifecycle.
    #[test]
    fn evaluate_hora_after_replay_counts_ippatsu_for_dealer_ron() {
        use crate::game_state::tracker::GameTracker;
        use crate::schema::MjaiEvent as E;

        let mut t = GameTracker::new();
        t.handle(&E::StartGame {
            names: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            kyoku_first: None,
            aka_flag: None,
            id: Some(0),
            num_players: 4,
        })
        .unwrap();
        t.handle(&E::StartKyoku {
            bakaze: "E".into(),
            dora_marker: "9p".into(),
            kyoku: 1,
            honba: 2,
            kyotaku: 0,
            oya: 0,
            scores: vec![36_000, 28_000, 28_000, 28_000],
            tehais: vec![
                ["C", "6p", "4m", "3p", "8s", "3m", "2p", "1m", "N", "P", "9p", "W", "7p"]
                    .iter()
                    .map(|s| (*s).into())
                    .collect(),
                vec!["?".into(); 13],
                vec!["?".into(); 13],
                vec!["?".into(); 13],
            ],
            num_players: 4,
        })
        .unwrap();

        // Each row: (actor, drawn_tile, dahai_tile). Tsumogiri inferred from
        // equality; non-actor draws use "?" since the bridge feeds the
        // observer perspective.
        let turns: &[(u8, &str, &str)] = &[
            (0, "C", "W"),     (1, "?", "6s"),  (2, "?", "2s"),  (3, "?", "7m"),
            (0, "E", "N"),     (1, "?", "8p"),  (2, "?", "P"),   (3, "?", "6s"),
            (0, "3p", "1m"),   (1, "?", "3s"),  (2, "?", "3m"),  (3, "?", "6s"),
            (0, "8s", "9p"),   (1, "?", "3m"),  (2, "?", "S"),   (3, "?", "2p"),
            (0, "5sr", "P"),   (1, "?", "E"),   (2, "?", "4s"),  (3, "?", "W"),
            (0, "1s", "1s"),   (1, "?", "4p"),  (2, "?", "2s"),  (3, "?", "7m"),
            (0, "7m", "7m"),   (1, "?", "8s"),  (2, "?", "2m"),  (3, "?", "3p"),
            (0, "1p", "3p"),   (1, "?", "9m"),  (2, "?", "4s"),  (3, "?", "3s"),
            (0, "5p", "E"),    (1, "?", "1m"),  (2, "?", "S"),   (3, "?", "F"),
            (0, "7s", "5sr"),  (1, "?", "9p"),  (2, "?", "8m"),  (3, "?", "5m"),
        ];
        for &(actor, draw, dahai) in turns {
            t.handle(&E::Tsumo { actor, pai: draw.into() }).unwrap();
            t.handle(&E::Dahai {
                actor,
                pai: dahai.into(),
                tsumogiri: draw == dahai,
            })
            .unwrap();
        }

        // Reach + reach-discard 7s + reach_accepted for actor 0.
        t.handle(&E::Tsumo { actor: 0, pai: "8s".into() }).unwrap();
        t.handle(&E::Reach { actor: 0, pai: None }).unwrap();
        t.handle(&E::Dahai {
            actor: 0,
            pai: "7s".into(),
            tsumogiri: false,
        })
        .unwrap();
        t.handle(&E::ReachAccepted { actor: 0 }).unwrap();

        // One full go-around inside the ippatsu window: actor 1 then actor 2
        // discards 5mr (the ron tile).
        t.handle(&E::Tsumo { actor: 1, pai: "?".into() }).unwrap();
        t.handle(&E::Dahai {
            actor: 1,
            pai: "S".into(),
            tsumogiri: false,
        })
        .unwrap();
        t.handle(&E::Tsumo { actor: 2, pai: "?".into() }).unwrap();
        t.handle(&E::Dahai {
            actor: 2,
            pai: "5mr".into(),
            tsumogiri: false,
        })
        .unwrap();

        let info = t.evaluate_hora(0, false).expect("winning shape");
        // Pre-fix: (han=3, fu=40, points=8300) — ippatsu missing on replay.
        // Post-fix: dealer mangan 12000 + 2*300 honba = 12600.
        assert_eq!(
            (info.han, info.fu, info.points, info.win_tile.as_str()),
            (4, 40, 12_600, "5mr"),
            "riichi+ippatsu+dora+aka dealer ron must score 12600 with honba=2",
        );
    }

    #[test]
    fn tid34_covers_all_suits_and_honors() {
        assert_eq!(tid34_to_mjai(0), "1m");
        assert_eq!(tid34_to_mjai(8), "9m");
        assert_eq!(tid34_to_mjai(9), "1p");
        assert_eq!(tid34_to_mjai(17), "9p");
        assert_eq!(tid34_to_mjai(18), "1s");
        assert_eq!(tid34_to_mjai(26), "9s");
        assert_eq!(tid34_to_mjai(27), "E");
        assert_eq!(tid34_to_mjai(33), "C");
    }

    /// 3p analogue of `chiitoitsu_state`. Seeded wall + cleared
    /// dora_indicators keep `dora_count` at 0 across calls, so han depends
    /// only on the explicit hand / win-tile shape we configure — no
    /// flake from random dora landing on a hand tile.
    ///
    /// Hand is mixed suits + honor to avoid incidental chinitsu/honitsu
    /// han adding to the chiitoitsu base. All tiles are valid in the 3p
    /// wall (3p ships only 1m/9m of manzu, so this hand keeps to pinzu /
    /// souzu / honors).
    fn chiitoitsu_state_3p(actor: u8, oya: u8) -> GameState3P {
        let rule = riichienv_core::rule::GameRule::default_tenhou();
        let mut s = GameState3P::new(0, true, Some(42), 0, rule);
        s.oya = oya;
        s.round_wind = 0; // East
        s.honba = 0;
        s.riichi_sticks = 0;
        s.is_first_turn = false;
        s.wall.dora_indicators.clear();

        // 1p 1p 2p 2p 3p 3p 4p 4p 1s 1s S S + 8p (lone — chiitoitsu wait).
        // South pair (not round wind, not the actor's seat wind for any
        // oya choice in 3p with E round) avoids yakuhai. 1s and 1p
        // terminals block tanyao; mixed suits block honitsu/chinitsu.
        // 8p second copy (id 65) is the win tile.
        let hand = vec![
            36, 37, // 1p 1p
            40, 41, // 2p 2p
            44, 45, // 3p 3p
            48, 49, // 4p 4p
            72, 73, // 1s 1s
            112, 113, // S S (28*4=112)
            64, // 8p (lone — chiitoitsu wait)
        ];
        s.players[actor as usize].hand = hand;

        // Non-actor sentinel discard so first-turn workaround sees a
        // non-empty river and skips tenhou/chiihou (matches the 4p
        // helper's reasoning).
        let other = (actor + 1) % 3;
        s.players[other as usize].discards.push(0);

        s
    }

    /// Regression: 3p ron honba pays 200 per stick (2 ko × 100), not 4p's
    /// 300. `evaluate_hora_3p` previously routed through the 4p
    /// `HandEvaluator`, whose `calc()` calls `calculate_score(..., 4)` —
    /// so `honba_ron = honba * 100 * (4 - 1) = 300`. Switching to
    /// `HandEvaluator3P` fixes this at the source by passing `3`.
    #[test]
    fn evaluate_hora_3p_ron_honba_is_200_per_stick() {
        let mut s0 = chiitoitsu_state_3p(0, 1); // non-dealer
        s0.last_discard = Some((2, 65)); // seat 2 discards 2nd 8p
        let mut s1 = chiitoitsu_state_3p(0, 1);
        s1.honba = 1;
        s1.last_discard = Some((2, 65));

        let no_honba = evaluate_hora_3p(&s0, 0, false).expect("winning shape");
        let with_honba = evaluate_hora_3p(&s1, 0, false).expect("winning shape");

        // Chiitoitsu (2 han) 25 fu non-dealer ron base:
        //   base = 25 * 2^(2+2) = 400; ron = ceil_100(400 * 4) = 1600.
        assert_eq!(no_honba.points, 1600, "chiitoitsu non-dealer ron base");
        // 3p honba on ron = 100 * (3 - 1) = 200, not 4p's 300.
        assert_eq!(with_honba.points, 1800, "should be 1600 + 200");
        assert_eq!(with_honba.points - no_honba.points, 200);
    }

    /// Regression: kita han now flows into the score. The 4p
    /// `HandEvaluator` never reads `conditions.kita_count` — its
    /// `YakuContext` has no `nukidora_count` field — so every nukidora
    /// silently dropped to 0 han at scoring time, and a winning hand with
    /// 2 kita scored as if it had 0. `HandEvaluator3P` adds nukidora via
    /// `yaku_3p::calculate_yaku_3p`.
    #[test]
    fn evaluate_hora_3p_kita_han_counts_in_score() {
        let mut s = chiitoitsu_state_3p(0, 1); // non-dealer

        // 2 kita already declared by this seat. Values aren't read by the
        // evaluator (only `kita_tiles.len()` feeds `kita_count`), but pick
        // valid North tile ids for realism.
        s.players[0].kita_tiles = vec![120, 121];

        // Tsumo on 8p (second copy). Mirror the 4p test pattern by also
        // pushing the drawn tile into hand — the evaluator handles a
        // 14-tile hand by skipping the internal `hand_14.add(win_tile)`.
        s.drawn_tile = Some(65);
        s.players[0].hand.push(65);

        // Own discard so the first-turn workaround doesn't fire chiihou.
        s.players[0].discards.push(0);

        let info = evaluate_hora_3p(&s, 0, true).expect("winning shape");

        // chiitoitsu (2) + menzen tsumo (1) + 2 nukidora = 5 han = mangan.
        assert_eq!(info.han, 5, "chiitoitsu+tsumo+2 nukidora = 5 han");

        // 3p non-dealer mangan tsumo: pay_oya 4000 + pay_ko 2000 = 6000.
        assert_eq!(info.points, 6_000);
    }
}
