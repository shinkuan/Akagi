//! Our own observation (feature) encoding.
//!
//! We deliberately do **not** use riichienv-core's `Observation::encode()`
//! (that tensor method is `python`-feature-gated and unavailable in a pure-Rust
//! build). Instead we design a compact, self-relative feature plane and encode
//! it from an [`EncInput`] that is built the same way for both training-data
//! extraction and live inference — so the model always sees identical inputs.
//!
//! ## Layout
//!
//! Output is a **channel-major** `f32` buffer of shape `[C, T]` flattened as
//! `buf[ch * T + tile]`, where `T` is the tile axis (34 for 4p, 27 for sanma)
//! and `C` = [`channels`]`(num_players)`. Everything is encoded in *relative*
//! seat order — index 0 is always the deciding player, then the seats to their
//! right (shimocha, toimen, kamicha).
//!
//! Channel groups (per-player groups have `N` planes = player count):
//! ```text
//!   self hand thresholds (>=1,>=2,>=3,>=4)   4
//!   self aka-5 in hand                        1
//!   self drawn tile (one-hot)                 1
//!   self tenpai waits (one-hot set)           1
//!   dora indicators (count)                   1
//!   dora tiles (count)                        1
//!   per-player discards (count)               N
//!   per-player meld tiles (count)             N
//!   per-player riichi (broadcast)             N
//!   per-player riichi-declare tile (one-hot)  N
//!   per-player score (broadcast, normalized)  N
//!   last discard (one-hot, call context)      1
//!   round wind (one-hot on honor tile)        1
//!   self seat wind (one-hot on honor tile)    1
//!   honba (broadcast)                         1
//!   riichi sticks (broadcast)                 1
//!   turn progress (broadcast)                 1
//!   self tenpai (broadcast)                   1
//!   self is-dealer (broadcast)                1
//!   kyoku index (broadcast)                   1
//!   self riichi-declared (broadcast)          1
//!   per-player kita count (broadcast, sanma)  N   (4p: 0 planes)
//! ```

use crate::tiles::{is_aka, next_dora_tile34, tile_dim, tile_index, AKA_TILE34};

/// Planes that describe the deciding player only, before the per-player groups:
/// hand thresholds (4) + aka + drawn tile + waits + dora indicators + dora tiles.
const SELF_PLANES: usize = 4 + 1 + 1 + 1 + 1 + 1; // = 9
/// Table-wide planes that follow the per-player groups: last discard, round wind,
/// seat wind, honba, riichi sticks, turn progress, self tenpai, self is-dealer,
/// kyoku index, self riichi-declared.
const GLOBAL_PLANES: usize = 10;
/// Per-player groups before the globals: discards, melds, riichi, riichi tile, score.
const PER_PLAYER_GROUPS: usize = 5;

/// Number of feature channels for the given player count.
pub fn channels(num_players: u8) -> usize {
    let n = num_players as usize;
    let kita_groups = if num_players == 3 { 1 } else { 0 };
    SELF_PLANES + GLOBAL_PLANES + (PER_PLAYER_GROUPS + kita_groups) * n
}

/// Channel index of the "last discard" one-hot plane — the first of the global
/// planes, i.e. straight after the per-player groups. `encode` `debug_assert`s
/// its cursor against this, so the two can't drift apart.
pub fn last_discard_channel(num_players: u8) -> usize {
    SELF_PLANES + PER_PLAYER_GROUPS * num_players as usize
}

/// Per-player, self-relative view fed to the encoder.
#[derive(Debug, Clone, Default)]
pub struct SeatFeat {
    /// Raw tile ids (0..=135) discarded by this seat, in order.
    pub discards: Vec<u8>,
    /// Raw tile ids that make up this seat's called melds (flattened).
    pub meld_tiles: Vec<u8>,
    pub riichi_declared: bool,
    /// Raw tile id of this seat's riichi-declaration discard, if any.
    pub riichi_tile: Option<u8>,
    pub score: i32,
    /// Number of kita (nukidora) set aside (sanma only).
    pub kita_count: u8,
}

/// Everything the encoder needs, already resolved to relative seat order
/// (index 0 = deciding player). Built by adapters in [`crate::adapt`].
#[derive(Debug, Clone)]
pub struct EncInput {
    pub num_players: u8,
    /// Deciding player's concealed hand (raw tile ids).
    pub hand: Vec<u8>,
    /// Deciding player's just-drawn tile, if any.
    pub drawn_tile: Option<u8>,
    /// Deciding player's tenpai waits as tile34 indices.
    pub waits: Vec<u8>,
    pub is_tenpai: bool,
    /// Dora *indicator* raw tile ids.
    pub dora_indicators: Vec<u8>,
    /// Per-seat features, relative order (index 0 = self).
    pub seats: Vec<SeatFeat>,
    /// Most-recent discard on the table (raw tile id) — call context.
    pub last_discard: Option<u8>,
    /// Round wind index (0=E,1=S,2=W,3=N).
    pub round_wind: u8,
    /// Deciding player's seat wind index (0=E dealer .. 3=N).
    pub seat_wind: u8,
    pub honba: u8,
    pub riichi_sticks: u32,
    pub turn_count: u32,
    pub is_dealer: bool,
    pub kyoku_index: u8,
    /// Deciding player's own riichi-declared flag.
    pub self_riichi: bool,
}

