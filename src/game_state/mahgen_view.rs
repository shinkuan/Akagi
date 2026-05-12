//! Pre-encoded mahgen strings ready for the frontend `<mah-gen>` element.
//!
//! Mahjong rendering rules (chi/pon/kan layout, kakan with red dora,
//! ankan back-tile sandwich, river tedashi/tsumogiri/riichi markers) live
//! in Rust so every UI surface (HUD, replay viewer, future web panel)
//! consumes one canonical string format.
//!
//! See `claude_research_mahgen.md` for the full mahgen DSL reference.
//!
//! # Output shape
//!
//! [`MahgenView`] holds:
//!   - `players[4]` — per-seat hand + melds + river strings
//!   - `dora_indicators` — single string for the dora wall
//!
//! Hands for non-observer seats are rendered as `0z` × hand-size (tile
//! backs); the observer's hand uses real tile letters.

use serde::{Deserialize, Serialize};

use super::snapshot::{DiscardEntry, GameStateSnapshot, MeldKind, MeldSnapshot, PlayerSnapshot};

/// One player's view in the mahgen DSL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerMahgenView {
    pub seat: u8,
    /// Concealed hand. For the observer seat: real tiles grouped by suit.
    /// For other seats: `"0z" × n` (tile backs, n = closed-hand size).
    pub hand: String,
    /// One mahgen string per meld. Order matches [`PlayerSnapshot::melds`].
    pub melds: Vec<String>,
    /// River-mode mahgen string (use with `data-river-mode`).
    pub river: String,
}

/// Top-level mahgen view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MahgenView {
    /// One entry per seat. Length matches `num_players` (3 for sanma, 4 for yonma).
    pub players: Vec<PlayerMahgenView>,
    /// 3 (sanma) or 4 (yonma).
    #[serde(default = "default_num_players")]
    pub num_players: u8,
    /// Dora indicators — single concatenated string.
    pub dora_indicators: String,
}

fn default_num_players() -> u8 {
    4
}

impl MahgenView {
    pub fn from_snapshot(snap: &GameStateSnapshot) -> Self {
        let np = snap.num_players;
        let players: Vec<PlayerMahgenView> = snap
            .players
            .iter()
            .map(|p| PlayerMahgenView {
                seat: p.seat,
                hand: encode_hand(p, snap.our_seat),
                melds: p.melds.iter().map(|m| encode_meld(m, p.seat, np)).collect(),
                river: encode_river(&p.river),
            })
            .collect();
        Self {
            players,
            num_players: np,
            dora_indicators: encode_concat(&snap.dora_markers),
        }
    }
}

/// Translate a mjai tile string into mahgen notation.
///
/// Mapping: `5mr→0m`, `5pr→0p`, `5sr→0s`, `E→1z`, `S→2z`, `W→3z`,
/// `N→4z`, `P→5z`, `F→6z`, `C→7z`, `?→0z`. Numeric suit tiles pass
/// through unchanged.
fn mjai_to_mahgen(tile: &str) -> String {
    match tile {
        "5mr" => "0m".into(),
        "5pr" => "0p".into(),
        "5sr" => "0s".into(),
        "E" => "1z".into(),
        "S" => "2z".into(),
        "W" => "3z".into(),
        "N" => "4z".into(),
        "P" => "5z".into(),
        "F" => "6z".into(),
        "C" => "7z".into(),
        "?" => "0z".into(),
        other => other.to_string(),
    }
}

