//! Event-stream → `GameRecord` aggregator.
//!
//! Ports `reference/Mortal/libriichi/src/stat.rs::from_game` to operate on
//! `crate::schema::MjaiEvent` directly and produce a `GameRecord`. Walks
//! the event list once, accumulating per-seat scores via `deltas` (with
//! the same conventions: riichi cost paid via `ReachAccepted`; `Hora` /
//! `Ryukyoku` deltas cover all other kyotaku) plus per-game stat counters
//! from the recorded player's perspective.
//!
//! At end-of-stream the running scores are normalised to a fixed sum
//! (100k for 4p, 105k for 3p) by topping up the rank-1 seat — the same
//! trick `stat.rs:417-430` uses to backfill kyotaku that the bridge
//! didn't carry into the final round.
//!
//! The aggregator is platform-neutral: pass the `Platform` tag in
//! explicitly. It tolerates missing optional fields (e.g. observer-mode
//! logs without `start_game.id`).

use chrono::{DateTime, Utc};
use tracing::warn;

use crate::schema::{
    GameRecord, GameStats, HistoryEventLog, KyokuMode, MjaiEvent, Platform,
};

/// 4p starting score / kyotaku-normalisation target / sanma equivalents.
const STARTING_SCORE_4P: i32 = 25_000;
const STARTING_SCORE_3P: i32 = 35_000;
const TOTAL_SCORE_4P: i32 = 100_000;
const TOTAL_SCORE_3P: i32 = 105_000;

/// Inputs not derivable from the event stream.
pub struct AggregateInput<'a> {
    pub events: &'a HistoryEventLog,
    pub platform: Platform,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    /// ULID stem; used for both `record.id` and `log_path`.
    pub id: String,
}

