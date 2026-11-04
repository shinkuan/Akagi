//! A duration setting given as a range of milliseconds.
//!
//! Any duration that is the same every time is a tell: a person never
//! hovers, holds or waits for exactly as long twice. So the autoplay timing
//! settings are ranges, and every decision draws its own value from each
//! (see [`MsRange::draw`]).
//!
//! In `config.toml` a range is a two-element array, `hover_delay_ms = [150,
//! 300]`. A single number is read as a range of one value — that is how
//! these settings were written before they were ranges. A field read
//! through [`upgrade`] additionally treats the *old default* as "never
//! changed" and replaces it with the new default range, so an existing
//! install picks up the ranges without the user touching anything.

use rand::Rng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An inclusive range of milliseconds, `min <= max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsRange {
    pub min: u32,
    pub max: u32,
}

impl MsRange {
    /// The range between `a` and `b`, whichever way round they are given.
    pub const fn new(a: u32, b: u32) -> Self {
        if a <= b {
            Self { min: a, max: b }
        } else {
            Self { min: b, max: a }
        }
    }

    /// A range of exactly one value.
    pub const fn fixed(ms: u32) -> Self {
        Self { min: ms, max: ms }
    }

    /// One value drawn uniformly from the range, as a range of its own.
    /// Settings are drawn once per decision, so everything that decision
    /// does — the floor it waits out, the clicks it makes — agrees.
    pub fn draw<R: Rng + ?Sized>(&self, rng: &mut R) -> Self {
        Self::fixed(rng.random_range(self.min..=self.max))
    }

    /// The value in force: after [`draw`](Self::draw), the one drawn for
    /// this decision; on a range that was not drawn, its lower end.
    pub const fn ms(&self) -> u32 {
        self.min
    }
}

impl Serialize for MsRange {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        [self.min, self.max].serialize(s)
    }
}

/// Every shape a range is read from.
#[derive(Deserialize)]
#[serde(untagged)]
enum Repr {
    One(u32),
    Pair([u32; 2]),
    Table { min: u32, max: u32 },
}

impl<'de> Deserialize<'de> for MsRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Repr::deserialize(d)? {
            Repr::One(ms) => Self::fixed(ms),
            Repr::Pair([a, b]) | Repr::Table { min: a, max: b } => Self::new(a, b),
        })
    }
}

/// Read a range written before the setting was one: a bare `old_default`
/// becomes `new_default`, any other bare number a range of that one value,
/// and a range is taken as written.
pub fn upgrade<'de, D: Deserializer<'de>>(
    d: D,
    old_default: u32,
    new_default: MsRange,
) -> Result<MsRange, D::Error> {
    Ok(match Repr::deserialize(d)? {
        Repr::One(ms) if ms == old_default => new_default,
        Repr::One(ms) => MsRange::fixed(ms),
        Repr::Pair([a, b]) | Repr::Table { min: a, max: b } => MsRange::new(a, b),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[derive(Debug, Deserialize, Serialize)]
    struct Plain {
        r: MsRange,
    }

    #[derive(Debug, Deserialize)]
    struct Upgraded {
        #[serde(deserialize_with = "up")]
        r: MsRange,
    }

    fn up<'de, D: Deserializer<'de>>(d: D) -> Result<MsRange, D::Error> {
        upgrade(d, 200, MsRange::new(150, 300))
    }

    #[test]
    fn reads_every_shape_and_writes_a_pair() {
        let r = |s: &str| toml::from_str::<Plain>(s).unwrap().r;
        assert_eq!(r("r = 200"), MsRange::fixed(200));
        assert_eq!(r("r = [100, 300]"), MsRange::new(100, 300));
        assert_eq!(
            r("r = [300, 100]"),
            MsRange::new(100, 300),
            "either way round"
        );
        assert_eq!(r("r = { min = 1, max = 2 }"), MsRange::new(1, 2));
        let out = toml::to_string(&Plain {
            r: MsRange::new(150, 300),
        })
        .unwrap();
        assert_eq!(out.trim(), "r = [150, 300]");
    }

    #[test]
    fn the_old_default_becomes_the_new_range_and_an_edit_stays_put() {
        let r = |s: &str| toml::from_str::<Upgraded>(s).unwrap().r;
        assert_eq!(r("r = 200"), MsRange::new(150, 300), "never changed");
        assert_eq!(r("r = 250"), MsRange::fixed(250), "the user's own value");
        assert_eq!(r("r = [200, 200]"), MsRange::fixed(200), "already a range");
    }

    #[test]
    fn a_draw_lands_inside_and_varies() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let range = MsRange::new(100, 200);
        let draws: Vec<u32> = (0..200).map(|_| range.draw(&mut rng).ms()).collect();
        assert!(draws.iter().all(|ms| (100..=200).contains(ms)));
        assert!(draws.iter().any(|&ms| ms != draws[0]), "not one value");
        assert_eq!(MsRange::fixed(5).draw(&mut rng), MsRange::fixed(5));
    }
}
