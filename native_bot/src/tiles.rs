//! Tile-index helpers shared by the observation encoder, the action codec,
//! the extractor, and inference.
//!
//! riichienv-core represents tiles as raw 136-space ids (`tid`, 0..=135):
//! `tile34 = tid / 4` (0..=33), with red-fives at the specific ids
//! `16` (5m), `52` (5p), `88` (5s). 3-player mahjong (sanma) removes 2m–8m,
//! so it uses a compacted 27-tile axis via [`TILE34_TO_COMPACT`].

/// tile34 index (0..=33) of a raw 136-space tile id.
#[inline]
pub fn tile34(tid: u8) -> usize {
    (tid / 4) as usize
}

/// Whether a raw tile id is a red five (aka dora).
#[inline]
pub fn is_aka(tid: u8) -> bool {
    matches!(tid, 16 | 52 | 88)
}

/// tile34 positions of the three red fives: 5m=4, 5p=13, 5s=22.
pub const AKA_TILE34: [usize; 3] = [4, 13, 22];

/// Map a tile34 type (0..=33) to the sanma compact index (0..=26), or `None`
/// for the manzu 2m–8m tiles that do not exist in 3-player mahjong.
///
/// Mirrors `riichienv_core::action`'s private `TILE34_TO_COMPACT` (255 = absent).
pub const TILE34_TO_COMPACT: [Option<usize>; 34] = {
    let mut m = [None; 34];
    m[0] = Some(0); // 1m
                    // 1..=7 (2m-8m) stay None
    m[8] = Some(1); // 9m
                    // 1p..9p -> 2..10
    let mut i = 9;
    while i <= 17 {
        m[i] = Some(i - 7);
        i += 1;
    }
    // 1s..9s -> 11..19
    while i <= 26 {
        m[i] = Some(i - 7);
        i += 1;
    }
    // ESWN + PFC (27..33) -> 20..26
    while i <= 33 {
        m[i] = Some(i - 7);
        i += 1;
    }
    m
};

/// Compact tile index for a raw tile id under the given player count.
/// 4-player → `tile34` (0..=33). 3-player → compact 0..=26 (`None` for 2m–8m).
#[inline]
pub fn tile_index(tid: u8, num_players: u8) -> Option<usize> {
    if num_players == 3 {
        TILE34_TO_COMPACT[tile34(tid)]
    } else {
        Some(tile34(tid))
    }
}

/// Tile-axis width for the given player count (34 for 4p, 27 for 3p).
#[inline]
pub fn tile_dim(num_players: u8) -> usize {
    if num_players == 3 {
        27
    } else {
        34
    }
}

/// Dora tile (tile34) indicated by a dora-*indicator* tile34, for the given
/// player count. 4-player uses the standard wrap; 3-player wraps 1m↔9m for the
/// two surviving manzu and is otherwise identical.
#[inline]
pub fn next_dora_tile34(indicator34: u8, num_players: u8) -> u8 {
    if num_players == 3 {
        match indicator34 {
            0 => 8, // 1m -> 9m
            8 => 0, // 9m -> 1m
            other => riichienv_core::types::standard_next_dora_tile(other),
        }
    } else {
        riichienv_core::types::standard_next_dora_tile(indicator34)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile34_and_aka() {
        assert_eq!(tile34(0), 0); // 1m
        assert_eq!(tile34(16), 4); // 5m (red)
        assert_eq!(tile34(52), 13); // 5p (red)
        assert_eq!(tile34(88), 22); // 5s (red)
        assert_eq!(tile34(135), 33); // chun
        assert!(is_aka(16) && is_aka(52) && is_aka(88));
        assert!(!is_aka(17) && !is_aka(0));
    }

    #[test]
    fn compact_map_drops_manzu_2_to_8() {
        assert_eq!(TILE34_TO_COMPACT[0], Some(0)); // 1m

        // 2m-8m gone in sanma: every position between the ones that remain.
        assert!((1..=7).all(|t| TILE34_TO_COMPACT[t].is_none()));
        assert_eq!(TILE34_TO_COMPACT[8], Some(1)); // 9m
        assert_eq!(TILE34_TO_COMPACT[9], Some(2)); // 1p
        assert_eq!(TILE34_TO_COMPACT[13], Some(6)); // 5p
        assert_eq!(TILE34_TO_COMPACT[22], Some(15)); // 5s
        assert_eq!(TILE34_TO_COMPACT[33], Some(26)); // chun
    }

    #[test]
    fn dora_wrap() {
        assert_eq!(next_dora_tile34(0, 4), 1); // 1m -> 2m (4p)
        assert_eq!(next_dora_tile34(8, 4), 0); // 9m -> 1m (4p)
        assert_eq!(next_dora_tile34(0, 3), 8); // 1m -> 9m (sanma)
        assert_eq!(next_dora_tile34(8, 3), 0); // 9m -> 1m (sanma)
        assert_eq!(next_dora_tile34(30, 4), 27); // N -> E
        assert_eq!(next_dora_tile34(33, 4), 31); // chun -> haku
    }
}
