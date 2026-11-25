//! HoloMahjong tile notation → mjai.
//!
//! The server uses the notation of kobalab's majiang-core
//! (<https://github.com/kobalab/majiang-core>): a tile is a suit letter
//! (`m`/`p`/`s`/`z`) plus a digit, `0` being the red five, so `"m0"` is
//! mjai `"5mr"` and `"z5"` is `"P"`. A hidden tile is `x1`. Around that:
//!
//! - **discard** — `"m3"`, with `_` for a tsumogiri and `*` (riichi) or `@`
//!   (open riichi) for a declaration (`"z3_"`, `"m3*"`, `"m3_@"`). A kita
//!   tile is marked the same way (`"z4_"`).
//! - **hand** — either one suit letter per tile (`"s2z5m4…"`, the deal) or
//!   one letter per run of digits (`"m2345677p678s123*"`, majiang-core's
//!   `Shoupai` string); `*`/`@` mark riichi, `_` stands for a hidden tile,
//!   and melds and kita tiles follow after commas.
//! - **meld** — the called tile's digit is followed by where it came from,
//!   relative to the caller: `+` the next seat, `=` the opposite seat, `-`
//!   the previous seat. `"p1-23"` chi, `"z444+"` pon, `"m5555="` open kan,
//!   `"m555=5"` added kan (the trailing digit is the added tile), `"m5555"`
//!   closed kan.

use crate::schema::mjai::Tile;

const HONORS: [&str; 7] = ["E", "S", "W", "N", "P", "F", "C"];

/// One tile from its suit letter and digit.
pub fn tile_of(suit: char, digit: u8) -> Option<Tile> {
    match (suit, digit) {
        ('m' | 'p' | 's', 0) => Some(format!("5{suit}r")),
        ('m' | 'p' | 's', 1..=9) => Some(format!("{digit}{suit}")),
        ('z', 1..=7) => Some(HONORS[digit as usize - 1].to_string()),
        _ => None,
    }
}

/// A bare tile such as `"m3"` or `"z7"`.
pub fn tile(s: &str) -> Option<Tile> {
    let mut chars = s.chars();
    let suit = chars.next()?;
    let digit = chars.next()?.to_digit(10)? as u8;
    if chars.next().is_some() {
        return None;
    }
    tile_of(suit, digit)
}

/// A discard: the tile, whether it was the tile just drawn, and whether it
/// declares riichi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discard {
    pub pai: Tile,
    pub tsumogiri: bool,
    pub riichi: bool,
}

pub fn discard(s: &str) -> Option<Discard> {
    let pai = tile(s.get(..2)?)?;
    let rest = &s[2..];
    if !matches!(rest, "" | "_" | "*" | "@" | "_*" | "_@") {
        return None;
    }
    Some(Discard {
        pai,
        tsumogiri: rest.starts_with('_'),
        riichi: rest.ends_with(['*', '@']),
    })
}

/// The concealed tiles of a hand string, in order; melds after the first
/// comma and the riichi marker are ignored. `None` if any tile is hidden or
/// malformed.
pub fn hand(s: &str) -> Option<Vec<Tile>> {
    let concealed = s.split(',').next().unwrap_or("");
    let mut out = Vec::new();
    let mut suit = None;
    for c in concealed.chars() {
        match c {
            'm' | 'p' | 's' | 'z' => suit = Some(c),
            '0'..='9' => out.push(tile_of(suit?, c.to_digit(10)? as u8)?),
            '*' | '@' | '_' => {}
            _ => return None,
        }
    }
    Some(out)
}

/// Where a called tile came from, relative to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `+`: the seat after the caller.
    Next,
    /// `=`: the seat opposite.
    Opposite,
    /// `-`: the seat before the caller.
    Previous,
}

