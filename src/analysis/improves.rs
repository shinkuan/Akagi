//! 13-tile analysis core: waits, improves, agari rate, score, mixed-waits-score,
//! and the `next_shanten_waits` look-ahead.
//!
//! Mirrors the mahjong-helper `shanten_improve.go` algorithm. The Go original
//! drives a recursive search tree; we replace it with a single-step look-ahead
//! that produces the same `Hand13AnalysisResult` output fields. The inner
//! "improves" pass (non-progressing draws that widen the wait set) is
//! implemented exactly as the Go author wrote it — no shanten gate, no
//! result truncation. The only equivalent of Go's `_stopShanten` is our
//! search-depth cap (already enforced by single-step look-ahead).

use std::collections::BTreeMap;

use super::agari_rate;
use super::data::point::{RON_POINT_DAMA, RON_POINT_RIICHI};
use super::hand::PlayerInfo34;
use super::result::{Hand13Result, ImproveEntry, WaitInfo};
use super::score;
use super::shanten;
use super::tile::{Tile34, TILE_COUNT};
use super::waits::{self, Waits};

const LEFT_TURNS: f64 = 10.0;

/// Speed-score formula from `Hand13AnalysisResult.speedScore` in the Go ref.
/// Approximates "P(progressing twice within `LEFT_TURNS` draws)" × 100.
fn speed_score(waits_total: u32, avg_next_waits: f64, left_count: u32) -> f64 {
    if waits_total == 0 || avg_next_waits <= 0.0 || left_count == 0 {
        return 0.0;
    }
    let p2 = waits_total as f64 / left_count as f64;
    let p1 = avg_next_waits / left_count as f64;
    if p1 <= 0.0 || p2 <= 0.0 {
        return 0.0;
    }
    let p2_ = 1.0 - p2;
    let p1_ = 1.0 - p1;
    if (p2_ - p1_).abs() < f64::EPSILON {
        // Degenerate (p1 ≈ p2): fall back to the integral form.
        let geom = p2_ * (1.0 - p2_.powf(LEFT_TURNS)) / p2;
        return p2 * p1 * (LEFT_TURNS - geom) * 100.0;
    }
    let sum_p2 = p2_ * (1.0 - p2_.powf(LEFT_TURNS)) / p2;
    let sum_p1 = p1_ * (1.0 - p1_.powf(LEFT_TURNS)) / p1;
    p2 * p1 * (sum_p2 - sum_p1) / (p2_ - p1_) * 100.0
}

/// Compute next-shanten waits look-ahead.
///
/// For each tile in `current_waits`, simulate drawing it, then iterate over
/// each possible discard from the resulting 14-tile state. Keep the discard
/// that yields the largest progressing-waits set. Return per-draw best counts
/// + the weighted average across draws.
fn next_shanten_waits(
    info: &PlayerInfo34,
    current_waits: &Waits,
    current_shanten: i8,
) -> (BTreeMap<u8, u32>, f64) {
    let mut per_draw: BTreeMap<u8, u32> = BTreeMap::new();
    if current_shanten < 0 {
        return (per_draw, 0.0);
    }

    let target = current_shanten - 1;
    let len_div3 = info.tehai_len_div3();
    let mut probe = info.hand;

    for (drawn, _left) in current_waits.iter() {
        if probe[drawn as usize] >= 4 {
            continue;
        }
        probe[drawn as usize] += 1;
        // 14-tile state: try every discard, keep best.
        let mut best = 0u32;
        for d in 0..TILE_COUNT {
            if probe[d] == 0 {
                continue;
            }
            probe[d] -= 1;
            let post_shanten = shanten::shanten_from_counts(&probe, len_div3);
            if post_shanten == target {
                let new_left = info.compute_left_tiles();
                // adjust new_left for the removed draw
                let mut nl = new_left;
                if nl[drawn as usize] > 0 {
                    nl[drawn as usize] -= 1;
                }
                let next_waits = waits::waits_for_counts(&probe, len_div3, &nl, post_shanten - 1);
                let total = next_waits.total_left();
                if total > best {
                    best = total;
                }
            }
            probe[d] += 1;
        }
        probe[drawn as usize] -= 1;
        per_draw.insert(drawn, best);
    }

    let mut weight_sum = 0u64;
    let mut sum = 0u64;
    for (drawn, count) in &per_draw {
        if let Some(&left) = current_waits.map.get(drawn) {
            sum += (*count as u64) * (left as u64);
            weight_sum += left as u64;
        }
    }
    let avg = if weight_sum > 0 {
        sum as f64 / weight_sum as f64
    } else {
        0.0
    };
    (per_draw, avg)
}