/// Concatenate mjai tile strings into compact mahgen form, grouped by suit.
///
/// Example: `["1m", "2m", "3m", "5pr", "P"]` → `"123m0p5z"`.
fn encode_concat(tiles: &[String]) -> String {
    // Bucket digits per suit, then emit `digits + suit-letter` per non-empty bucket.
    let mut man = String::new();
    let mut pin = String::new();
    let mut sou = String::new();
    let mut honor = String::new();
    for t in tiles {
        let m = mjai_to_mahgen(t);
        // `m` is `<digit><suit>`; suit always at index 1.
        let bytes = m.as_bytes();
        if bytes.len() != 2 {
            continue;
        }
        let digit = bytes[0] as char;
        match bytes[1] as char {
            'm' => man.push(digit),
            'p' => pin.push(digit),
            's' => sou.push(digit),
            'z' => honor.push(digit),
            _ => {}
        }
    }
    // Canonical-sort each bucket (digits ascending) so identical hands render
    // the same string regardless of input order.
    let sort_bucket = |s: &mut String| {
        let mut v: Vec<char> = s.chars().collect();
        v.sort();
        s.clear();
        s.extend(v);
    };
    sort_bucket(&mut man);
    sort_bucket(&mut pin);
    sort_bucket(&mut sou);
    sort_bucket(&mut honor);

    let mut out = String::new();
    if !man.is_empty() {
        out.push_str(&man);
        out.push('m');
    }
    if !pin.is_empty() {
        out.push_str(&pin);
        out.push('p');
    }
    if !sou.is_empty() {
        out.push_str(&sou);
        out.push('s');
    }
    if !honor.is_empty() {
        out.push_str(&honor);
        out.push('z');
    }
    out
}

/// Encode the closed hand for one player. The observer seat sees real
/// tiles; other seats see `"0z" × hand_size` (back tiles).
fn encode_hand(p: &PlayerSnapshot, our_seat: Option<u8>) -> String {
    let hand_size = p.tehai.len();
    if hand_size == 0 {
        return String::new();
    }
    if our_seat == Some(p.seat) {
        encode_concat(&p.tehai)
    } else {
        // n × `0z` — but mahgen wants `nz` form: `0` repeated, suffix `z`.
        // i.e. 13 backs = "0000000000000z". Each `0` is one tile back.
        let mut s = "0".repeat(hand_size);
        s.push('z');
        s
    }
}

/// Encode one meld in mahgen DSL.
///
/// `caller_seat` is the seat owning this meld (used to translate `from_who`
/// into a relative direction).
///
/// Conventions (per `reference/mahgen/README.md`):
///
/// - **Chi**: `_` at position determined by from_who. Hand tiles fill
///   remaining slots in ascending numerical order.
///     - kamicha → pos 1: `_213m`, `_312m`, `_123m`
///     - toimen  → pos 2: `1_32m` (e.g. 12m chi 3m from across)
///     - shimocha→ pos 3: `12_3m` (rare, only if mahgen used non-realistically)
/// - **Pon** (3 tiles): `_` at pos 1/2/3 = kamicha/toimen/shimocha.
///     - `_111s` / `1_11s` / `11_1s`
/// - **Daiminkan** (4 tiles): `_` at pos 1/2/4 = kamicha/toimen/shimocha
///   (note: pos 3 is skipped — shimocha sits at the **last** position).
///     - `_1111z` / `1_111z` / `111_1z`
/// - **Ankan**: `0z<digit><digit><suit>0z` — back tiles flanking inner pair.
/// - **Kakan**: take the original pon's structure and replace the `_<digit>`
///   slot with `^<digit>` (no red) or `v<digit>` (red involved). For red:
///   `v0` = bottom red + top normal, `v5` = bottom normal + top red.
fn encode_meld(m: &MeldSnapshot, caller_seat: u8, np: u8) -> String {
    match m.kind {
        MeldKind::Chi => encode_chi(m, caller_seat, np),
        MeldKind::Pon => encode_pon(m, caller_seat, np),
        MeldKind::Daiminkan => encode_daiminkan(m, caller_seat, np),
        MeldKind::Ankan => encode_ankan(m),
        MeldKind::Kakan => encode_kakan(m, caller_seat, np),
    }
}

/// Strip the suit char and red-flag from a single mjai tile string,
/// returning the digit char (`'0'`-`'9'`) and the suit char (`'m'`, etc).
fn split_tile(tile: &str) -> Option<(char, char)> {
    let m = mjai_to_mahgen(tile);
    let bytes = m.as_bytes();
    if bytes.len() != 2 {
        return None;
    }
    Some((bytes[0] as char, bytes[1] as char))
}

