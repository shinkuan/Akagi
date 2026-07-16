//! Auto-start / auto-continue configuration.
//!
//! Drives the `AutoStartManager`: queue Ranked matches, play a target
//! number of games, then stop; optionally override the room by the player's
//! coarse rank class. v1 covers Ranked only -- the from-lobby path is
//! Ranked -> tier -> mode (4P East / 4P South / 3P East / 3P South), and the
//! manager returns to the lobby after each game using a visual check on the
//! Ranked label.

use serde::{Deserialize, Serialize};

/// Which lobby category to queue in. v1 supports Ranked only; the enum
/// reserves room for Friendly / Tournament lobbies later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatchCategory {
    #[default]
    Ranked,
}

/// Four-player (Four) vs three-player (Three). Doubles as the "which rank to read" selector in
/// a [`RankRule`] -- 4p rank lives in `Account.level`, 3p in `Account.level3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlayerCount {
    #[default]
    Four,
    Three,
}

/// East-round (tonpuu) vs South-round (hanchan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RoundLength {
    East,
    #[default]
    South,
}

/// Ranked room tier: Bronze/Silver/Gold/Jade/Throne room. (Novice has no
/// selectable ranked room, so it is intentionally absent.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RoomTier {
    Bronze,
    Silver,
    #[default]
    Gold,
    Jade,
    Throne,
}

/// Coarse Majsoul rank class (major rank category), derived from `AccountLevel.id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RankMajor {
    Novice,
    Adept,
    Expert,
    Master,
    Saint,
    Celestial,
}

impl RankMajor {
    /// Majsoul encodes rank as a 4-digit `AccountLevel.id` = `NXXX`, where the
    /// thousands digit `N` is the major class: 1=Novice, 2=Adept, 3=Expert,
    /// 4=Master, 5=Saint; 6 and 7 both map to Celestial here to stay safe about which digit
    /// the live client uses. VERIFY against live `AccountLevel.id` values
    /// (surfaced once protocol rank-read lands) before trusting the boundaries.
    pub fn from_level_id(id: u32) -> Option<Self> {
        match id / 1000 {
            1 => Some(Self::Novice),
            2 => Some(Self::Adept),
            3 => Some(Self::Expert),
            4 => Some(Self::Master),
            5 => Some(Self::Saint),
            6 | 7 => Some(Self::Celestial),
            _ => None,
        }
    }
}

/// One rank-based room override. When the player's coarse rank class (for the
/// rule's `rank_kind` game type) equals `when_rank`, queue `tier`/`length`
/// instead of the base config. First matching rule (top-down) wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RankRule {
    /// Which rank to test -- 4-player (`Account.level`) or 3-player (`level3`) -- and
    /// therefore which game type this rule queues.
    pub rank_kind: PlayerCount,
    /// The coarse rank class this rule fires on.
    pub when_rank: RankMajor,
    /// Room tier to queue when the rule matches.
    pub tier: RoomTier,
    /// Round length to queue when the rule matches.
    pub length: RoundLength,
}

