//! Hand-calibrated 16:9-normalised coordinates for Majsoul UI elements.
//!
//! Ported from `reference/majsoul/autoplay_majsoul.py:13-65`. The Python
//! file has years of production validation — DO NOT change these values
//! without screen-tested re-calibration.
//!
//! Coordinate system: the Majsoul canvas is laid out in a 16x9 logical
//! grid regardless of actual pixel resolution (letterboxed). To convert
//! a `(x_norm, y_norm)` here into CSS pixels, see
//! `crate::autoplay::context::CanvasRect::pixel`.

use riichienv_core::action::ActionType;

/// 14 hand-tile slots. Index 0..=12 is the closed hand; index 13 is the
/// drawn tile when one exists (offset by `TSUMO_SPACE`).
pub const TILES: [(f64, f64); 14] = [
    (2.231_25, 8.362_5),
    (3.021_875, 8.362_5),
    (3.812_5, 8.362_5),
    (4.603_125, 8.362_5),
    (5.393_75, 8.362_5),
    (6.184_375, 8.362_5),
    (6.975, 8.362_5),
    (7.765_625, 8.362_5),
    (8.556_25, 8.362_5),
    (9.346_875, 8.362_5),
    (10.137_5, 8.362_5),
    (10.928_125, 8.362_5),
    (11.718_75, 8.362_5),
    (12.509_375, 8.362_5),
];

/// Horizontal offset between the closed hand's last tile and the
/// just-drawn tsumohai.
pub const TSUMO_SPACE: f64 = 0.246_875;

/// Action button positions (3x3 grid). The first two rows (indices 0..=5)
/// are the only ones in use; rows 3 (indices 6..=8) are dead slots that
/// Majsoul has reserved but never showed in production.
pub const ACTIONS: [(f64, f64); 9] = [
    (10.875, 7.0),
    (8.637_5, 7.0),
    (6.4, 7.0),
    (10.875, 5.9),
    (8.637_5, 5.9),
    (6.4, 5.9),
    (10.875, 4.8),
    (8.637_5, 4.8),
    (6.4, 4.8),
];

/// Candidate-selection row for chi/pon disambiguation. Index formula
/// from the Python reference: `idx = int((-(len/2) + i + 0.5)*2 + 5)`.
pub const CANDIDATES: [(f64, f64); 11] = [
    (3.662_5, 6.3),
    (4.496_25, 6.3),
    (5.33, 6.3),
    (6.163_75, 6.3),
    (6.997_5, 6.3),
    (7.831_25, 6.3),
    (8.665, 6.3),
    (9.498_75, 6.3),
    (10.332_5, 6.3),
    (11.166_25, 6.3),
    (12.0, 6.3),
];

/// Candidate row for kan disambiguation (only 7 slots, formula offset
/// is `+3` instead of `+5`).
pub const CANDIDATES_KAN: [(f64, f64); 7] = [
    (4.325, 6.3),
    (5.491_5, 6.3),
    (6.658_3, 6.3),
    (7.825, 6.3),
    (8.991_7, 6.3),
    (10.158_3, 6.3),
    (11.325, 6.3),
];

/// Action priority for the 3x3 button grid. Lower priority renders
/// first (rightmost / bottom-rightmost). Source:
/// `autoplay_majsoul.py:67-81`. Indices align with `MajsoulOpType`.
pub const ACTION_PRIORITY: [u32; 12] = [
    0, // None       — pass / cancel button (always rightmost)
    99, // Discard   — no button
    4, // Chi
    3, // Pon
    3, // Ankan
    2, // Daiminkan
    3, // Kakan
    2, // Reach
    1, // Zimo (tsumo agari)
    1, // Ron
    5, // Ryukyoku
    4, // Nukidora
];

/// Numeric type codes used by the priority table above. These match the
/// values Majsoul's `OptionalOperation.type` field uses on the wire and
/// the legacy `ACTION2TYPE` table from the Python reference. Kept as a
/// `repr(u32)` so the table indexing is direct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum MajsoulOpType {
    None = 0,
    Discard = 1,
    Chi = 2,
    Pon = 3,
    Ankan = 4,
    Daiminkan = 5,
    Kakan = 6,
    Reach = 7,
    Zimo = 8,
    Ron = 9,
    Ryukyoku = 10,
    Nukidora = 11,
}