/// Where the discarder of a called tile sat relative to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallSide {
    Kamicha,  // upper / left → mahgen position 1
    Toimen,   // across       → mahgen position 2
    Shimocha, // lower / right → mahgen position 3 (pon) or 4 (daiminkan)
}

fn call_side(caller: u8, from_who: i8, np: u8) -> CallSide {
    if from_who < 0 {
        return CallSide::Kamicha; // shouldn't happen for opened melds; safe default
    }
    let from = (from_who as u8) % np;
    let diff = (from + np - caller) % np;
    match (np, diff) {
        // 4p: 3 seats other than self, three relative kinds.
        (4, 3) => CallSide::Kamicha,
        (4, 2) => CallSide::Toimen,
        (4, 1) => CallSide::Shimocha,
        // 3p: only kamicha and shimocha exist (no toimen — opposite seat
        // doesn't exist in a triangle).
        (3, 2) => CallSide::Kamicha,
        (3, 1) => CallSide::Shimocha,
        _ => CallSide::Kamicha,
    }
}

/// Pon position 1..=3 from the call side.
fn pon_pos(side: CallSide) -> usize {
    match side {
        CallSide::Kamicha => 0,
        CallSide::Toimen => 1,
        CallSide::Shimocha => 2,
    }
}

/// Daiminkan position 1..=4 from the call side. Note: shimocha → 4 (NOT 3).
fn daiminkan_pos(side: CallSide) -> usize {
    match side {
        CallSide::Kamicha => 0,
        CallSide::Toimen => 1,
        CallSide::Shimocha => 3,
    }
}

/// Chi position 1..=3 from the call side. Hand tiles fill the remaining
/// slots in ascending numerical order.
fn chi_pos(side: CallSide) -> usize {
    pon_pos(side)
}

fn encode_chi(m: &MeldSnapshot, caller_seat: u8, np: u8) -> String {
    let side = call_side(caller_seat, m.from_who, np);
    let pos = chi_pos(side);

    // Pull called digit + suit, plus the two non-called tiles' digits.
    let called = m.called_tile.clone().unwrap_or_default();
    let mut called_digit: char = '0';
    let mut suit: char = ' ';
    let mut hand_digits: Vec<char> = Vec::new();

    let mut called_taken = false;
    for t in &m.tiles {
        let Some((d, s)) = split_tile(t) else {
            continue;
        };
        suit = s;
        if !called_taken && t == &called {
            called_digit = d;
            called_taken = true;
        } else {
            hand_digits.push(d);
        }
    }
    hand_digits.sort();

    // Place called at `pos`; fill remaining slots with sorted hand digits.
    let mut slots: [Option<char>; 3] = [None; 3];
    slots[pos] = Some(called_digit);
    let mut hand_iter = hand_digits.into_iter();
    for slot in slots.iter_mut() {
        if slot.is_none() {
            *slot = hand_iter.next();
        }
    }

    let mut out = String::new();
    for (i, slot) in slots.iter().enumerate() {
        if let Some(d) = slot {
            if i == pos {
                out.push('_');
            }
            out.push(*d);
        }
    }
    if suit != ' ' {
        out.push(suit);
    }
    out
}

fn encode_pon(m: &MeldSnapshot, caller_seat: u8, np: u8) -> String {
    let side = call_side(caller_seat, m.from_who, np);
    let pos = pon_pos(side);
    let (digit, suit) = first_tile_digit_suit(m);

    let mut out = String::new();
    for i in 0..3 {
        if i == pos {
            out.push('_');
        }
        out.push(digit);
    }
    out.push(suit);
    out
}

fn encode_daiminkan(m: &MeldSnapshot, caller_seat: u8, np: u8) -> String {
    let side = call_side(caller_seat, m.from_who, np);
    let pos = daiminkan_pos(side);
    let (digit, suit) = first_tile_digit_suit(m);

    let mut out = String::new();
    for i in 0..4 {
        if i == pos {
            out.push('_');
        }
        out.push(digit);
    }
    out.push(suit);
    out
}

