//! Hand-calibrated 16:9-normalised LOBBY coordinates for Majsoul auto-start,
//! plus the room-selection -> coordinate mapping. This module is the MAJSOUL
//! side of the auto-start platform seam: everything Majsoul-specific the
//! platform-agnostic manager (`crate::autostart`) needs to drive the lobby
//! lives here (coordinates, scroll parameters, the visual home anchor).
//!
//! Measured by the user on a live client via the calibration overlay
//! (`LobbyCalibrationCard` -> `lobby_calibration_overlay`). Same 16:9
//! convention as the in-game `coords.rs`; convert to CSS pixels via
//! [`crate::autoplay::context::CanvasRect::pixel`].
//!
//! The full queue path (Ranked -> tier -> mode) and the post-game return via
//! Confirm are wired into `crate::autostart` and verified on a live client.
//! Re-measure with the calibration overlay whenever the Majsoul client
//! layout changes (same caveat as `coords.rs`).

use crate::config::{PlayerCount, RoomTier, RoundLength};
use std::time::Duration;

/// Ranked entry button on the lobby home.
pub const RANKED_ENTRY: (f64, f64) = (11.4, 2.6);

/// Visual anchor for the return-to-lobby check: the bounding box of the
/// static Ranked label graphic (gold calligraphy on blue) as
/// `(x_center, y_center, width, height)` in 16:9 units. Only appears on the
/// lobby home and never animates, unlike the player character on the left.
/// Measured from the label's corners: top-left (9.905, 2.224), bottom-right
/// (12.693, 3.205) -> centre (11.299, 2.714), size 2.788 x 0.981.
pub const HOME_ANCHOR: (f64, f64, f64, f64) = (11.299, 2.714, 2.788, 0.981);

/// Room-tier buttons in the Ranked list. Bronze/Silver/Gold are visible
/// without scrolling; Jade/Throne sit below the fold and were measured AFTER
/// scrolling the tier list to the bottom (hence their non-monotonic y vs Gold).
/// To click Jade/Throne, scroll the list at [`TIER_LIST_SCROLL_AT`] to the
/// bottom first.
pub const TIER_BRONZE: (f64, f64) = (11.6, 3.4); // Bronze room (no scroll)
pub const TIER_SILVER: (f64, f64) = (11.6, 4.9); // Silver room (no scroll)
pub const TIER_GOLD: (f64, f64) = (11.6, 6.3); // Gold room   (no scroll)
pub const TIER_JADE: (f64, f64) = (11.6, 5.3); // Jade room   (after scroll-to-bottom)
pub const TIER_THRONE: (f64, f64) = (11.6, 6.7); // Throne room (after scroll-to-bottom)

/// A point inside the tier list to scroll (wheel/drag) before clicking the
/// below-the-fold tiers (Jade/Throne).
pub const TIER_LIST_SCROLL_AT: (f64, f64) = (11.6, 5.0);

/// Mode buttons (4P/3P x East/South) shown in the SAME right-column panel as
/// the tiers, one step later (the two lists reuse the same slot coordinates).
/// There is no separate "Start matchmaking" button -- picking a mode starts
/// matchmaking directly. Step order Ranked -> tier -> mode is confirmed on a
/// live client.
pub const MODE_4P_EAST: (f64, f64) = (11.6, 3.4); // 4P East
pub const MODE_4P_SOUTH: (f64, f64) = (11.6, 4.9); // 4P South
pub const MODE_3P_EAST: (f64, f64) = (11.6, 6.3); // 3P East
pub const MODE_3P_SOUTH: (f64, f64) = (11.6, 7.3); // 3P South

/// Post-game result screens: the bottom-right Confirm button advances one
/// screen. The number of screens (rank pt / rank change / rewards) VARIES, so
/// the manager loops on it (with a vision check per press) rather than pressing
/// a fixed count. (Play Again exists at (12.0, 8.2) but is deliberately unused:
/// it locks the same room and bypasses the lobby fingerprint refresh.)
pub const RESULT_CONFIRM: (f64, f64) = (14.5, 8.2); // Confirm

/// Wheel ticks (and their pacing) used to scroll the tier list to the bottom
/// so Jade/Throne become clickable.
pub const TIER_SCROLL_TICKS: u32 = 8;
pub const TIER_SCROLL_DELTA: f64 = 300.0;
pub const TIER_SCROLL_INTERVAL: Duration = Duration::from_millis(60);

/// Tier button coordinate + whether the tier list must be scrolled to the
/// bottom first (Jade/Throne sit below the fold).
pub fn tier_target(t: RoomTier) -> ((f64, f64), bool) {
    match t {
        RoomTier::Bronze => (TIER_BRONZE, false),
        RoomTier::Silver => (TIER_SILVER, false),
        RoomTier::Gold => (TIER_GOLD, false),
        RoomTier::Jade => (TIER_JADE, true),
        RoomTier::Throne => (TIER_THRONE, true),
    }
}

/// Mode button coordinate (4P/3P x East/South).
pub fn mode_target(pc: PlayerCount, rl: RoundLength) -> (f64, f64) {
    match (pc, rl) {
        (PlayerCount::Four, RoundLength::East) => MODE_4P_EAST,
        (PlayerCount::Four, RoundLength::South) => MODE_4P_SOUTH,
        (PlayerCount::Three, RoundLength::East) => MODE_3P_EAST,
        (PlayerCount::Three, RoundLength::South) => MODE_3P_SOUTH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_scroll_only_for_jade_and_throne() {
        assert!(!tier_target(RoomTier::Bronze).1);
        assert!(!tier_target(RoomTier::Silver).1);
        assert!(!tier_target(RoomTier::Gold).1);
        assert!(tier_target(RoomTier::Jade).1);
        assert!(tier_target(RoomTier::Throne).1);
    }

    #[test]
    fn tier_targets_map_to_coords() {
        assert_eq!(tier_target(RoomTier::Gold).0, TIER_GOLD);
        assert_eq!(tier_target(RoomTier::Throne).0, TIER_THRONE);
    }

    #[test]
    fn mode_targets_cover_all_four() {
        assert_eq!(
            mode_target(PlayerCount::Four, RoundLength::East),
            MODE_4P_EAST
        );
        assert_eq!(
            mode_target(PlayerCount::Four, RoundLength::South),
            MODE_4P_SOUTH
        );
        assert_eq!(
            mode_target(PlayerCount::Three, RoundLength::East),
            MODE_3P_EAST
        );
        assert_eq!(
            mode_target(PlayerCount::Three, RoundLength::South),
            MODE_3P_SOUTH
        );
    }
}
