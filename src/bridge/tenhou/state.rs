//! Per-flow Tenhou game state mirror.
//!
//! Tenhou's wire format is 4-positional regardless of player count: seat 0 =
//! East, 1 = South, 2 = West, 3 = North. Sanma games leave wire-abs 3 as a
//! ghost slot (no real player). All references in the wire (`oya`,
//! `T<n>`/`D<n>` actors, `who`/`fromWho`, `ten[i]`) are *relative* to us, with
//! rel 0 being the observing player. We translate every reference through
//! [`State::rel_to_abs`] using 4-cycle arithmetic so the wire-abs mapping is
//! preserved across yonma/sanma without collisions.
//!
//! mjai exposes only real seats, so for sanma our wire-abs is also our mjai-abs
//! (always in `{0, 1, 2}`). Wire-abs 3 has no mjai counterpart — callers must
//! discard events that resolve to the ghost slot (or, in practice, never see
//! them: Tenhou doesn't emit play actions for a seat that has no player).

use super::meld::Meld;

#[derive(Debug, Clone)]
pub struct State {
    /// Our wire-absolute seat in 4-cycle (0=E, 1=S, 2=W, 3=N). For sanma this
    /// is always in `{0, 1, 2}` since we are a real player; the same value is
    /// our mjai-abs seat.
    pub seat: u8,
    /// Tenhou tile indices in our hand (other players' hands are not tracked).
    pub hand: Vec<u32>,
    /// Open melds we have called this kyoku.
    pub melds: Vec<Meld>,
    pub in_riichi: bool,
    /// Tiles remaining in the wall (counts down from 70 each kyoku).
    pub live_wall: u32,
    /// Last discard, stored as mjai string for ron-target attribution.
    pub last_kawa_tile: String,
    /// True between our tsumo and our dahai.
    pub is_tsumo: bool,
    /// True iff this is a 3-player (sanma) game.
    pub is_3p: bool,
    /// 3 (sanma) or 4 (yonma). The wire is always 4-cycle; this field only
    /// controls output sizing (mjai event field widths) and ghost detection.
    pub num_players: u8,
    /// Absolute seat that performed the most recent revealing action
    /// (discard / kakan / ankan), so a subsequent `<AGARI/>` can attribute
    /// the ron target correctly.
    pub last_revealed_tile_actor: Option<u8>,
    /// True once `<TAIKYOKU/>` has been seen and `start_game` is owed at the
    /// next `<INIT/>`. We defer emission so we know yonma vs sanma (via INIT's
    /// 0-score slot) and can stamp `num_players` correctly on `start_game`.
    pub pending_start_game: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            seat: 0,
            hand: Vec::new(),
            melds: Vec::new(),
            in_riichi: false,
            live_wall: 70,
            last_kawa_tile: "?".to_string(),
            is_tsumo: false,
            is_3p: false,
            num_players: 4,
            last_revealed_tile_actor: None,
            pending_start_game: false,
        }
    }
}

impl State {
    /// Convert wire-relative seat to wire-absolute seat. Always uses 4-cycle
    /// arithmetic because the Tenhou wire is 4-positional in both yonma and
    /// sanma — `(rel + seat) % num_players` would conflate wire-rel 0 (us)
    /// with wire-rel 3 (kamicha) for sanma.
    pub fn rel_to_abs(&self, rel: u8) -> u8 {
        (rel + self.seat) % 4
    }

    /// Inverse of [`rel_to_abs`].
    pub fn abs_to_rel(&self, abs: u8) -> u8 {
        (abs + 4 - self.seat) % 4
    }

    /// True if the given wire-abs seat is the sanma ghost slot (no real
    /// player). Callers receiving wire-abs from [`rel_to_abs`] should discard
    /// events / score entries that hit this position.
    pub fn is_ghost_abs(&self, abs: u8) -> bool {
        self.is_3p && abs == 3
    }

    /// Reset per-kyoku fields. Called from `INIT`.
    pub fn reset_for_kyoku(&mut self) {
        self.hand.clear();
        self.melds.clear();
        self.in_riichi = false;
        self.live_wall = 70;
        self.last_kawa_tile = "?".to_string();
        self.is_tsumo = false;
        self.last_revealed_tile_actor = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yonma_seat_round_trip() {
        let mut s = State::default();
        s.seat = 2;
        for abs in 0..4u8 {
            let rel = s.abs_to_rel(abs);
            assert_eq!(s.rel_to_abs(rel), abs);
        }
    }

    /// Sanma still uses 4-cycle arithmetic; the round trip covers all four
    /// wire positions including the ghost. Our seat is always real.
    #[test]
    fn sanma_seat_round_trip() {
        let mut s = State::default();
        s.num_players = 3;
        s.is_3p = true;
        s.seat = 1;
        for abs in 0..4u8 {
            let rel = s.abs_to_rel(abs);
            assert_eq!(s.rel_to_abs(rel), abs);
        }
        // Wire-abs 3 is the ghost slot for any sanma seating.
        assert!(s.is_ghost_abs(3));
        for abs in 0..3u8 {
            assert!(!s.is_ghost_abs(abs));
        }
    }

    #[test]
    fn yonma_has_no_ghost() {
        let s = State::default();
        for abs in 0..4u8 {
            assert!(!s.is_ghost_abs(abs));
        }
    }
}