/// Resolved room to queue for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoomSelection {
    pub player_count: PlayerCount,
    pub round_length: RoundLength,
    pub tier: RoomTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoStartConfig {
    /// Lobby category. v1: Ranked only.
    pub category: MatchCategory,
    /// Base game type / room, used when no rank rule matches.
    pub player_count: PlayerCount,
    pub round_length: RoundLength,
    pub tier: RoomTier,
    /// Stop after this many finished games. `0` = unlimited.
    pub target_game_count: u32,
    /// Count only games our bot actually sat in (has a seat), not spectated or
    /// replayed ones.
    pub count_only_our_seat: bool,
    /// Use the visual (Ranked-label NCC) check to detect the lobby home before
    /// navigating. When `false`, the manager falls back to blindly pressing
    /// Confirm a fixed number of times then navigating (no screenshot).
    pub use_vision: bool,
    /// NCC similarity threshold above which the Ranked region is judged "lobby
    /// home". Live-measured separation is huge (lobby ~1.0, else <0.1), so the
    /// 0.65 default has wide margin.
    pub home_ncc_threshold: f32,
    /// Delay (ms) after a game ends before the return-to-lobby loop starts, to
    /// let the scoring animation finish and the first result screen appear.
    /// Too short and the first Confirm press lands mid-animation (visible
    /// flicker, a wasted press); raise this if you see it on slow machines.
    pub settle_delay_ms: u32,
    /// Delay (ms) between successive lobby-navigation clicks.
    pub inter_click_delay_ms: u32,
    /// Delay (ms) between Confirm presses while clearing result screens. Paced so a
    /// press lands after the previous result-screen animation, not during it.
    pub confirm_interval_ms: u32,
    /// Human-like pause (ms) after reaching the lobby, before clicking Ranked to
    /// queue the next game. Spaces games out so re-queues don't look robotic.
    pub inter_game_delay_ms: u32,
    /// How long (ms) to wait for the next game to actually begin (StartGame)
    /// after issuing the queue navigation, before retrying.
    pub matchmaking_timeout_ms: u32,
    /// Max return-to-lobby / re-queue attempts before giving up and notifying.
    pub max_attempts: u32,
    /// Auto-capture the lobby-home reference fingerprint at the moment the
    /// manager clicks Ranked to queue (it is necessarily at the lobby then), so
    /// the user doesn't have to run Calibrate Lobby Reference manually. Refreshes
    /// every queue, which self-heals across skin/version changes. Assumes Start
    /// was pressed
    /// while at the lobby (the first queue of a session trusts this).
    pub auto_calibrate_home: bool,
    /// Rank-based room overrides, first match wins. Empty = always use the base
    /// tier/length above. KEEP LAST: serialized as a TOML array-of-tables, which
    /// must follow all scalar keys in this section.
    pub rank_rules: Vec<RankRule>,
}

impl Default for AutoStartConfig {
    fn default() -> Self {
        Self {
            category: MatchCategory::Ranked,
            player_count: PlayerCount::Four,
            round_length: RoundLength::South,
            tier: RoomTier::Gold,
            target_game_count: 0,
            count_only_our_seat: true,
            use_vision: true,
            home_ncc_threshold: crate::autoplay::majsoul::vision::DEFAULT_HOME_NCC,
            settle_delay_ms: 8000,
            inter_click_delay_ms: 700,
            confirm_interval_ms: 2500,
            inter_game_delay_ms: 3000,
            matchmaking_timeout_ms: 180_000,
            max_attempts: 12,
            auto_calibrate_home: true,
            rank_rules: Vec::new(),
        }
    }
}