impl MajsoulOpType {
    /// Translate a riichi-engine `ActionType` into the Majsoul on-screen
    /// op-type. Returns `None` if the action type doesn't have a button
    /// representation (e.g. `Discard` is freeform tile click).
    ///
    /// Tsumo vs Ron: in mjai both are `Hora`, but Majsoul renders them
    /// in different button slots (Zimo on self-discard turn, Ron on
    /// opponent-discard turn). `riichienv_core::ActionType` distinguishes
    /// these via `Tsumo` and `Ron` so we map directly.
    pub fn from_engine(at: ActionType) -> Option<Self> {
        Some(match at {
            ActionType::Discard => Self::Discard,
            ActionType::Chi => Self::Chi,
            ActionType::Pon => Self::Pon,
            ActionType::Daiminkan => Self::Daiminkan,
            ActionType::Ankan => Self::Ankan,
            ActionType::Kakan => Self::Kakan,
            ActionType::Riichi => Self::Reach,
            ActionType::Tsumo => Self::Zimo,
            ActionType::Ron => Self::Ron,
            ActionType::KyushuKyuhai => Self::Ryukyoku,
            ActionType::Kita => Self::Nukidora,
            ActionType::Pass => Self::None,
        })
    }
}

/// Coordinate of a hand tile, accounting for the optional tsumohai gap.
///
/// `idx` is the position in the sorted closed hand 0..=13. `tehai_count`
/// is the number of closed (non-tsumohai) tiles. When `idx == 13`, the
/// returned point is offset by `TSUMO_SPACE` to land on the just-drawn
/// tile.
pub fn get_pai_coord(idx: usize, tehai_count: usize) -> (f64, f64) {
    debug_assert!(idx < TILES.len(), "tile index {idx} out of range");
    if idx == 13 {
        let base = TILES[tehai_count.min(TILES.len() - 1)];
        (base.0 + TSUMO_SPACE, base.1)
    } else {
        TILES[idx]
    }
}

/// Position of an action button, given the deduplicated set of Majsoul
/// op-types currently legal. The set is sorted by [`ACTION_PRIORITY`]
/// (ascending) and the index of `target` in that sorted list = the
/// button slot in [`ACTIONS`].
///
/// Returns `None` if `target` isn't in `ops` or the button position
/// would exceed the rendered grid.
pub fn action_button_pos(ops: &[MajsoulOpType], target: MajsoulOpType) -> Option<(f64, f64)> {
    let mut sorted: Vec<MajsoulOpType> = ops.to_vec();
    sorted.sort_by_key(|op| ACTION_PRIORITY[*op as usize]);
    let idx = sorted.iter().position(|&op| op == target)?;
    ACTIONS.get(idx).copied()
}

/// Candidate-selection slot for chi/pon. Reference formula:
/// `int((-(len/2) + idx + 0.5)*2 + 5)` → index into [`CANDIDATES`].
///
/// Returns `None` if the computed slot is out of [`CANDIDATES`] bounds
/// (defensive — should never happen with valid inputs).
pub fn candidate_pos(idx: usize, len: usize) -> Option<(f64, f64)> {
    if len == 0 {
        return None;
    }
    // Formula taken verbatim from Python reference, line 237:
    //   candidate_idx = int((-(len/2) + idx + 0.5) * 2 + 5)
    // The cast in Python truncates toward zero. Replicate with a
    // float-then-floor pattern that matches positive arguments.
    let raw = (-((len as f64) / 2.0) + idx as f64 + 0.5) * 2.0 + 5.0;
    let slot = raw as isize;
    if slot < 0 || (slot as usize) >= CANDIDATES.len() {
        return None;
    }
    Some(CANDIDATES[slot as usize])
}