/// Aggregate one game's events into a `GameRecord`. Returns `None` when
/// the stream lacks a leading `start_game` (so we can't tell 3p vs 4p
/// or who's playing) — caller should drop and warn.
pub fn aggregate(input: AggregateInput<'_>) -> Option<GameRecord> {
    let AggregateInput {
        events,
        platform,
        started_at,
        ended_at,
        id,
    } = input;

    // ----- Bootstrap: pull num_players + names + our_seat from start_game -----
    let (num_players, names, our_seat) = events.iter().find_map(|ev| match ev {
        MjaiEvent::StartGame {
            names,
            id,
            num_players,
            ..
        } => Some((*num_players, names.clone(), *id)),
        _ => None,
    })?;

    let n = num_players as usize;
    if names.len() != n {
        warn!(
            target: "akagi::history",
            "start_game names.len()={} but num_players={}; padding/truncating",
            names.len(),
            num_players
        );
    }

    // ----- Walk events: per-seat running score + per-game stats (our perspective) -----
    let mut cur_scores = vec![0i32; n];
    let mut stats = GameStats::default();

    let mut kyoku_mode = KyokuMode::EastOnly;

    // Per-kyoku transient state. Only meaningful when our_seat is Some.
    let mut riichi_declared = false;
    let mut riichi_accepted = false;
    let mut others_riichi_declared = false;
    let mut cur_oya: u8 = 0;
    let mut jun: i64 = 0;
    let mut fuuro_num: i64 = 0;

    for ev in events {
        match ev {
            MjaiEvent::StartKyoku {
                bakaze,
                oya,
                scores,
                ..
            } => {
                stats.round += 1;

                // Track game length from the highest-bakaze round seen.
                kyoku_mode = match (kyoku_mode, bakaze.as_str()) {
                    (KyokuMode::Other, _) => KyokuMode::Other,
                    (_, "W" | "N") => KyokuMode::Other,
                    (_, "S") => KyokuMode::EastSouth,
                    _ => kyoku_mode,
                };

                // Snap running scores to the round-opening official scores.
                // The bridge doesn't always carry kyotaku into the deltas
                // (Mortal mirrors this) — we'll renormalise at game end.
                if scores.len() == n {
                    cur_scores.copy_from_slice(scores);
                }

                cur_oya = *oya;
                if let Some(me) = our_seat {
                    if cur_oya == me {
                        stats.oya += 1;
                    }
                }
                riichi_declared = false;
                riichi_accepted = false;
                others_riichi_declared = false;
                jun = 0;
                fuuro_num = 0;
            }

            MjaiEvent::Dahai { actor, .. } => {
                if let Some(me) = our_seat {
                    if *actor == me {
                        jun += 1;
                    }
                }
            }

            MjaiEvent::Chi { actor, .. }
            | MjaiEvent::Pon { actor, .. }
            | MjaiEvent::Daiminkan { actor, .. } => {
                if let Some(me) = our_seat {
                    if *actor == me {
                        fuuro_num += 1;
                    }
                }
            }

            MjaiEvent::Reach { actor, .. } => {
                if let Some(me) = our_seat {
                    if *actor == me {
                        riichi_declared = true;
                        stats.riichi += 1;
                        stats.riichi_jun += jun;
                        if cur_oya == me {
                            stats.riichi_as_oya += 1;
                        }
                        if others_riichi_declared {
                            stats.chasing_riichi += 1;
                        }
                    } else if riichi_declared {
                        stats.riichi_got_chased += 1;
                    } else {
                        others_riichi_declared = true;
                    }
                }
            }

            MjaiEvent::ReachAccepted { actor } => {
                let i = *actor as usize;
                if i < cur_scores.len() {
                    cur_scores[i] -= 1000;
                }
                if let Some(me) = our_seat {
                    if *actor == me {
                        riichi_accepted = true;
                    }
                }
            }

            MjaiEvent::Hora {
                actor,
                target,
                deltas,
                ..
            } => {
                let Some(deltas) = deltas else {
                    continue;
                };
                add_deltas(&mut cur_scores, deltas);

                let Some(me) = our_seat else { continue };

                if *actor == me {
                    let point = i64::from(deltas.get(me as usize).copied().unwrap_or(0))
                        - i64::from(riichi_accepted) * 1000;
                    stats.agari += 1;
                    stats.agari_jun += jun;
                    if cur_oya == me {
                        stats.agari_as_oya += 1;
                        stats.agari_point_oya += point;
                    } else {
                        stats.agari_point_ko += point;
                    }

                    if riichi_accepted {
                        stats.riichi_agari += 1;
                        stats.riichi_agari_jun += jun;
                        stats.riichi_agari_point += point;
                        stats.riichi_point += point;
                    } else if fuuro_num > 0 {
                        stats.fuuro_agari += 1;
                        stats.fuuro_agari_jun += jun;
                        stats.fuuro_agari_point += point;
                        stats.fuuro_point += point;
                    } else {
                        stats.dama_agari += 1;
                        stats.dama_agari_jun += jun;
                        stats.dama_agari_point += point;
                    }

                    // Yakuman threshold mirrors libriichi: dealer ron 48000,
                    // non-dealer ron 32000. We approximate via the received
                    // amount only — close enough at this granularity.
                    let yakuman_threshold = if cur_oya == me { 48000 } else { 32000 };
                    if point >= yakuman_threshold {
                        stats.yakuman += 1;
                    }
                } else if *target == me {
                    let point = i64::from(deltas.get(me as usize).copied().unwrap_or(0));
                    stats.houjuu += 1;
                    stats.houjuu_jun += jun;
                    if cur_oya == *actor {
                        stats.houjuu_to_oya += 1;
                        stats.houjuu_point_to_oya += point;
                    } else {
                        stats.houjuu_point_to_ko += point;
                    }

                    if riichi_declared {
                        stats.riichi_houjuu += 1;
                        stats.riichi_point += point;
                    } else if fuuro_num > 0 {
                        stats.fuuro_houjuu += 1;
                        stats.fuuro_point += point;
                    }
                }
            }

            MjaiEvent::Ryukyoku { deltas } => {
                let Some(deltas) = deltas else {
                    continue;
                };
                add_deltas(&mut cur_scores, deltas);

                let Some(me) = our_seat else { continue };

                let point = i64::from(deltas.get(me as usize).copied().unwrap_or(0));
                stats.ryukyoku += 1;
                stats.ryukyoku_point += point;
                if riichi_accepted {
                    stats.riichi_ryukyoku += 1;
                    stats.riichi_point += point - 1000;
                } else if fuuro_num > 0 {
                    stats.fuuro_point += point;
                }

                if point >= 8000 {
                    stats.nagashi_mangan += 1;
                }
            }

            MjaiEvent::EndKyoku => {
                if let Some(_me) = our_seat {
                    if fuuro_num > 0 {
                        stats.fuuro += 1;
                        stats.fuuro_num += fuuro_num;
                    }
                }
            }

            _ => {}
        }
    }

    // ----- Final score normalisation (Mortal trick) -----
    let total = if num_players == 3 {
        TOTAL_SCORE_3P
    } else {
        TOTAL_SCORE_4P
    };
    let starting = if num_players == 3 {
        STARTING_SCORE_3P
    } else {
        STARTING_SCORE_4P
    };
    let sum: i32 = cur_scores.iter().sum();
    if sum < total {
        // Top up rank-1 (highest-score, then lowest-seat tiebreak).
        let top_seat = (0..n)
            .max_by(|&a, &b| {
                cur_scores[a]
                    .cmp(&cur_scores[b])
                    .then_with(|| b.cmp(&a)) // lower seat wins tie
            })
            .unwrap_or(0);
        cur_scores[top_seat] += total - sum;
    }

    let final_ranks = ranks_from_scores(&cur_scores);
    let our_rank = our_seat.map(|seat| final_ranks[seat as usize]);
    let our_delta = our_seat.map(|seat| cur_scores[seat as usize] - starting);

    Some(GameRecord {
        id: id.clone(),
        started_at,
        ended_at,
        platform,
        num_players,
        kyoku_mode,
        names: pad_or_truncate(names, n),
        our_seat,
        final_scores: cur_scores,
        final_ranks,
        our_rank,
        our_delta,
        stats,
        log_path: format!("games/{id}.mjai.jsonl"),
    })
}