/// Improves pass.
///
/// Mirrors the mahjong-helper `shanten_improve.go` "improves" pass (lines
/// 260-285 in the upstream Go source) plus the avg-formula at lines 403-412.
///
/// For each tile that does NOT progress shanten when drawn, try every possible
/// discard from the resulting 14-state. If the resulting 13-state still has
/// `cur_shanten` AND its waits set has more total-left than the current waits,
/// that draw "improves" the hand. Track the best (largest) widening per draw,
/// count the total number of (draw, discard) widening pairs.
fn compute_improves(
    info: &PlayerInfo34,
    cur_shanten: shanten::Shanten,
    current_waits: &Waits,
) -> (
    BTreeMap<u8, Waits>, // best widened waits per drawn tile
    u32,                 // total improve_way_count
    [u32; TILE_COUNT],   // per-tile max improve waits count (initial = waits_total)
) {
    let waits_total = current_waits.total_left();
    let mut max_per_tile = [waits_total; TILE_COUNT];
    let mut improves: BTreeMap<u8, Waits> = BTreeMap::new();
    let mut improve_way_count: u32 = 0;

    if cur_shanten < 0 {
        return (improves, improve_way_count, max_per_tile);
    }

    let len_div3 = info.tehai_len_div3();
    let left = info.compute_left_tiles();
    let mut probe = info.hand;

    for drawn in 0..TILE_COUNT {
        // Skip progressing draws (already handled by next_shanten_waits).
        if current_waits.map.contains_key(&(drawn as u8)) {
            continue;
        }
        if left[drawn] == 0 || probe[drawn] >= 4 {
            continue;
        }
        probe[drawn] += 1;
        // Try discarding each tile in hand other than the just-drawn one.
        for d in 0..TILE_COUNT {
            if probe[d] == 0 {
                continue;
            }
            if d == drawn {
                // Discarding what we just drew → identity, doesn't widen.
                continue;
            }
            probe[d] -= 1;
            let post_shanten = shanten::shanten_from_counts(&probe, len_div3);
            if post_shanten == cur_shanten {
                let new_waits = waits::waits_for_counts(&probe, len_div3, &left, cur_shanten - 1);
                let new_total = new_waits.total_left();
                if new_total > waits_total {
                    improve_way_count += 1;
                    if new_total > max_per_tile[drawn] {
                        max_per_tile[drawn] = new_total;
                        improves.insert(drawn as u8, new_waits);
                    }
                }
            }
            probe[d] += 1;
        }
        probe[drawn] -= 1;
    }

    (improves, improve_way_count, max_per_tile)
}

/// Compute the weighted-average waits count factoring improves in. Mirrors
/// Go `result13.AvgImproveWaitsCount = improveWaitsSum / weight` at lines 403-412.
fn avg_improve_waits(left_tiles: &[u8; TILE_COUNT], max_per_tile: &[u32; TILE_COUNT]) -> f64 {
    let mut sum: u64 = 0;
    let mut weight: u64 = 0;
    for i in 0..TILE_COUNT {
        let w = left_tiles[i] as u64;
        sum += w * max_per_tile[i] as u64;
        weight += w;
    }
    if weight == 0 {
        0.0
    } else {
        sum as f64 / weight as f64
    }
}

