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
    /// True once this flow knows our seat — from `<TAIKYOKU/>`, or failing
    /// that from the first kyoku frame (`<INIT/>` / `<REINIT/>`), whose
    /// `seed`/`oya` pair pins it down (see [`seat_from_kyoku`]). A flow that
    /// starts mid-game (reconnect, late attach) never sees `<TAIKYOKU/>`.
    pub seat_resolved: bool,
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
    /// True while `start_game` is owed at the next `<INIT/>`. Set by
    /// `<TAIKYOKU/>` and `<REINIT/>`, and true from the start so a flow that
    /// first sees the game mid-way (a reconnect that skips `<TAIKYOKU/>`, or
    /// Akagi attaching between hands) still opens the game for the tracker.
    /// Emission is deferred to `<INIT/>` so we know yonma vs sanma (via the
    /// 0-score slot) and can stamp `num_players` correctly on `start_game`.
    pub pending_start_game: bool,
    /// True between a `<REINIT/>` and the next `<INIT/>`: this flow attached
    /// to a kyoku already in progress and has no event history for it, so
    /// nothing it parses can be turned into a coherent mjai stream until the
    /// next kyoku starts. Handlers still run (autoplay reads the hand and
    /// window from here) but their events are dropped — see
    /// `TenhouBridge::dispatch`.
    pub suspended: bool,
    /// True from a `<REINIT/>` until the next game starts: this flow never
    /// saw the hands before the rejoin, so the game's record is incomplete
    /// and its `end_game` goes out as terminated (History drops it rather
    /// than filing the remainder as a complete game with its own stats).
    pub rejoined_mid_game: bool,
    /// `<GO type=…/>` rule/room bitfield (room tier in bits 0x20/0x80),
    /// stashed for `start_game` emission. Read non-destructively so a
    /// reconnect's re-emitted `start_game` (TAIKYOKU+INIT with no fresh
    /// `<GO/>`) keeps the room; every new game sends its own `<GO/>`.
    pub go_type: Option<u32>,
    /// `<GO lobby=…/>` lobby number, stashed like `go_type`.
    pub lobby: Option<u32>,
    /// `<TAIKYOKU log=…/>` paifu id; reassigned by every `<TAIKYOKU/>`.
    pub log_id: Option<String>,
    /// `<UN/>` roster names, percent-decoded, in wire-*relative* order
    /// (index 0 = us). `<UN/>` arrives before `<TAIKYOKU/>` resolves our
    /// seat, so the remap to wire-absolute happens at `start_game` emission.
    pub un_names: Option<[String; 4]>,
    /// Decision window currently open for *our* seat, if any. Only consumed by
    /// autoplay (see [`crate::autoplay::tenhou_state`]); parsing keeps it up to
    /// date unconditionally because it is cheap and a stale window is worse
    /// than none.
    pub window: Option<crate::autoplay::tenhou_state::DecisionWindow>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            seat: 0,
            seat_resolved: false,
            hand: Vec::new(),
            melds: Vec::new(),
            in_riichi: false,
            live_wall: 70,
            last_kawa_tile: "?".to_string(),
            is_tsumo: false,
            is_3p: false,
            num_players: 4,
            last_revealed_tile_actor: None,
            pending_start_game: true,
            suspended: false,
            rejoined_mid_game: false,
            go_type: None,
            lobby: None,
            log_id: None,
            un_names: None,
            window: None,
        }
    }
}

/// `<GO type=…/>` bit for a three-player (sanma) table.
pub const GO_TYPE_SANMA: u32 = 0x10;

/// Our wire-absolute seat from a kyoku frame (`<INIT/>` / `<REINIT/>`).
///
/// Wire-abs 0 is the player who dealt E1, and the deal rotates 0→1→2→3, so
/// the dealer of kyoku index `k` (`seed[0]`: `bakaze * 4 + kyoku - 1`) sits
/// at wire-abs `k % 4`. `oya` on the same frame is that dealer's seat
/// *relative* to us, which leaves exactly one possibility for ours. Sanma
/// keeps the 4-cycle (`seed[0]` skips the ghost's turn: E1 E2 E3 S1 are
/// 0 1 2 4), so the same formula holds and never lands on the ghost.
pub fn seat_from_kyoku(kyoku_index: i32, oya_rel: u8) -> u8 {
    let oya_abs = kyoku_index.rem_euclid(4) as u8;
    (oya_abs + 4 - oya_rel % 4) % 4
}

/// Player count from a kyoku frame's scores. Tenhou deals 25000 × 4 =
/// 100000 points in yonma and 35000 × 3 = 105000 in sanma, and points only
/// move between the players and the riichi sticks on the table (a bust ends
/// the game), so the total names the player count on any frame, not just
/// the first deal. `ten` is in points (already × 100); `kyotaku` is the
/// stick count from `seed[2]`. `None` for a total that is neither.
pub fn three_players_from_scores(ten: &[i32], kyotaku: u8) -> Option<bool> {
    let total = ten.iter().sum::<i32>() + 1000 * i32::from(kyotaku);
    match total {
        100_000 => Some(false),
        105_000 => Some(true),
        _ => None,
    }
}

impl State {
    /// Set the player count. `<GO/>`, `<UN/>`, `<TAIKYOKU/>` and the E1H0
    /// `<INIT/>` all feed this; the last one wins, and E1H0 is authoritative.
    pub fn set_three_players(&mut self, three: bool) {
        self.is_3p = three;
        self.num_players = if three { 3 } else { 4 };
    }

