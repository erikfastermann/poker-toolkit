use std::{cmp::Ordering, fmt};

use crate::{card::Card, cards::Cards, result::Result};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hand(Card, Card);

impl fmt::Display for Hand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.high(), self.low())
    }
}

impl fmt::Debug for Hand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}

impl Hand {
    pub const MIN: Self = Self(Card::MIN, Card::MIN);

    pub fn of_two_cards(a: Card, b: Card) -> Self {
        match a.rank().cmp(&b.rank()) {
            Ordering::Less => Self(b, a),
            Ordering::Equal => match a.suite().to_usize().cmp(&b.suite().to_usize()) {
                Ordering::Less => Self(b, a),
                Ordering::Equal => unreachable!(),
                Ordering::Greater => Self(a, b),
            },
            Ordering::Greater => Self(a, b),
        }
    }

    fn from_cards(cards: Cards) -> Result<Self> {
        if cards.count() != 2 {
            Err(format!("hand: expected 2 cards, got {}", cards.count()).into())
        } else {
            Ok(cards.to_hand().unwrap())
        }
    }

    pub fn from_str(s: &str) -> Result<Self> {
        Self::from_bytes(s.as_bytes())
    }

    pub fn from_bytes(s: &[u8]) -> Result<Self> {
        Self::from_cards(Cards::from_bytes(s)?)
    }

    pub fn high(self) -> Card {
        self.0
    }

    pub fn low(self) -> Card {
        self.1
    }

    pub fn suited(self) -> bool {
        self.high().suite() == self.low().suite()
    }

    pub fn cmp_by_rank(self, other: Self) -> Ordering {
        self.high()
            .rank()
            .cmp(&other.high().rank())
            .then_with(|| self.low().rank().cmp(&other.low().rank()))
            .then_with(|| {
                self.high()
                    .suite()
                    .to_usize()
                    .cmp(&other.high().suite().to_usize())
            })
            .then_with(|| {
                self.low()
                    .suite()
                    .to_usize()
                    .cmp(&other.low().suite().to_usize())
            })
    }

    pub fn to_card_array(self) -> [Card; 2] {
        [self.high(), self.low()]
    }

    pub fn to_cards(self) -> Cards {
        Cards::EMPTY.with(self.high()).with(self.low())
    }

    pub fn to_index(self) -> usize {
        self.high().to_usize() * self.low().to_usize()
    }
}