impl Source {
    /// Seats after the caller around a 4-seat table. A 3-player table is
    /// numbered the same way with seat 3 empty, so the next player of seat 2
    /// is two seats on (`=`).
    pub fn offset(self) -> u8 {
        match self {
            Source::Next => 1,
            Source::Opposite => 2,
            Source::Previous => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meld {
    Chi {
        from: Source,
        pai: Tile,
        consumed: [Tile; 2],
    },
    Pon {
        from: Source,
        pai: Tile,
        consumed: [Tile; 2],
    },
    Daiminkan {
        from: Source,
        pai: Tile,
        consumed: [Tile; 3],
    },
    /// `pai` is the added tile; `consumed` the pon it joins.
    Kakan {
        pai: Tile,
        consumed: [Tile; 3],
    },
    Ankan {
        consumed: [Tile; 4],
    },
}

pub fn meld(s: &str) -> Option<Meld> {
    let mut chars = s.chars();
    let suit = chars.next()?;
    let mut digits: Vec<u8> = Vec::new();
    // (digits before the mark, source): the called tile is the digit just before it.
    let mut mark: Option<(usize, Source)> = None;
    for c in chars {
        let from = match c {
            '+' => Source::Next,
            '=' => Source::Opposite,
            '-' => Source::Previous,
            '0'..='9' => {
                digits.push(c as u8 - b'0');
                continue;
            }
            _ => return None,
        };
        if mark.is_some() || digits.is_empty() {
            return None;
        }
        mark = Some((digits.len(), from));
    }
    let tiles: Vec<Tile> = digits
        .iter()
        .map(|&d| tile_of(suit, d))
        .collect::<Option<_>>()?;
    let rank = |d: u8| if d == 0 { 5 } else { d };
    let all_same = digits.windows(2).all(|w| rank(w[0]) == rank(w[1]));
    let is_run = {
        let mut r: Vec<u8> = digits.iter().map(|&d| rank(d)).collect();
        r.sort_unstable();
        suit != 'z' && r.windows(2).all(|w| w[1] == w[0] + 1)
    };
    let without = |i: usize| -> Vec<Tile> {
        tiles
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, t)| t.clone())
            .collect()
    };
    match (mark, tiles.len()) {
        (Some((k, from)), 3) if all_same => Some(Meld::Pon {
            from,
            pai: tiles[k - 1].clone(),
            consumed: without(k - 1).try_into().ok()?,
        }),
        (Some((k, from)), 3) if is_run => Some(Meld::Chi {
            from,
            pai: tiles[k - 1].clone(),
            consumed: without(k - 1).try_into().ok()?,
        }),
        // A pon with the added tile after its mark.
        (Some((3, _)), 4) if all_same => Some(Meld::Kakan {
            pai: tiles[3].clone(),
            consumed: tiles[..3].to_vec().try_into().ok()?,
        }),
        (Some((k, from)), 4) if all_same => Some(Meld::Daiminkan {
            from,
            pai: tiles[k - 1].clone(),
            consumed: without(k - 1).try_into().ok()?,
        }),
        (None, 4) if all_same => Some(Meld::Ankan {
            consumed: tiles.try_into().ok()?,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(v: &[&str]) -> Vec<Tile> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tiles_map_to_mjai_including_red_fives_and_honors() {
        assert_eq!(tile("m1").as_deref(), Some("1m"));
        assert_eq!(tile("p0").as_deref(), Some("5pr"));
        assert_eq!(tile("s9").as_deref(), Some("9s"));
        assert_eq!(tile("z1").as_deref(), Some("E"));
        assert_eq!(tile("z5").as_deref(), Some("P"));
        assert_eq!(tile("z7").as_deref(), Some("C"));
        for bad in ["", "m", "z0", "z8", "x1", "m10", "q1"] {
            assert_eq!(tile(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn discards_carry_tsumogiri_and_riichi_marks() {
        let d = |s| discard(s).unwrap();
        assert_eq!(
            d("z6"),
            Discard {
                pai: "F".into(),
                tsumogiri: false,
                riichi: false
            }
        );
        assert_eq!(
            d("z3_"),
            Discard {
                pai: "W".into(),
                tsumogiri: true,
                riichi: false
            }
        );
        assert_eq!(
            d("m3*"),
            Discard {
                pai: "3m".into(),
                tsumogiri: false,
                riichi: true
            }
        );
        assert_eq!(
            d("p0_*"),
            Discard {
                pai: "5pr".into(),
                tsumogiri: true,
                riichi: true
            }
        );
        assert_eq!(
            d("s7@"),
            Discard {
                pai: "7s".into(),
                tsumogiri: false,
                riichi: true
            }
        );
        assert_eq!(
            d("s7_@"),
            Discard {
                pai: "7s".into(),
                tsumogiri: true,
                riichi: true
            }
        );
        for bad in ["m3+", "m3*_", "m3**", "m3*@"] {
            assert_eq!(discard(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hands_parse_in_both_spellings() {
        assert_eq!(hand("s3z6m2p0").unwrap(), t(&["3s", "F", "2m", "5pr"]));
        assert_eq!(
            hand("m2458p1337s69,s7-89").unwrap(),
            t(&["2m", "4m", "5m", "8m", "1p", "3p", "3p", "7p", "6s", "9s"])
        );
        assert_eq!(hand("m11p0*").unwrap(), t(&["1m", "1m", "5pr"]));
        assert_eq!(
            hand("p55s123@,z4,z4").unwrap(),
            t(&["5p", "5p", "1s", "2s", "3s"])
        );
        assert_eq!(hand("x1x1x1"), None);
    }

    #[test]
    fn chi_names_the_called_tile_by_its_mark() {
        assert_eq!(
            meld("p1-23").unwrap(),
            Meld::Chi {
                from: Source::Previous,
                pai: "1p".into(),
                consumed: ["2p".into(), "3p".into()]
            }
        );
        assert_eq!(
            meld("s340-").unwrap(),
            Meld::Chi {
                from: Source::Previous,
                pai: "5sr".into(),
                consumed: ["3s".into(), "4s".into()]
            }
        );
        assert_eq!(
            meld("m45-6").unwrap(),
            Meld::Chi {
                from: Source::Previous,
                pai: "5m".into(),
                consumed: ["4m".into(), "6m".into()]
            }
        );
    }

    #[test]
    fn pon_and_kans_are_told_apart() {
        assert_eq!(
            meld("z444+").unwrap(),
            Meld::Pon {
                from: Source::Next,
                pai: "N".into(),
                consumed: ["N".into(), "N".into()]
            }
        );
        assert_eq!(
            meld("p505=").unwrap(),
            Meld::Pon {
                from: Source::Opposite,
                pai: "5p".into(),
                consumed: ["5p".into(), "5pr".into()]
            }
        );
        assert_eq!(
            meld("p550=").unwrap(),
            Meld::Pon {
                from: Source::Opposite,
                pai: "5pr".into(),
                consumed: ["5p".into(), "5p".into()]
            }
        );
        assert_eq!(
            meld("p9999+").unwrap(),
            Meld::Daiminkan {
                from: Source::Next,
                pai: "9p".into(),
                consumed: ["9p".into(), "9p".into(), "9p".into()]
            }
        );
        assert_eq!(
            meld("m5555-").unwrap(),
            Meld::Daiminkan {
                from: Source::Previous,
                pai: "5m".into(),
                consumed: ["5m".into(), "5m".into(), "5m".into()]
            }
        );
        assert_eq!(
            meld("m555=0").unwrap(),
            Meld::Kakan {
                pai: "5mr".into(),
                consumed: ["5m".into(), "5m".into(), "5m".into()]
            }
        );
        assert_eq!(
            meld("s5550").unwrap(),
            Meld::Ankan {
                consumed: ["5s".into(), "5s".into(), "5s".into(), "5sr".into()]
            }
        );
        for bad in [
            "z123-", "m12-", "m135-", "m555", "m5555=5", "m55=5=5", "-m555", "m555=6",
        ] {
            assert_eq!(meld(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn marks_are_4_seat_offsets() {
        assert_eq!(Source::Next.offset(), 1);
        assert_eq!(Source::Opposite.offset(), 2);
        assert_eq!(Source::Previous.offset(), 3);
    }
}