impl EncInput {
    /// Encode into a channel-major `[C, T]` f32 buffer.
    pub fn encode(&self) -> Vec<f32> {
        let np = self.num_players;
        let t = tile_dim(np);
        let c = channels(np);
        let n = np as usize;
        let mut buf = vec![0.0f32; c * t];

        // Cursor over channels; helpers close over `buf`, `t`.
        let mut ch = 0usize;
        macro_rules! set {
            ($channel:expr, $tile:expr, $val:expr) => {
                buf[$channel * t + $tile] = $val;
            };
        }
        // Fill an entire plane with a constant (broadcast scalar).
        macro_rules! broadcast {
            ($channel:expr, $val:expr) => {{
                let base = $channel * t;
                for i in 0..t {
                    buf[base + i] = $val;
                }
            }};
        }

        // --- self hand: 4 threshold planes + aka ---
        let mut hand_counts = vec![0u8; t];
        let mut aka_here = [false; 3];
        for &tid in &self.hand {
            if let Some(idx) = tile_index(tid, np) {
                hand_counts[idx] += 1;
            }
            if is_aka(tid) {
                for (k, &a34) in AKA_TILE34.iter().enumerate() {
                    if crate::tiles::tile34(tid) == a34 {
                        aka_here[k] = true;
                    }
                }
            }
        }
        for thresh in 1..=4u8 {
            for idx in 0..t {
                if hand_counts[idx] >= thresh {
                    set!(ch, idx, 1.0);
                }
            }
            ch += 1;
        }
        // aka plane
        for (k, &a34) in AKA_TILE34.iter().enumerate() {
            if aka_here[k] {
                if let Some(idx) = tile_index((a34 as u8) * 4, np) {
                    set!(ch, idx, 1.0);
                }
            }
        }
        ch += 1;

        // --- self drawn tile (one-hot) ---
        if let Some(tid) = self.drawn_tile {
            if let Some(idx) = tile_index(tid, np) {
                set!(ch, idx, 1.0);
            }
        }
        ch += 1;

        // --- self waits (tile34 -> compact) ---
        for &w34 in &self.waits {
            if let Some(idx) = tile_index(w34 * 4, np) {
                set!(ch, idx, 1.0);
            }
        }
        ch += 1;

        // --- dora indicators (count) ---
        for &tid in &self.dora_indicators {
            if let Some(idx) = tile_index(tid, np) {
                buf[ch * t + idx] += 0.25;
            }
        }
        ch += 1;

        // --- dora tiles (count) ---
        for &tid in &self.dora_indicators {
            let dora34 = next_dora_tile34(crate::tiles::tile34(tid) as u8, np);
            if let Some(idx) = tile_index(dora34 * 4, np) {
                buf[ch * t + idx] += 0.25;
            }
        }
        ch += 1;

        // --- per-player discards (count, normalized /4) ---
        for seat in &self.seats {
            for &tid in &seat.discards {
                if let Some(idx) = tile_index(tid, np) {
                    buf[ch * t + idx] += 0.25;
                }
            }
            ch += 1;
        }
        // Pad if fewer seats than n (defensive; adapters always fill n).
        ch += n.saturating_sub(self.seats.len());

        // --- per-player meld tiles (count, normalized /4) ---
        for seat in &self.seats {
            for &tid in &seat.meld_tiles {
                if let Some(idx) = tile_index(tid, np) {
                    buf[ch * t + idx] += 0.25;
                }
            }
            ch += 1;
        }
        ch += n.saturating_sub(self.seats.len());

        // --- per-player riichi (broadcast) ---
        for seat in &self.seats {
            if seat.riichi_declared {
                broadcast!(ch, 1.0);
            }
            ch += 1;
        }
        ch += n.saturating_sub(self.seats.len());

        // --- per-player riichi-declare tile (one-hot) ---
        for seat in &self.seats {
            if let Some(tid) = seat.riichi_tile {
                if let Some(idx) = tile_index(tid, np) {
                    set!(ch, idx, 1.0);
                }
            }
            ch += 1;
        }
        ch += n.saturating_sub(self.seats.len());

        // --- per-player score (broadcast, normalized, non-negative) ---
        // Kept in [0, 4] so the extractor can store obs as u8 without a sign.
        for seat in &self.seats {
            let v = (seat.score as f32 / 50_000.0).clamp(0.0, 4.0);
            broadcast!(ch, v);
            ch += 1;
        }
        ch += n.saturating_sub(self.seats.len());

        // --- last discard (one-hot) ---
        debug_assert_eq!(
            ch,
            last_discard_channel(np),
            "last-discard plane moved; update `last_discard_channel`"
        );
        if let Some(tid) = self.last_discard {
            if let Some(idx) = tile_index(tid, np) {
                set!(ch, idx, 1.0);
            }
        }
        ch += 1;

        // --- round wind (one-hot on honor position) ---
        // wind i lives at tile34 27+i; encode via tile id (27+i)*4.
        if let Some(idx) = tile_index((27 + self.round_wind.min(3)) * 4, np) {
            set!(ch, idx, 1.0);
        }
        ch += 1;

        // --- self seat wind (one-hot on honor position) ---
        if let Some(idx) = tile_index((27 + self.seat_wind.min(3)) * 4, np) {
            set!(ch, idx, 1.0);
        }
        ch += 1;

        // --- honba / sticks / turn / tenpai / dealer / kyoku / self-riichi (broadcast) ---
        broadcast!(ch, (self.honba as f32 / 4.0).min(2.0));
        ch += 1;
        broadcast!(ch, (self.riichi_sticks as f32 / 4.0).min(2.0));
        ch += 1;
        broadcast!(ch, (self.turn_count as f32 / 70.0).min(1.0));
        ch += 1;
        broadcast!(ch, if self.is_tenpai { 1.0 } else { 0.0 });
        ch += 1;
        broadcast!(ch, if self.is_dealer { 1.0 } else { 0.0 });
        ch += 1;
        broadcast!(ch, (self.kyoku_index as f32 / 8.0).min(1.0));
        ch += 1;
        broadcast!(ch, if self.self_riichi { 1.0 } else { 0.0 });
        ch += 1;

        // --- per-player kita count (broadcast, sanma only) ---
        if np == 3 {
            for seat in &self.seats {
                broadcast!(ch, (seat.kita_count as f32 / 4.0).min(1.0));
                ch += 1;
            }
            ch += n.saturating_sub(self.seats.len());
        }

        debug_assert_eq!(ch, c, "channel cursor must match declared channel count");
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(np: u8) -> EncInput {
        let seats = (0..np)
            .map(|i| SeatFeat {
                discards: vec![0, 4, 4],
                meld_tiles: vec![],
                riichi_declared: i == 1,
                riichi_tile: if i == 1 { Some(36) } else { None },
                score: 25_000,
                kita_count: if np == 3 { i } else { 0 },
            })
            .collect();
        EncInput {
            num_players: np,
            hand: vec![0, 1, 2, 16, 52, 88, 108, 108, 108, 120, 124, 128, 132],
            drawn_tile: Some(132),
            waits: vec![9, 18],
            is_tenpai: true,
            dora_indicators: vec![0, 108],
            seats,
            last_discard: Some(4),
            round_wind: 0,
            seat_wind: 1,
            honba: 2,
            riichi_sticks: 1,
            turn_count: 7,
            is_dealer: false,
            kyoku_index: 0,
            self_riichi: false,
        }
    }

    #[test]
    fn encode_shape_4p() {
        let inp = sample(4);
        let buf = inp.encode();
        assert_eq!(buf.len(), channels(4) * 34);
    }

    #[test]
    fn encode_shape_3p() {
        let inp = sample(3);
        let buf = inp.encode();
        assert_eq!(buf.len(), channels(3) * 27);
    }

    #[test]
    fn self_relative_hand_present() {
        // Hand contains 1m (tile34 0); the >=1 plane (channel 0) must mark it.
        let inp = sample(4);
        let buf = inp.encode();
        // Channel 0 (hand >= 1), tile34 0 (1m).
        assert_eq!(buf[0], 1.0, "1m present in >=1 hand plane");
        // aka plane is channel 4: 5m/5p/5s red present -> positions 4,13,22
        let aka_ch = 4;
        assert_eq!(buf[aka_ch * 34 + 4], 1.0);
        assert_eq!(buf[aka_ch * 34 + 13], 1.0);
        assert_eq!(buf[aka_ch * 34 + 22], 1.0);
    }

    #[test]
    fn channels_match_encode_cursor() {
        // The debug_assert in encode() guards drift; run it for both modes.
        let _ = sample(4).encode();
        let _ = sample(3).encode();
    }
}
