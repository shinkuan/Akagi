//! Convert a `GameStateSnapshot` into a `PlayerInfo34` keyed on a given seat.
//!
//! The snapshot keeps tiles as mjai strings (e.g. `"5mr"`), uses 0..3 seat
//! indices, and uses `"E"`/`"S"`/`"W"`/`"N"` for round wind. The analysis
//! engine wants 34-space tile counts, `Tile34` indices, and `Meld34`
//! structs, so this module bridges the two.
//!
//! Some signals the analysis engine consumes (per-discard tedashi flag,
//! ippatsu eligibility, called-from-others) are not represented in the
//! snapshot today. We fill them with conservative defaults; the risk
//! engine still produces useful output, just slightly more pessimistic.

use anyhow::{anyhow, Result};

use super::hand::{Counts34, Meld34, Meld34Kind, OpponentInfo, PlayerInfo34};
use super::tile::{Tile34, HONOR_E, TILE_COUNT};
use crate::game_state::snapshot::{
    DiscardEntry, GameStateSnapshot, MeldKind as SnapMeldKind, MeldSnapshot, PlayerSnapshot,
};

fn parse_tile(mjai: &str) -> Result<Tile34> {
    Tile34::from_mjai(mjai).ok_or_else(|| anyhow!("invalid mjai tile: {mjai}"))
}

fn count_aka_in_strings(tiles: &[String]) -> u8 {
    tiles.iter().filter(|t| t.ends_with('r')).count() as u8
}

fn count_aka_in_tile(tile: &str) -> u8 {
    if tile.ends_with('r') {
        1
    } else {
        0
    }
}

/// Build a Meld34 from the snapshot view of a meld.
fn meld_from_snapshot(m: &MeldSnapshot) -> Result<Meld34> {
    let kind = match m.kind {
        SnapMeldKind::Chi => Meld34Kind::Chi,
        SnapMeldKind::Pon => Meld34Kind::Pon,
        SnapMeldKind::Daiminkan => Meld34Kind::Daiminkan,
        SnapMeldKind::Ankan => Meld34Kind::Ankan,
        SnapMeldKind::Kakan => Meld34Kind::Kakan,
    };
    let aka_in_meld = count_aka_in_strings(&m.tiles)
        + m.called_tile.as_deref().map(count_aka_in_tile).unwrap_or(0);
    let tiles: Vec<Tile34> = m
        .tiles
        .iter()
        .map(|s| parse_tile(s))
        .collect::<Result<Vec<_>>>()?;
    // Pon/kan: collapse to a single anchor tile (the tile being multiplied).
    let canonical: Vec<Tile34> = match kind {
        Meld34Kind::Chi => tiles,
        _ => {
            // Pick the first tile — they're all the same Tile34 by construction.
            vec![*tiles.first().ok_or_else(|| anyhow!("empty meld tiles"))?]
        }
    };
    let called_tile = match &m.called_tile {
        Some(s) => Some(parse_tile(s)?),
        None => None,
    };
    let from_who = if m.from_who >= 0 {
        Some(m.from_who as u8)
    } else {
        None
    };
    Ok(Meld34 {
        kind,
        tiles: canonical,
        called_tile,
        from_who,
        aka_count: aka_in_meld,
    })
}

/// Convert a player's tehai (mjai strings) to a 34-counts vector and the aka
/// red-five count.
fn hand_from_tehai(tehai: &[String]) -> Result<(Counts34, u8)> {
    let mut counts = [0u8; TILE_COUNT];
    let mut aka = 0u8;
    for s in tehai {
        let t = parse_tile(s)?;
        counts[t.idx() as usize] += 1;
        if s.ends_with('r') {
            aka += 1;
        }
    }
    Ok((counts, aka))
}

/// Compute a player's seat wind tile (`E`/`S`/`W`/`N`) given the dealer seat
/// and the number of players. 4p maps offsets 0/1/2/3 → E/S/W/N. 3p has no
/// player-N wind, so offsets cycle 0/1/2 → E/S/W.
fn jikaze_for(seat: u8, oya: u8, num_players: u8) -> Tile34 {
    let np = num_players.max(1);
    let offset = (seat + np - oya) % np;
    Tile34(HONOR_E + offset)
}

fn river_to_tiles(river: &[DiscardEntry]) -> Result<Vec<Tile34>> {
    river.iter().map(|d| parse_tile(&d.tile)).collect()
}

fn river_tedashi(river: &[DiscardEntry]) -> Vec<bool> {
    river.iter().map(|d| d.tedashi).collect()
}

fn build_opponent(snap: &GameStateSnapshot, p: &PlayerSnapshot) -> Result<OpponentInfo> {
    let discards = river_to_tiles(&p.river)?;
    let tedashi = river_tedashi(&p.river);
    let melds = p
        .melds
        .iter()
        .map(meld_from_snapshot)
        .collect::<Result<Vec<_>>>()?;
    Ok(OpponentInfo {
        seat: p.seat,
        discards,
        tedashi,
        melds,
        is_riichi: p.riichi_declared,
        riichi_turn: p.riichi_declaration_index.map(|i| i as u8),
        can_ippatsu: false,
        jikaze: jikaze_for(p.seat, snap.oya, snap.num_players),
        called_from: vec![],
    })
}

