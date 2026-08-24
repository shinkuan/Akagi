//! Per-flow Riichi City game state mirror.
//!
//! Riichi City identifies players by numeric `user_id`. `player_list` holds
//! those ids in **mjai-actor order**: at `cmd_game_start` the enter-room order
//! is rotated left by `dealer_pos`, so index 0 is the first dealer (East-1 oya)
//! and our own seat is `player_list.index(uid)`. Every wire `user_id` is mapped
//! back to an actor index by position in this list.
//!
//! Sanma (3-player) differs from the v2 Python bridge: we keep `player_list`,
//! `scores`, and `tehais` at the native length 3 (no ghost-padding to 4), so
//! actor indices stay in `0..=2` — matching the Tenhou bridge and the rest of
//! the V3 sanma pipeline.

use crate::schema::MjaiEvent;

#[derive(Debug, Clone)]
pub struct GameStatus {
    /// Our own mjai-actor seat (index into `player_list`). Valid after
    /// `cmd_game_start`.
    pub seat: u8,
    /// Player `user_id`s in mjai-actor order (dealer-rotated). Length is 3 for
    /// sanma, 4 for yonma.
    pub player_list: Vec<i64>,
    /// 3-player game.
    pub is_3p: bool,
    /// 3 (sanma) or 4 (yonma). Controls output vector widths.
    pub num_players: u8,
    /// Dealer position at the first kyoku, used to derive `kyoku`/`oya` for
    /// later rounds: `(dealer_pos - shift) mod num_players`.
    pub shift: i64,
    /// Matchmaking classification id from `cmd_enter_room.options` (a wire
    /// string of ~22 base32-ish chars). Names the queue class — repeats
    /// across games in the same room tier. Persisted into `MatchInfo`.
    pub classify_id: Option<String>,
    /// Table-instance token from the `cmd_enter_room` wrapper (`room_id`).
    /// Unique per table, so it (not `classify_id`) keys the duplicate-
    /// `cmd_enter_room` dedup.
    pub room_id: Option<String>,
    /// `options.stage_type` — rank stage tier of the matchmaking room.
    pub stage_type: Option<i64>,
    /// `options.game_play` — game mode id (e.g. 1001).
    pub game_play: Option<i64>,
    /// `user_id → nickname` from `cmd_enter_room.players[].user`, used to
    /// stamp real display names onto `start_game`.
    pub nicknames: std::collections::HashMap<i64, String>,
    /// Actor of the most recent discard — the ron/pon/kan target.
    pub last_dahai_actor: Option<u8>,
    /// Seat currently holding a fresh draw (wall or rinshan), set by the
    /// draw broadcasts and cleared by that seat's discard. Gates the
    /// tsumogiri inference: a discard can only be tsumogiri on a turn
    /// that drew — a post-chi/pon discard never is.
    pub drawn_by: Option<u8>,
    /// Called melds per actor this kyoku (chi/pon/daiminkan/ankan; a
    /// kakan upgrades an existing pon and keeps the count). Sizes each
    /// seat's tile rack: holding a draw, it racks `13 - 3*melds + 1`
    /// tiles, and that last slot is the `move_cards_pos` index the
    /// client sends for a tsumogiri.
    pub meld_counts: [u8; 4],
    /// Deferred `reach_accepted`, emitted just before the next action so a
    /// chi/pon/kan on the riichi discard is ordered correctly.
    pub pending_reach: Option<MjaiEvent>,
    /// Kan-dora markers awaiting flush after the kan-caller's next discard.
    pub pending_dora: Vec<String>,
    /// True between `cmd_enter_room` and the first `cmd_game_start`, when
    /// `start_game` is still owed.
    pub game_start: bool,
    /// Whether the dealer rotation and our seat have been resolved. Set by
    /// the first `cmd_game_start`, or immediately on a mid-game reconnect
    /// (`cmd_enter_room` with `is_reconnect`, which carries the same
    /// `initial_dealer_pos` but never delivers a `cmd_game_start` for the
    /// running hand).
    pub seat_resolved: bool,
}

impl Default for GameStatus {
    fn default() -> Self {
        Self {
            seat: 0,
            player_list: Vec::new(),
            is_3p: false,
            num_players: 4,
            shift: 0,
            classify_id: None,
            room_id: None,
            stage_type: None,
            game_play: None,
            nicknames: std::collections::HashMap::new(),
            last_dahai_actor: None,
            drawn_by: None,
            meld_counts: [0; 4],
            pending_reach: None,
            pending_dora: Vec::new(),
            game_start: false,
            seat_resolved: false,
        }
    }
}

impl GameStatus {
    /// mjai-actor index for a wire `user_id`, or `None` if unknown.
    pub fn actor_of(&self, user_id: i64) -> Option<u8> {
        self.player_list
            .iter()
            .position(|&id| id == user_id)
            .map(|p| p as u8)
    }
}