    /// Player count implied by the session frames seen on this flow, before
    /// any kyoku frame: `<GO/>`'s sanma bit, else a `<UN/>` roster with
    /// exactly one empty slot (the ghost) or none. `None` when neither was
    /// seen (a flow attached mid-game) or the roster is not readable — our
    /// own name (`n0`) is never blank, so a blank one is not a roster.
    pub fn three_players_hint(&self) -> Option<bool> {
        if let Some(t) = self.go_type {
            return Some(t & GO_TYPE_SANMA != 0);
        }
        let names = self.un_names.as_ref()?;
        if names[0].is_empty() {
            return None;
        }
        match names.iter().filter(|n| n.is_empty()).count() {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

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

    /// Borrow the parts an encode needs (see
    /// [`super::encode::encode`]).
    pub fn hand_view(&self) -> super::encode::HandView<'_> {
        super::encode::HandView {
            hand: &self.hand,
            melds: &self.melds,
            is_tsumo: self.is_tsumo,
        }
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
        self.window = None;
    }

    /// Open a decision window for our seat with the server's `t` bitmask.
    pub fn open_window(&mut self, ops: u32) {
        self.window = Some(crate::autoplay::tenhou_state::DecisionWindow {
            ops,
            opened_at: std::time::Instant::now(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yonma_seat_round_trip() {
        let s = State {
            seat: 2,
            ..Default::default()
        };
        for abs in 0..4u8 {
            let rel = s.abs_to_rel(abs);
            assert_eq!(s.rel_to_abs(rel), abs);
        }
    }

    /// Sanma still uses 4-cycle arithmetic; the round trip covers all four
    /// wire positions including the ghost. Our seat is always real.
    #[test]
    fn sanma_seat_round_trip() {
        let s = State {
            num_players: 3,
            is_3p: true,
            seat: 1,
            ..Default::default()
        };
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

    /// The dealer of kyoku index `k` sits at wire-abs `k % 4`; `oya` is that
    /// seat relative to us. Every combination round-trips through the
    /// TAIKYOKU formula (`seat = (4 - oya_rel_at_e1) % 4`).
    #[test]
    fn seat_from_kyoku_matches_taikyoku_derivation() {
        for seat in 0..4u8 {
            let s = State {
                seat,
                ..Default::default()
            };
            for kyoku_index in 0..16i32 {
                let oya_abs = (kyoku_index % 4) as u8;
                let oya_rel = s.abs_to_rel(oya_abs);
                assert_eq!(seat_from_kyoku(kyoku_index, oya_rel), seat);
            }
        }
        // E1: oya rel 1 → we sit at wire-abs 3, same as TAIKYOKU oya="1".
        assert_eq!(seat_from_kyoku(0, 1), 3);
        // S2 (index 5, dealer wire-abs 1) seen as our toimen → we are 3.
        assert_eq!(seat_from_kyoku(5, 2), 3);
    }

    /// Sanma skips the ghost's deal, so its kyoku indices are 0 1 2 4 5 6 …
    /// and the derivation never resolves a real player onto wire-abs 3.
    #[test]
    fn seat_from_kyoku_sanma_never_hits_the_ghost() {
        for seat in 0..3u8 {
            let s = State {
                seat,
                is_3p: true,
                num_players: 3,
                ..Default::default()
            };
            for kyoku_index in [0, 1, 2, 4, 5, 6, 8, 9, 10] {
                let oya_rel = s.abs_to_rel((kyoku_index % 4) as u8);
                let derived = seat_from_kyoku(kyoku_index, oya_rel);
                assert_eq!(derived, seat);
                assert!(!s.is_ghost_abs(derived));
            }
        }
    }

    #[test]
    fn three_players_hint_prefers_go_over_roster() {
        let mut s = State::default();
        assert_eq!(s.three_players_hint(), None, "nothing seen yet");
        s.un_names = Some(["a".into(), "".into(), "b".into(), "c".into()]);
        assert_eq!(s.three_players_hint(), Some(true), "one empty roster slot");
        s.un_names = Some(["a".into(), "b".into(), "c".into(), "d".into()]);
        assert_eq!(s.three_players_hint(), Some(false));
        s.go_type = Some(GO_TYPE_SANMA | 0x09);
        assert_eq!(s.three_players_hint(), Some(true), "GO bit wins");
        s.go_type = Some(0x09);
        assert_eq!(s.three_players_hint(), Some(false));
        // A blank own name is not a roster; two blanks is not a table.
        s.go_type = None;
        s.un_names = Some(["".into(), "".into(), "b".into(), "c".into()]);
        assert_eq!(s.three_players_hint(), None);
        s.un_names = Some(["a".into(), "".into(), "".into(), "c".into()]);
        assert_eq!(s.three_players_hint(), None);
    }

    /// 100000 points on the table (players + sticks) is yonma, 105000 is
    /// sanma, on any deal.
    #[test]
    fn three_players_from_scores_reads_the_table_total() {
        assert_eq!(three_players_from_scores(&[25_000; 4], 0), Some(false));
        assert_eq!(
            three_players_from_scores(&[35_000, 35_000, 35_000, 0], 0),
            Some(true)
        );
        assert_eq!(
            three_players_from_scores(&[35_000, 35_000, 35_000], 0),
            Some(true)
        );
        // Mid-game, two riichi sticks out.
        assert_eq!(
            three_players_from_scores(&[18_000, 31_000, 24_000, 25_000], 2),
            Some(false)
        );
        assert_eq!(
            three_players_from_scores(&[40_000, 30_000, 34_000, 0], 1),
            Some(true)
        );
        assert_eq!(three_players_from_scores(&[30_000; 4], 0), None);
    }

    #[test]
    fn yonma_has_no_ghost() {
        let s = State::default();
        for abs in 0..4u8 {
            assert!(!s.is_ghost_abs(abs));
        }
    }
}