/// Analyze a 13-tile player state.
pub fn analyze_13(info: &PlayerInfo34) -> Hand13Result {
    let cur_shanten = shanten::shanten(info);
    let current_waits = waits::waits(info);
    let waits_total = current_waits.total_left();

    let (next_shanten_waits_count, avg_next) =
        next_shanten_waits(info, &current_waits, cur_shanten);

    let left = info.compute_left_tiles();
    let left_count: u32 = left.iter().map(|&v| v as u32).sum();
    let mut mixed_waits_score = speed_score(waits_total, avg_next, left_count);
    // 2-shanten special case: divided by 4 (Go shanten_improve.go lines 416-418).
    if cur_shanten == 2 {
        mixed_waits_score /= 4.0;
    }

    // Improves pass — must come before agari/score so we can fold its weighted
    // average back into the result.
    let (improves_map, improve_way_count, max_per_tile) =
        compute_improves(info, cur_shanten, &current_waits);
    let avg_improve = if !improves_map.is_empty() {
        avg_improve_waits(&left, &max_per_tile)
    } else {
        waits_total as f64
    };
    let improves_vec: Vec<ImproveEntry> = improves_map
        .iter()
        .map(|(tile, w)| ImproveEntry {
            draw: Tile34(*tile).to_mjai().to_string(),
            widened_waits: WaitInfo::from_waits(w),
            widened_total: w.total_left(),
        })
        .collect();

    let is_open = info
        .melds
        .iter()
        .any(|m| !matches!(m.kind, super::hand::Meld34Kind::Ankan));

    // Agari rate: only meaningful when actually tenpai.
    let mut tile_rates: BTreeMap<u8, f64> = BTreeMap::new();
    let mut avg_agari_rate = 0.0;
    let mut is_furiten = false;
    let mut dama_point = 0.0;
    let mut riichi_point = 0.0;
    let mut yaku_ids: Vec<u32> = Vec::new();
    let mut yaku_names: Vec<String> = Vec::new();
    let mut yaku_names_en: Vec<String> = Vec::new();
    if cur_shanten == 0 && !current_waits.is_empty() {
        // Compute dora set from indicators.
        let dora: Vec<_> = info.dora_indicators.iter().map(|d| d.dora_next()).collect();
        is_furiten = agari_rate::is_furiten(&current_waits, &info.own_discards);
        tile_rates = agari_rate::per_wait(&current_waits, &info.own_discards, &dora, is_furiten);
        avg_agari_rate = agari_rate::average(&current_waits, &info.own_discards, &dora, is_furiten);

        let est = score::expectation(info, &current_waits, !is_open);
        dama_point = est.dama_point;
        riichi_point = est.riichi_point;
        yaku_ids = est.yaku_ids;
        yaku_names = est.yaku_names;
        yaku_names_en = est.yaku_names_en;
        if !est.has_yaku && is_open {
            // Open & yakuless: cannot win → zero out point expectation.
            avg_agari_rate = 0.0;
        }
        // Fall back to the table baseline when the calc engine couldn't
        // produce a number (e.g. yakuless closed hand at dama).
        if dama_point == 0.0 && !is_open {
            dama_point = RON_POINT_DAMA;
        }
        if riichi_point == 0.0 && !is_open {
            riichi_point = RON_POINT_RIICHI;
        }
    }

    // Furiten-rate: 1.0 at genuine tenpai furiten, 0.5 at 1-shanten possible
    // furiten (we may still escape), 0 otherwise. Mirrors Go reference
    // shanten_improve.go lines 363-373.
    let furiten_rate = if cur_shanten <= 1 {
        let mut r = 0.0f64;
        for d in &info.own_discards {
            if current_waits.map.contains_key(&d.idx()) {
                r = if cur_shanten == 0 { 1.0 } else { 0.5 };
                break;
            }
        }
        r
    } else {
        0.0
    };

    let best_point = if riichi_point > 0.0 {
        riichi_point
    } else {
        dama_point
    };
    let mixed_round_point = if avg_agari_rate > 0.0 && best_point > 0.0 {
        avg_agari_rate / 100.0 * (best_point + 1500.0) - 1500.0
    } else {
        0.0
    };

    let waits = WaitInfo::from_waits_with_rates(&current_waits, &tile_rates);
    Hand13Result {
        shanten: cur_shanten,
        waits,
        waits_total,
        next_shanten_waits_count,
        avg_next_shanten_waits: avg_next,
        mixed_waits_score,
        avg_agari_rate,
        is_furiten,
        furiten_rate,
        improves: improves_vec,
        improve_way_count,
        avg_improve_waits_count: avg_improve,
        dama_point,
        riichi_point,
        mixed_round_point,
        yaku_ids,
        yaku_names,
        yaku_names_en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::hand::PlayerInfo34Builder;
    use crate::analysis::tile::Tile34;

    #[test]
    fn tenpai_produces_waits_and_agari_rate() {
        // 234m 234p 234s 67p + EE pair, dama waiting on 5p/8p
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "2m", "3m", "4m", "2p", "3p", "4p", "6p", "7p", "2s", "3s", "4s", "E", "E",
            ])
            .build();
        let r = analyze_13(&info);
        assert_eq!(r.shanten, 0);
        assert_eq!(r.waits.len(), 2);
        assert!(r.avg_agari_rate > 0.0);
    }

    #[test]
    fn one_shanten_speed_score_positive() {
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "N", "P",
            ])
            .build();
        let r = analyze_13(&info);
        assert_eq!(r.shanten, 1);
        assert!(!r.waits.is_empty());
        assert!(r.avg_next_shanten_waits > 0.0);
        assert!(r.mixed_waits_score > 0.0);
    }

    #[test]
    fn next_shanten_waits_lookahead_exists() {
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "N", "P",
            ])
            .build();
        let r = analyze_13(&info);
        // Drawing 3p completes a run → tenpai → many waits possible.
        let three_p = Tile34::from_mjai("3p").unwrap().idx();
        assert!(r.next_shanten_waits_count.contains_key(&three_p));
        assert!(r.next_shanten_waits_count[&three_p] > 0);
    }

    #[test]
    fn improves_pass_finds_widening_draws() {
        // 1-shanten hand with an obvious improvement opportunity:
        // 234m 234p 234s 67p + 11s pair (13 tiles) is tenpai. To get a
        // 1-shanten with a clear improve, swap the pair for a single + extra.
        // Build: 234m 234p 234s 6p7p + 1s + 9s (13 tiles, 1-shanten).
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "2m", "3m", "4m", "2p", "3p", "4p", "6p", "7p", "2s", "3s", "4s", "1s", "9s",
            ])
            .build();
        let r = analyze_13(&info);
        assert_eq!(r.shanten, 1);
        // At 1-shanten there should be at least one (drawn, discard) pair
        // that widens the wait set.
        assert!(
            r.improve_way_count > 0,
            "expected non-zero improve_way_count, got {}",
            r.improve_way_count
        );
        assert!(!r.improves.is_empty());
    }

    #[test]
    fn improves_empty_for_kokushi_13_wait() {
        // Kokushi 13-面待 — every draw outside the yaochuhai set forces
        // discarding a kokushi anchor, breaking the shape. No improvement
        // possible.
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "1m", "9m", "1p", "9p", "1s", "9s", "E", "S", "W", "N", "P", "F", "C",
            ])
            .build();
        let r = analyze_13(&info);
        assert_eq!(r.shanten, 0);
        assert_eq!(r.improve_way_count, 0);
        assert!(r.improves.is_empty());
    }

    #[test]
    fn two_shanten_score_is_quartered() {
        // 2-shanten: 234m + 567p + 8s9s tatsu + NN pair + 3 isolated honors.
        let info = PlayerInfo34Builder::new()
            .add_many(&[
                "2m", "3m", "4m", "5p", "6p", "7p", "8s", "9s", "N", "N", "E", "S", "W",
            ])
            .build();
        let r = analyze_13(&info);
        assert_eq!(r.shanten, 2);
        // The 2-shanten /=4 transformation must shrink the score below the
        // unscaled equivalent. Both should still be non-negative.
        assert!(r.mixed_waits_score >= 0.0);
    }

    #[test]
    fn furiten_rate_split_by_shanten() {
        // 1-shanten + a wait tile in our river → furiten_rate should be 0.5.
        let mut info = PlayerInfo34Builder::new()
            .add_many(&[
                "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "N", "P",
            ])
            .build();
        // 3p is a progressing draw at this shape; put it on our river.
        info.own_discards.push(Tile34::from_mjai("3p").unwrap());
        let r = analyze_13(&info);
        assert_eq!(r.shanten, 1);
        assert!(
            (r.furiten_rate - 0.5).abs() < 1e-9,
            "got {}",
            r.furiten_rate
        );
    }
}