fn encode_ankan(m: &MeldSnapshot) -> String {
    let (digit, suit) = first_tile_digit_suit(m);
    // `0z<digit><digit><suit>0z` — back tiles flank the inner pair. mahgen
    // parses `0z`, `<digits><suit>`, `0z` as three adjacent sets.
    format!("0z{digit}{digit}{suit}0z")
}

/// Kakan encoding. Takes the original pon's structure (position from
/// `from_who`) and replaces the `_<digit>` slot with the kakan-stacked
/// pair. Red dora detection inspects whether the originally-rotated tile
/// (which becomes the stack's bottom) is red, and whether the newly-added
/// tile (stack's top) is red.
///
/// Heuristic for red detection: riichienv stores the kakan's tiles such
/// that the originally-claimed tile equals `m.called_tile` (the one that
/// was rotated during the pon). Any other red five in `m.tiles` came from
/// the original pon group's body OR was the kakan-added tile. For our
/// purposes (3-tile pon → 4-tile kakan) we cannot reliably distinguish
/// "added" from "original body" without explicit ordering. We approximate:
///   - If `called_tile` is red (`5xr`), bottom is red → `v0`.
///   - Else if any tile in `m.tiles` is red, top is red → `v5`.
///   - Else no red → `^`.
fn encode_kakan(m: &MeldSnapshot, caller_seat: u8, np: u8) -> String {
    let side = call_side(caller_seat, m.from_who, np);
    let pos = pon_pos(side);
    let (digit, suit) = first_tile_digit_suit(m);

    let called_is_aka = m
        .called_tile
        .as_deref()
        .map(|t| t.ends_with('r'))
        .unwrap_or(false);
    let any_aka = m.tiles.iter().any(|t| t.ends_with('r'));

    let (mark, stack_digit) = if called_is_aka {
        ('v', '0') // bottom red (aka), top normal
    } else if any_aka {
        ('v', '5') // bottom normal, top red
    } else {
        ('^', digit)
    };

    let mut out = String::new();
    for i in 0..3 {
        if i == pos {
            out.push(mark);
            out.push(stack_digit);
        } else {
            out.push(digit);
        }
    }
    out.push(suit);
    out
}

fn first_tile_digit_suit(m: &MeldSnapshot) -> (char, char) {
    m.tiles
        .first()
        .and_then(|t| split_tile(t))
        .unwrap_or(('0', 'z'))
}

