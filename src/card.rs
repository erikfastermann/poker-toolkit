use std::{cmp::Ordering, fmt};

use rand::{
    distributions::{Distribution, Standard},
    Rng,
};

use crate::{cards::Cards, rank::Rank, result::Result, suite::Suite};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Card(i8);

impl Distribution<Card> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Card {
        Card::of(rng.r#gen(), rng.r#gen())
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.rank(), self.suite())
    }
}

impl Card {
    pub const MIN: Self = Self(0);

    pub const COUNT: usize = Suite::COUNT * Rank::COUNT;

    pub fn of(rank: Rank, suite: Suite) -> Self {
        Self(suite.to_index() + rank.to_i8())
    }

    pub fn from_index(index: i8) -> Option<Self> {
        if index < 0 || index > 63 {
            None
        } else if Cards::MASK_FULL & (1u64 << u64::try_from(index).unwrap()) == 0 {
            None
        } else {
            Some(Self(index))
        }
    }

    pub fn from_index_52(index: i8) -> Option<Self> {
        if index < 0 || index > 51 {
            None
        } else {
            let suite = Suite::try_from(index / 13).ok()?;
            let rank = Rank::try_from(index % 13).ok()?;
            Some(Self::of(rank, suite))
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        match s.as_bytes() {
            [rank_raw, suite_raw] => {
                let rank = Rank::from_ascii(*rank_raw)?;
                let suite = Suite::from_ascii(*suite_raw)?;
                Ok(Self::of(rank, suite))
            }
            _ => Err(format!("invalid card '{s}': bad length").into()),
        }
    }

    pub fn all() -> impl Iterator<Item = Self> {
        Suite::SUITES
            .iter()
            .flat_map(|suite| Rank::RANKS.iter().map(|rank| Self::of(*rank, *suite)))
    }

    pub fn all_by_rank() -> impl Iterator<Item = Self> {
        Rank::RANKS
            .iter()
            .flat_map(|rank| Suite::SUITES.iter().map(|suite| Self::of(*rank, *suite)))
    }

    pub fn rank(self) -> Rank {
        Rank::try_from(self.0 % 16).unwrap()
    }

    pub fn suite(self) -> Suite {
        Suite::try_from(self.0 / 16).unwrap()
    }

    pub fn to_index(self) -> i8 {
        self.0
    }

    pub fn to_index_52(self) -> usize {
        self.suite().to_usize() * Rank::COUNT + self.rank().to_usize()
    }

    pub fn to_index_52_by_rank(self) -> usize {
        self.rank().to_usize() * Suite::COUNT + self.suite().to_usize()
    }

    pub fn to_index_u64(self) -> u64 {
        self.to_index() as u64
    }

    pub fn to_usize(self) -> usize {
        self.0 as usize
    }

    pub fn cmp_by_rank(self, other: Self) -> Ordering {
        self.rank()
            .cmp(&other.rank())
            .then_with(|| self.suite().to_usize().cmp(&other.suite().to_usize()))
    }
}