/// Kan-candidate slot. Same formula but offset is `+3` (kan row only
/// has 7 slots).
pub fn kan_candidate_pos(idx: usize, len: usize) -> Option<(f64, f64)> {
    if len == 0 {
        return None;
    }
    let raw = (-((len as f64) / 2.0) + idx as f64 + 0.5) * 2.0 + 3.0;
    let slot = raw as isize;
    if slot < 0 || (slot as usize) >= CANDIDATES_KAN.len() {
        return None;
    }
    Some(CANDIDATES_KAN[slot as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pai_coord_closed_hand() {
        // First tile slot, hand of any size.
        assert_eq!(get_pai_coord(0, 13), TILES[0]);
        // Last closed-hand position.
        assert_eq!(get_pai_coord(12, 13), TILES[12]);
    }

    #[test]
    fn pai_coord_tsumohai_offset() {
        // For a 13-tile closed hand, tsumohai sits past slot 13 by TSUMO_SPACE.
        let (x, y) = get_pai_coord(13, 13);
        assert!((x - (TILES[13].0 + TSUMO_SPACE)).abs() < 1e-9);
        assert!((y - TILES[13].1).abs() < 1e-9);
    }

    #[test]
    fn pai_coord_tsumohai_after_chi() {
        // After chi, hand is 10 tiles + tsumohai. The tsumohai position
        // is past slot 10 by TSUMO_SPACE.
        let (x, _) = get_pai_coord(13, 10);
        assert!((x - (TILES[10].0 + TSUMO_SPACE)).abs() < 1e-9);
    }

    #[test]
    fn action_button_pos_rightmost_slot_zero() {
        // Sole "Pass" → goes in slot 0 (rightmost top row).
        let pos = action_button_pos(&[MajsoulOpType::None], MajsoulOpType::None).unwrap();
        assert_eq!(pos, ACTIONS[0]);
    }

    #[test]
    fn action_button_pos_priority_sort() {
        // Pass + Chi: priority 0 = Pass first (slot 0), priority 4 = Chi (slot 1).
        let ops = [MajsoulOpType::None, MajsoulOpType::Chi];
        let pass_pos = action_button_pos(&ops, MajsoulOpType::None).unwrap();
        let chi_pos = action_button_pos(&ops, MajsoulOpType::Chi).unwrap();
        assert_eq!(pass_pos, ACTIONS[0]);
        assert_eq!(chi_pos, ACTIONS[1]);
    }

    #[test]
    fn action_button_pos_three_buttons() {
        // Pass(0) + Pon(3) + Chi(4): sorted Pass, Pon, Chi → slots 0,1,2.
        let ops = [MajsoulOpType::None, MajsoulOpType::Pon, MajsoulOpType::Chi];
        assert_eq!(
            action_button_pos(&ops, MajsoulOpType::None).unwrap(),
            ACTIONS[0]
        );
        assert_eq!(
            action_button_pos(&ops, MajsoulOpType::Pon).unwrap(),
            ACTIONS[1]
        );
        assert_eq!(
            action_button_pos(&ops, MajsoulOpType::Chi).unwrap(),
            ACTIONS[2]
        );
    }

    #[test]
    fn action_button_pos_zimo_and_pass() {
        // Tsumo agari turn typically: Pass(0) + Zimo(1). Zimo gets slot 1.
        let ops = [MajsoulOpType::None, MajsoulOpType::Zimo];
        assert_eq!(
            action_button_pos(&ops, MajsoulOpType::Zimo).unwrap(),
            ACTIONS[1]
        );
    }

    #[test]
    fn action_button_pos_reach_with_pass() {
        // Reach(2) + Pass(0): Pass slot 0, Reach slot 1.
        let ops = [MajsoulOpType::None, MajsoulOpType::Reach];
        assert_eq!(
            action_button_pos(&ops, MajsoulOpType::Reach).unwrap(),
            ACTIONS[1]
        );
    }

    #[test]
    fn action_button_pos_missing() {
        // Asking for an op not in the legal set → None.
        assert!(action_button_pos(&[MajsoulOpType::None], MajsoulOpType::Chi).is_none());
    }

    #[test]
    fn candidate_pos_one_candidate() {
        // len=1 idx=0 → raw = (-0.5 + 0.5)*2 + 5 = 5 → middle slot.
        assert_eq!(candidate_pos(0, 1).unwrap(), CANDIDATES[5]);
    }

    #[test]
    fn candidate_pos_two_candidates() {
        // len=2 idx=0 → (-1 + 0.5)*2 + 5 = 4
        // len=2 idx=1 → (-1 + 1.5)*2 + 5 = 6
        assert_eq!(candidate_pos(0, 2).unwrap(), CANDIDATES[4]);
        assert_eq!(candidate_pos(1, 2).unwrap(), CANDIDATES[6]);
    }

    #[test]
    fn candidate_pos_three_candidates() {
        // len=3 idx=0 → (-1.5 + 0.5)*2 + 5 = 3
        // len=3 idx=1 → (-1.5 + 1.5)*2 + 5 = 5
        // len=3 idx=2 → (-1.5 + 2.5)*2 + 5 = 7
        assert_eq!(candidate_pos(0, 3).unwrap(), CANDIDATES[3]);
        assert_eq!(candidate_pos(1, 3).unwrap(), CANDIDATES[5]);
        assert_eq!(candidate_pos(2, 3).unwrap(), CANDIDATES[7]);
    }

    #[test]
    fn kan_candidate_pos_two() {
        // len=2 idx=0 → (-1 + 0.5)*2 + 3 = 2
        // len=2 idx=1 → (-1 + 1.5)*2 + 3 = 4
        assert_eq!(kan_candidate_pos(0, 2).unwrap(), CANDIDATES_KAN[2]);
        assert_eq!(kan_candidate_pos(1, 2).unwrap(), CANDIDATES_KAN[4]);
    }

    #[test]
    fn op_type_from_engine_action_types() {
        assert_eq!(MajsoulOpType::from_engine(ActionType::Riichi), Some(MajsoulOpType::Reach));
        assert_eq!(MajsoulOpType::from_engine(ActionType::Tsumo), Some(MajsoulOpType::Zimo));
        assert_eq!(MajsoulOpType::from_engine(ActionType::Ron), Some(MajsoulOpType::Ron));
        assert_eq!(MajsoulOpType::from_engine(ActionType::KyushuKyuhai), Some(MajsoulOpType::Ryukyoku));
        assert_eq!(MajsoulOpType::from_engine(ActionType::Pass), Some(MajsoulOpType::None));
    }
}
