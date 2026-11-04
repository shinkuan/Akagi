//! What the table says about a discard, without asking any bot.
//!
//! Adapted from Washizu (<https://github.com/nanomena/Washizu>, Apache-2.0).
//!
//! Two plain facts a person takes in at a glance and a bot's probabilities do
//! not carry. They go to the delay script (`ctx.loose`, `ctx.runner_up_loose`,
//! `ctx.safe`) so that a pace can tell apart decisions which look alike in
//! numbers: two loose tiles at even odds are an order to throw them in, not a
//! question; a live tile against a riichi is one however sure the bot was.
//!
//! Both are about the tile's place in the hand and on the table — not about
//! what kind of tile it is. A loose 8 is as loose as a loose North, and a 9
//! that finishes a run is not loose at all.

/// A tile's kind: its mjai name, with a red five read as the plain five.
fn kind(pai: &str) -> &str {
    pai.strip_suffix('r').unwrap_or(pai)
}

/// The number and suit of a number tile; `None` for an honour.
fn number_and_suit(kind: &str) -> Option<(u8, char)> {
    let mut chars = kind.chars();
    let number = chars.next()?.to_digit(10)? as u8;
    let suit = chars.next()?;
    matches!(suit, 'm' | 'p' | 's').then_some((number, suit))
}

/// Whether `pai` is loose in `hand`: nothing there connects to it — no second
/// copy, and for a number tile nothing of its suit within two.
///
/// `hand` is the concealed hand including the tile itself. A tile that is not
/// in it is not called loose: there is nothing to read.
pub fn is_loose(pai: &str, hand: &[String]) -> bool {
    let me = kind(pai);
    if hand.iter().filter(|tile| kind(tile) == me).count() != 1 {
        return false;
    }
    let Some((number, suit)) = number_and_suit(me) else {
        return true;
    };
    !hand.iter().any(|tile| {
        matches!(number_and_suit(kind(tile)),
            Some((other, its)) if its == suit && other != number && other.abs_diff(number) <= 2)
    })
}

/// Whether discarding `pai` cannot deal in to a riichi: every seat that has
/// declared one has discarded the tile itself. `None` with no riichi on the
/// table — there is nothing to be safe from.
///
/// `riichi_rivers` are the discards of the opponents in riichi, called tiles
/// included. Only this certain kind of safety is read; a tile that merely
/// passed after the declaration is not counted.
pub fn is_safe(pai: &str, riichi_rivers: &[Vec<&str>]) -> Option<bool> {
    if riichi_rivers.is_empty() {
        return None;
    }
    let me = kind(pai);
    Some(
        riichi_rivers
            .iter()
            .all(|river| river.iter().any(|tile| kind(tile) == me)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hand(tiles: &[&str]) -> Vec<String> {
        tiles.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn a_tile_is_loose_by_its_place_and_not_by_its_kind() {
        let h = hand(&[
            "1m", "2m", "3m", "8m", "4p", "5pr", "9p", "2s", "2s", "6s", "E", "N", "N", "9s",
        ]);
        // Nothing within two of it: loose, though it is a middle tile.
        assert!(is_loose("8m", &h));
        // A terminal that is part of a run is not.
        assert!(!is_loose("1m", &h));
        // Next to the red five.
        assert!(!is_loose("4p", &h));
        assert!(is_loose("9p", &h));
        assert!(is_loose("6s", &h));
        // A pair is not loose, in a suit or among the honours.
        assert!(!is_loose("2s", &h));
        assert!(!is_loose("N", &h));
        assert!(is_loose("E", &h));
        // 9s: 6s is three away.
        assert!(is_loose("9s", &h));
    }

    #[test]
    fn a_red_five_is_a_five() {
        let h = hand(&["5mr", "5m", "3p", "5pr", "9s"]);
        assert!(!is_loose("5mr", &h), "paired with the plain five");
        assert!(!is_loose("3p", &h), "two away from the red five");
        assert!(!is_loose("5pr", &h));
    }

    #[test]
    fn a_tile_that_is_not_in_the_hand_is_not_called_loose() {
        assert!(!is_loose("7z", &hand(&["1m", "9p"])));
        assert!(!is_loose("E", &hand(&["1m", "9p"])));
    }

    #[test]
    fn safe_means_every_riichi_has_discarded_it() {
        assert_eq!(is_safe("3p", &[]), None);
        let one = [vec!["1m", "3p", "E"]];
        assert_eq!(is_safe("3p", &one), Some(true));
        assert_eq!(is_safe("4p", &one), Some(false));
        let two = [vec!["1m", "3p", "E"], vec!["9s", "5mr"]];
        assert_eq!(is_safe("3p", &two), Some(false), "safe against one only");
        assert_eq!(is_safe("5m", &[vec!["5mr"]]), Some(true));
    }
}