impl AutoStartConfig {
    /// Resolve which room to queue for. Applies the first rank rule whose
    /// `when_rank` matches the live rank for its `rank_kind`; otherwise falls
    /// back to the base `player_count`/`round_length`/`tier`. `rank_4p` /
    /// `rank_3p` are live `AccountLevel.id` values (`None` if not yet known).
    pub fn resolve_room(&self, rank_4p: Option<u32>, rank_3p: Option<u32>) -> RoomSelection {
        for rule in &self.rank_rules {
            let id = match rule.rank_kind {
                PlayerCount::Four => rank_4p,
                PlayerCount::Three => rank_3p,
            };
            if let Some(id) = id {
                if RankMajor::from_level_id(id) == Some(rule.when_rank) {
                    return RoomSelection {
                        player_count: rule.rank_kind,
                        round_length: rule.length,
                        tier: rule.tier,
                    };
                }
            }
        }
        RoomSelection {
            player_count: self.player_count,
            round_length: self.round_length,
            tier: self.tier,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_gold_south_four() {
        let c = AutoStartConfig::default();
        assert_eq!(c.player_count, PlayerCount::Four);
        assert_eq!(c.round_length, RoundLength::South);
        assert_eq!(c.tier, RoomTier::Gold);
        assert_eq!(c.target_game_count, 0);
        assert!(c.count_only_our_seat);
        assert!(c.use_vision);
    }

    #[test]
    fn rank_major_boundaries() {
        assert_eq!(RankMajor::from_level_id(1001), Some(RankMajor::Novice));
        assert_eq!(RankMajor::from_level_id(2003), Some(RankMajor::Adept));
        assert_eq!(RankMajor::from_level_id(3002), Some(RankMajor::Expert));
        assert_eq!(RankMajor::from_level_id(4001), Some(RankMajor::Master));
        assert_eq!(RankMajor::from_level_id(5003), Some(RankMajor::Saint));
        assert_eq!(RankMajor::from_level_id(7001), Some(RankMajor::Celestial));
        assert_eq!(RankMajor::from_level_id(0), None);
    }

    #[test]
    fn resolve_room_falls_back_to_base_with_no_rules() {
        let c = AutoStartConfig::default();
        let r = c.resolve_room(Some(4001), Some(2001));
        assert_eq!(r.player_count, PlayerCount::Four);
        assert_eq!(r.round_length, RoundLength::South);
        assert_eq!(r.tier, RoomTier::Gold);
    }

    #[test]
    fn resolve_room_applies_first_matching_rule() {
        let c = AutoStartConfig {
            rank_rules: vec![
                RankRule {
                    rank_kind: PlayerCount::Four,
                    when_rank: RankMajor::Saint,
                    tier: RoomTier::Jade,
                    length: RoundLength::South,
                },
                RankRule {
                    rank_kind: PlayerCount::Four,
                    when_rank: RankMajor::Master,
                    tier: RoomTier::Gold,
                    length: RoundLength::East,
                },
            ],
            ..Default::default()
        };
        // 4p rank 5xxx = Saint -> first rule -> Jade South.
        let r = c.resolve_room(Some(5002), None);
        assert_eq!(r.tier, RoomTier::Jade);
        assert_eq!(r.round_length, RoundLength::South);
        assert_eq!(r.player_count, PlayerCount::Four);
        // 4p rank 4xxx = Master -> second rule -> Gold East.
        let r2 = c.resolve_room(Some(4003), None);
        assert_eq!(r2.tier, RoomTier::Gold);
        assert_eq!(r2.round_length, RoundLength::East);
    }

    #[test]
    fn resolve_room_ignores_nonmatching_and_unknown_rank() {
        let c = AutoStartConfig {
            rank_rules: vec![RankRule {
                rank_kind: PlayerCount::Four,
                when_rank: RankMajor::Celestial,
                tier: RoomTier::Throne,
                length: RoundLength::South,
            }],
            ..Default::default()
        };
        // Master doesn't match a Celestial rule -> base (Gold).
        assert_eq!(c.resolve_room(Some(4001), None).tier, RoomTier::Gold);
        // Unknown rank id -> base.
        assert_eq!(c.resolve_room(Some(0), None).tier, RoomTier::Gold);
        // No rank known -> base.
        assert_eq!(c.resolve_room(None, None).tier, RoomTier::Gold);
    }

    #[test]
    fn toml_round_trip_with_rank_rule() {
        let c = AutoStartConfig {
            target_game_count: 10,
            rank_rules: vec![RankRule {
                rank_kind: PlayerCount::Three,
                when_rank: RankMajor::Expert,
                tier: RoomTier::Silver,
                length: RoundLength::East,
            }],
            ..Default::default()
        };
        let body = toml::to_string_pretty(&c).unwrap();
        assert!(
            body.contains("[[rank_rules]]"),
            "expected an array-of-tables in:\n{body}"
        );
        let back: AutoStartConfig = toml::from_str(&body).unwrap();
        assert_eq!(back.target_game_count, 10);
        assert_eq!(back.rank_rules.len(), 1);
        assert_eq!(back.rank_rules[0].rank_kind, PlayerCount::Three);
        assert_eq!(back.rank_rules[0].when_rank, RankMajor::Expert);
        assert_eq!(back.rank_rules[0].tier, RoomTier::Silver);
    }
}