/// Encode a river. Per-tile state markers:
///   - `_` → riichi-declaration tile
///   - `^` → tsumogiri (and not riichi)
///   - `v` → tsumogiri-while-riichi (rare: declared riichi by tsumogiri)
///   - (none) → manual cut, non-riichi
fn encode_river(river: &[DiscardEntry]) -> String {
    if river.is_empty() {
        return String::new();
    }
    // mahgen river mode forbids `|`. We emit one set per discard so the
    // parser sees `[state?]<digit><suit>` runs back-to-back.
    let mut out = String::new();
    for d in river {
        let prefix = match (d.is_riichi, d.tedashi) {
            (true, true) => "_",   // declared riichi via tedashi
            (true, false) => "v",  // declared riichi via tsumogiri
            (false, false) => "^", // tsumogiri, not riichi
            (false, true) => "",   // tedashi, not riichi
        };
        if let Some((digit, suit)) = split_tile(&d.tile) {
            out.push_str(prefix);
            out.push(digit);
            out.push(suit);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pl(
        seat: u8,
        tehai: Vec<&str>,
        melds: Vec<MeldSnapshot>,
        river: Vec<DiscardEntry>,
    ) -> PlayerSnapshot {
        PlayerSnapshot {
            seat,
            tehai: tehai.iter().map(|s| (*s).to_string()).collect(),
            melds,
            river,
            score: 25_000,
            riichi_declared: false,
            riichi_stage: false,
            double_riichi: false,
            riichi_declaration_index: None,
            kita_tiles: vec![],
            drawn_tile: None,
        }
    }

    fn discard(tile: &str, tedashi: bool, is_riichi: bool) -> DiscardEntry {
        DiscardEntry {
            tile: tile.into(),
            tedashi,
            is_riichi,
        }
    }

    #[test]
    fn mjai_translation_table() {
        assert_eq!(mjai_to_mahgen("1m"), "1m");
        assert_eq!(mjai_to_mahgen("5mr"), "0m");
        assert_eq!(mjai_to_mahgen("5pr"), "0p");
        assert_eq!(mjai_to_mahgen("5sr"), "0s");
        assert_eq!(mjai_to_mahgen("E"), "1z");
        assert_eq!(mjai_to_mahgen("S"), "2z");
        assert_eq!(mjai_to_mahgen("W"), "3z");
        assert_eq!(mjai_to_mahgen("N"), "4z");
        assert_eq!(mjai_to_mahgen("P"), "5z");
        assert_eq!(mjai_to_mahgen("F"), "6z");
        assert_eq!(mjai_to_mahgen("C"), "7z");
        assert_eq!(mjai_to_mahgen("?"), "0z");
    }

    #[test]
    fn hand_encoding_groups_by_suit() {
        let tiles: Vec<String> = ["1m", "2m", "3m", "5pr", "P", "E", "9s", "9s"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(encode_concat(&tiles), "123m0p99s15z");
    }

    #[test]
    fn opponent_hand_renders_as_backs() {
        let p = pl(1, vec!["?"; 13], vec![], vec![]);
        assert_eq!(encode_hand(&p, Some(0)), "0000000000000z");
    }

    #[test]
    fn observer_hand_renders_real_tiles() {
        let tiles: Vec<&str> = vec![
            "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "1s",
        ];
        let p = pl(0, tiles, vec![], vec![]);
        assert_eq!(encode_hand(&p, Some(0)), "123456789m123p1s");
    }

    fn chi_meld(called: &str, hand: &[&str], from_who: i8) -> MeldSnapshot {
        let mut tiles: Vec<String> = hand.iter().map(|s| (*s).into()).collect();
        tiles.push(called.into());
        MeldSnapshot {
            kind: MeldKind::Chi,
            tiles,
            from_who,
            called_tile: Some(called.into()),
        }
    }

    #[test]
    fn chi_kamicha_called_tile_at_pos_1() {
        // Caller=seat 0 (E), from=seat 3 (N) → kamicha. Hand 2m+3m, called 1m.
        let m = chi_meld("1m", &["2m", "3m"], 3);
        assert_eq!(encode_meld(&m, 0, 4), "_123m");

        // Hand 1m+2m, called 3m from kamicha.
        let m = chi_meld("3m", &["1m", "2m"], 3);
        assert_eq!(encode_meld(&m, 0, 4), "_312m");

        // Hand 1m+3m, called 2m from kamicha.
        let m = chi_meld("2m", &["1m", "3m"], 3);
        assert_eq!(encode_meld(&m, 0, 4), "_213m");
    }

    #[test]
    fn chi_toimen_called_tile_at_pos_2() {
        // Caller=seat 0, from=seat 2 → toimen. Hand 1m+2m, called 3m.
        // User's example: `1_32m`.
        let m = chi_meld("3m", &["1m", "2m"], 2);
        assert_eq!(encode_meld(&m, 0, 4), "1_32m");
    }

    #[test]
    fn pon_position_per_call_side() {
        // Caller=seat 0. from_who=3 (kamicha) → pos 1.
        let m = MeldSnapshot {
            kind: MeldKind::Pon,
            tiles: vec!["1z".into(), "1z".into(), "1z".into()],
            from_who: 3,
            called_tile: Some("1z".into()),
        };
        assert_eq!(encode_meld(&m, 0, 4), "_111z");

        // Toimen → pos 2.
        let m2 = MeldSnapshot {
            from_who: 2,
            ..m.clone()
        };
        assert_eq!(encode_meld(&m2, 0, 4), "1_11z");

        // Shimocha → pos 3.
        let m3 = MeldSnapshot {
            from_who: 1,
            ..m.clone()
        };
        assert_eq!(encode_meld(&m3, 0, 4), "11_1z");
    }

    #[test]
    fn daiminkan_shimocha_at_pos_4_not_3() {
        // Caller=seat 0, from=seat 1 → shimocha. Per mahgen, daiminkan
        // shimocha sits at the LAST position (pos 4), skipping pos 3.
        let m = MeldSnapshot {
            kind: MeldKind::Daiminkan,
            tiles: vec!["1z".into(); 4],
            from_who: 1,
            called_tile: Some("1z".into()),
        };
        assert_eq!(encode_meld(&m, 0, 4), "111_1z");

        let toimen = MeldSnapshot {
            from_who: 2,
            ..m.clone()
        };
        assert_eq!(encode_meld(&toimen, 0, 4), "1_111z");

        let kamicha = MeldSnapshot {
            from_who: 3,
            ..m.clone()
        };
        assert_eq!(encode_meld(&kamicha, 0, 4), "_1111z");
    }

    #[test]
    fn ankan_sandwich() {
        let m = MeldSnapshot {
            kind: MeldKind::Ankan,
            tiles: vec!["1p".into(); 4],
            from_who: -1,
            called_tile: None,
        };
        assert_eq!(encode_meld(&m, 0, 4), "0z11p0z");
    }

    #[test]
    fn kakan_position_inherits_pon_position() {
        // Caller=seat 0, original pon called from toimen (from_who=2) → pos 2.
        // Kakan structure: `5^5s` family with `^` at pos 2.
        let m = MeldSnapshot {
            kind: MeldKind::Kakan,
            tiles: vec!["5s".into(); 4],
            from_who: 2,
            called_tile: Some("5s".into()),
        };
        assert_eq!(encode_meld(&m, 0, 4), "5^55s");
    }

    #[test]
    fn kakan_red_called_tile_uses_v0() {
        // Pon called the red 5s from toimen (so the rotated tile was the red).
        // After kakan, `v0` indicates bottom=red, top=normal.
        let m = MeldSnapshot {
            kind: MeldKind::Kakan,
            tiles: vec!["5s".into(), "5sr".into(), "5s".into(), "5s".into()],
            from_who: 2,
            called_tile: Some("5sr".into()),
        };
        assert_eq!(encode_meld(&m, 0, 4), "5v05s");
    }

    #[test]
    fn kakan_red_added_tile_uses_v5() {
        // Pon called a normal 5p from kamicha (pos 1); kakan added the red.
        // `v5` indicates bottom=normal, top=red.
        let m = MeldSnapshot {
            kind: MeldKind::Kakan,
            tiles: vec!["5p".into(), "5p".into(), "5p".into(), "5pr".into()],
            from_who: 3,
            called_tile: Some("5p".into()),
        };
        assert_eq!(encode_meld(&m, 0, 4), "v555p");
    }

    #[test]
    fn kakan_no_red_uses_caret() {
        let m = MeldSnapshot {
            kind: MeldKind::Kakan,
            tiles: vec!["5p".into(); 4],
            from_who: 3,
            called_tile: Some("5p".into()),
        };
        assert_eq!(encode_meld(&m, 0, 4), "^555p");
    }

    #[test]
    fn river_markers_per_state() {
        let r = vec![
            discard("1m", true, false),  // tedashi
            discard("2m", false, false), // tsumogiri
            discard("3m", true, true),   // riichi via tedashi
            discard("4m", false, false), // tsumogiri
        ];
        assert_eq!(encode_river(&r), "1m^2m_3m^4m");
    }

    #[test]
    fn river_riichi_via_tsumogiri_uses_v() {
        let r = vec![discard("E", false, true)];
        assert_eq!(encode_river(&r), "v1z");
    }
}