fn add_deltas(scores: &mut [i32], deltas: &[i32]) {
    for (s, &d) in scores.iter_mut().zip(deltas.iter()) {
        *s += d;
    }
}

fn pad_or_truncate(mut v: Vec<String>, n: usize) -> Vec<String> {
    use std::cmp::Ordering;
    match v.len().cmp(&n) {
        Ordering::Less => {
            v.resize(n, String::new());
            v
        }
        Ordering::Greater => {
            v.truncate(n);
            v
        }
        Ordering::Equal => v,
    }
}

/// Rank vector: descending score, ascending-seat tiebreak. Output is
/// 1-indexed (rank 1 = winner) so the JSON shape matches user
/// expectations (rather than libriichi's 0-indexed internal use).
fn ranks_from_scores(scores: &[i32]) -> Vec<u8> {
    let n = scores.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| scores[b].cmp(&scores[a]).then_with(|| a.cmp(&b)));
    let mut out = vec![0u8; n];
    for (rank0, seat) in order.into_iter().enumerate() {
        out[seat] = (rank0 + 1) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn start_game_4p(seat: u8) -> MjaiEvent {
        MjaiEvent::StartGame {
            names: vec!["A".into(), "B".into(), "C".into(), "D".into()],
            kyoku_first: Some(0),
            aka_flag: Some(true),
            id: Some(seat),
            num_players: 4,
        }
    }

    fn start_kyoku(bakaze: &str, oya: u8, scores: Vec<i32>) -> MjaiEvent {
        MjaiEvent::StartKyoku {
            bakaze: bakaze.into(),
            dora_marker: "1m".into(),
            kyoku: 1,
            honba: 0,
            kyotaku: 0,
            oya,
            scores,
            tehais: vec![vec![]; 4],
            num_players: 4,
        }
    }

    #[test]
    fn aggregates_simple_4p_tonpuu_with_one_hora() {
        let events = vec![
            start_game_4p(0),
            start_kyoku("E", 0, vec![25000, 25000, 25000, 25000]),
            MjaiEvent::Dahai {
                actor: 0,
                pai: "1m".into(),
                tsumogiri: false,
            },
            MjaiEvent::Hora {
                actor: 0,
                target: 1,
                deltas: Some(vec![8000, -8000, 0, 0]),
                ura_markers: None,
            },
            MjaiEvent::EndKyoku,
            MjaiEvent::EndGame,
        ];
        let rec = aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "TEST1".into(),
        })
        .unwrap();

        assert_eq!(rec.num_players, 4);
        assert_eq!(rec.our_seat, Some(0));
        // Pre-normalisation totals to 25000+25000+25000+25000 + delta sum 0 = 100k → no fixup.
        assert_eq!(rec.final_scores, vec![33000, 17000, 25000, 25000]);
        assert_eq!(rec.final_ranks, vec![1, 4, 2, 3]); // seat 0 highest, then 2,3 tied (seat 2 wins tiebreak), then seat 1
        assert_eq!(rec.our_rank, Some(1));
        assert_eq!(rec.our_delta, Some(8000));
        assert_eq!(rec.stats.agari, 1);
        assert_eq!(rec.stats.agari_as_oya, 1);
        assert_eq!(rec.stats.dama_agari, 1);
        assert_eq!(rec.stats.round, 1);
        assert_eq!(rec.stats.oya, 1);
        assert_eq!(rec.kyoku_mode, KyokuMode::EastOnly);
    }

    #[test]
    fn aggregates_houjuu_after_riichi() {
        // Seat 2 is "us"; we riichi, then deal in to seat 0.
        let events = vec![
            start_game_4p(2),
            start_kyoku("E", 0, vec![25000, 25000, 25000, 25000]),
            MjaiEvent::Reach { actor: 2, pai: None },
            MjaiEvent::ReachAccepted { actor: 2 },
            MjaiEvent::Dahai {
                actor: 2,
                pai: "1m".into(),
                tsumogiri: false,
            },
            MjaiEvent::Hora {
                actor: 0,
                target: 2,
                deltas: Some(vec![8000, 0, -8000, 0]),
                ura_markers: None,
            },
            MjaiEvent::EndKyoku,
            MjaiEvent::EndGame,
        ];
        let rec = aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "TEST2".into(),
        })
        .unwrap();
        assert_eq!(rec.our_seat, Some(2));
        assert_eq!(rec.stats.riichi, 1);
        assert_eq!(rec.stats.riichi_houjuu, 1);
        assert_eq!(rec.stats.houjuu, 1);
        assert_eq!(rec.stats.houjuu_to_oya, 1);
        // We lost 8000 + 1000 riichi stick = -9000 from 25000 = 16000.
        assert_eq!(rec.final_scores[2], 16000);
    }

    #[test]
    fn detects_east_south_from_bakaze() {
        let events = vec![
            start_game_4p(0),
            start_kyoku("E", 0, vec![25000, 25000, 25000, 25000]),
            start_kyoku("S", 0, vec![25000, 25000, 25000, 25000]),
            MjaiEvent::EndGame,
        ];
        let rec = aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "TEST3".into(),
        })
        .unwrap();
        assert_eq!(rec.kyoku_mode, KyokuMode::EastSouth);
    }

    #[test]
    fn missing_start_game_returns_none() {
        let events = vec![MjaiEvent::EndGame];
        assert!(aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "X".into(),
        })
        .is_none());
    }

    #[test]
    fn observer_mode_no_our_seat() {
        // start_game without `id` (no own seat).
        let events = vec![
            MjaiEvent::StartGame {
                names: vec!["A".into(), "B".into(), "C".into(), "D".into()],
                kyoku_first: Some(0),
                aka_flag: Some(true),
                id: None,
                num_players: 4,
            },
            start_kyoku("E", 0, vec![25000, 25000, 25000, 25000]),
            MjaiEvent::Hora {
                actor: 0,
                target: 1,
                deltas: Some(vec![8000, -8000, 0, 0]),
                ura_markers: None,
            },
            MjaiEvent::EndGame,
        ];
        let rec = aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "OBS".into(),
        })
        .unwrap();
        assert!(rec.our_seat.is_none());
        assert!(rec.our_rank.is_none());
        assert!(rec.our_delta.is_none());
        // Final ranks still computed.
        assert_eq!(rec.final_ranks, vec![1, 4, 2, 3]);
    }

    #[test]
    fn three_player_starting_score_is_35k() {
        let events = vec![
            MjaiEvent::StartGame {
                names: vec!["A".into(), "B".into(), "C".into()],
                kyoku_first: Some(0),
                aka_flag: Some(true),
                id: Some(0),
                num_players: 3,
            },
            MjaiEvent::StartKyoku {
                bakaze: "E".into(),
                dora_marker: "1m".into(),
                kyoku: 1,
                honba: 0,
                kyotaku: 0,
                oya: 0,
                scores: vec![35000, 35000, 35000],
                tehais: vec![vec![]; 3],
                num_players: 3,
            },
            MjaiEvent::Hora {
                actor: 0,
                target: 1,
                deltas: Some(vec![5000, -5000, 0]),
                ura_markers: None,
            },
            MjaiEvent::EndGame,
        ];
        let rec = aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "S3".into(),
        })
        .unwrap();
        assert_eq!(rec.num_players, 3);
        assert_eq!(rec.final_scores.len(), 3);
        // 40000 + 30000 + 35000 = 105000, no normalisation needed.
        assert_eq!(rec.final_scores, vec![40000, 30000, 35000]);
        assert_eq!(rec.our_delta, Some(5000));
    }

    #[test]
    fn kyotaku_normalisation_tops_up_first_place() {
        // 4p: deltas sum to <0 (kyotaku absorbed mid-game) → renormalise.
        let events = vec![
            start_game_4p(0),
            start_kyoku("E", 0, vec![25000, 25000, 25000, 25000]),
            MjaiEvent::Reach { actor: 0, pai: None },
            MjaiEvent::ReachAccepted { actor: 0 }, // -1000 to seat 0
            MjaiEvent::Ryukyoku {
                deltas: Some(vec![1500, -1500, 0, 0]),
            },
            MjaiEvent::EndGame,
        ];
        let rec = aggregate(AggregateInput {
            events: &events,
            platform: Platform::Majsoul,
            started_at: ts(),
            ended_at: ts(),
            id: "NORM".into(),
        })
        .unwrap();
        // Pre-norm: [25500, 23500, 25000, 25000] = 99000. Top up rank-1 (seat 0): +1000.
        assert_eq!(rec.final_scores[0], 26500);
        assert_eq!(rec.final_scores.iter().sum::<i32>(), 100000);
    }
}