/// Convert a `GameStateSnapshot` into a `PlayerInfo34` for the given seat.
pub fn to_player_info(snap: &GameStateSnapshot, seat: u8) -> Result<PlayerInfo34> {
    if seat as usize >= snap.players.len() {
        return Err(anyhow!("seat {seat} out of range"));
    }
    let me = &snap.players[seat as usize];
    let (hand, aka) = hand_from_tehai(&me.tehai)?;
    let melds = me
        .melds
        .iter()
        .map(meld_from_snapshot)
        .collect::<Result<Vec<_>>>()?;
    let melds_aka_total = melds_aka(&melds);
    let dora_indicators = snap
        .dora_markers
        .iter()
        .map(|s| parse_tile(s))
        .collect::<Result<Vec<_>>>()?;
    let bakaze = parse_tile(&snap.bakaze)?;
    let jikaze = jikaze_for(seat, snap.oya, snap.num_players);
    let own_discards = river_to_tiles(&me.river)?;
    let turn = own_discards.len() as u8;

    let opponents: Vec<OpponentInfo> = snap
        .players
        .iter()
        .filter(|p| p.seat != seat)
        .map(|p| build_opponent(snap, p))
        .collect::<Result<Vec<_>>>()?;

    Ok(PlayerInfo34 {
        num_players: snap.num_players,
        seat,
        hand,
        melds,
        aka_count: aka + melds_aka_total,
        dora_indicators,
        bakaze,
        jikaze,
        turn,
        opponents,
        left_tiles: None,
        own_discards,
    })
}

fn melds_aka(melds: &[Meld34]) -> u8 {
    melds.iter().map(|m| m.aka_count).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_state::snapshot::Phase;

    fn snap_with_player_0_hand(tehai: Vec<&str>) -> GameStateSnapshot {
        let make_player = |seat: u8, t: Vec<String>| PlayerSnapshot {
            seat,
            tehai: t,
            melds: vec![],
            river: vec![],
            score: 25_000,
            riichi_declared: false,
            riichi_stage: false,
            double_riichi: false,
            riichi_declaration_index: None,
            kita_tiles: vec![],
            drawn_tile: None,
        };
        GameStateSnapshot {
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
            players: vec![
                make_player(0, tehai.into_iter().map(String::from).collect()),
                make_player(1, vec!["1m".into(); 13]),
                make_player(2, vec!["1m".into(); 13]),
                make_player(3, vec!["1m".into(); 13]),
            ],
            dora_markers: vec!["2m".into()],
            our_seat: Some(0),
        }
    }

    #[test]
    fn jikaze_rotates_with_oya() {
        // 4p: seat == oya → E
        assert_eq!(jikaze_for(0, 0, 4).to_mjai(), "E");
        // 4p: seat one to the right of oya → S
        assert_eq!(jikaze_for(1, 0, 4).to_mjai(), "S");
        // 4p: seat == oya - 1 (mod 4) → N
        assert_eq!(jikaze_for(0, 1, 4).to_mjai(), "N");
        assert_eq!(jikaze_for(2, 0, 4).to_mjai(), "W");
        assert_eq!(jikaze_for(3, 0, 4).to_mjai(), "N");
    }

    #[test]
    fn jikaze_3p_skips_north_self_wind() {
        // 3p: only E/S/W cycle; oya at seat 0 → seat 0 = E, 1 = S, 2 = W.
        assert_eq!(jikaze_for(0, 0, 3).to_mjai(), "E");
        assert_eq!(jikaze_for(1, 0, 3).to_mjai(), "S");
        assert_eq!(jikaze_for(2, 0, 3).to_mjai(), "W");
        // Oya rotation: seat 0 with oya=1 wraps to W, not N.
        assert_eq!(jikaze_for(0, 1, 3).to_mjai(), "W");
    }

    #[test]
    fn hand_counts_aggregate() {
        let snap = snap_with_player_0_hand(vec![
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1s",
        ]);
        let info = to_player_info(&snap, 0).unwrap();
        assert_eq!(info.hand_size(), 13);
        assert_eq!(info.opponents.len(), 3);
        assert_eq!(info.bakaze.to_mjai(), "E");
        assert_eq!(info.jikaze.to_mjai(), "E");
    }

    #[test]
    fn aka_in_tehai_counts() {
        let snap = snap_with_player_0_hand(vec![
            "5mr", "1m", "2m", "3m", "4m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1s",
        ]);
        let info = to_player_info(&snap, 0).unwrap();
        assert_eq!(info.aka_count, 1);
    }

    #[test]
    fn dora_indicators_carry_through() {
        let snap = snap_with_player_0_hand(vec!["1m"; 13]);
        let info = to_player_info(&snap, 0).unwrap();
        assert_eq!(info.dora_indicators.len(), 1);
        assert_eq!(info.dora_indicators[0].to_mjai(), "2m");
    }

    #[test]
    fn opponent_seats_excluded_from_self() {
        let snap = snap_with_player_0_hand(vec!["1m"; 13]);
        let info = to_player_info(&snap, 0).unwrap();
        let seats: Vec<u8> = info.opponents.iter().map(|o| o.seat).collect();
        assert_eq!(seats, vec![1, 2, 3]);
    }

    #[test]
    fn out_of_range_seat_errors() {
        let snap = snap_with_player_0_hand(vec!["1m"; 13]);
        assert!(to_player_info(&snap, 4).is_err());
    }
}
