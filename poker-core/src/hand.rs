use std::{cmp::Ordering, fmt, str::FromStr};

use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{
    card::Card,
    cards::Cards,
    rank::Rank,
    result::{Error, Result},
    suite::Suite,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, DeserializeFromStr, SerializeDisplay)]
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

impl FromStr for Hand {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::from_bytes(s.as_bytes())
    }
}

impl Hand {
    pub(crate) const UNDEFINED: Self = Self(Card::MIN, Card::MIN);

    pub const MIN: Self = Self(
        Card::of(Rank::Three, Suite::Diamonds),
        Card::of(Rank::Two, Suite::Diamonds),
    );

    pub const COUNT: usize = 52 * 51 / 2;

    const HANDS: [Self; Self::COUNT] = Self::init_hands();

    const fn init_hands() -> [Hand; Hand::COUNT] {
        let mut hands = [Hand::UNDEFINED; Hand::COUNT];
        let mut index = 0;
        let mut a = 0;

        while a < Card::COUNT {
            let mut b = a + 1;

            while b < Card::COUNT {
                hands[index] = Hand::of_two_cards_sorted_unchecked(
                    Card::CARDS_BY_RANK[a],
                    Card::CARDS_BY_RANK[b],
                );
                index += 1;
                b += 1;
            }

            a += 1;
        }

        assert!(index == Hand::COUNT);
        hands
    }

    pub fn all() -> impl Iterator<Item = Self> {
        Self::HANDS.into_iter()
    }

    pub fn of_two_cards(a: Card, b: Card) -> Option<Self> {
        match a.rank().cmp(&b.rank()) {
            Ordering::Less => Some(Self(b, a)),
            Ordering::Equal => match a.suite().to_usize().cmp(&b.suite().to_usize()) {
                Ordering::Less => Some(Self(b, a)),
                Ordering::Equal => None,
                Ordering::Greater => Some(Self(a, b)),
            },
            Ordering::Greater => Some(Self(a, b)),
        }
    }

    const fn of_two_cards_sorted_unchecked(lt: Card, gt: Card) -> Self {
        Self(gt, lt)
    }

    fn from_cards(cards: Cards) -> Result<Self> {
        if cards.count() != 2 {
            Err(format!("hand: expected 2 cards, got {}", cards.count()).into())
        } else {
            Ok(cards.to_hand().unwrap())
        }
    }

    pub fn from_bytes(s: &[u8]) -> Result<Self> {
        Self::from_cards(Cards::from_bytes(s)?)
    }

    pub fn from_index(index: usize) -> Self {
        Self::HANDS[index]
    }

    pub fn to_index(self) -> usize {
        let low = self.low().to_index_52_by_rank() as isize;
        let high = self.high().to_index_52_by_rank() as isize;
        let start = 52 - low;
        let end = 51;
        let n = end - start + 1;
        let low_index = n * (start + end) / 2;
        let high_index = high - low - 1;
        (low_index + high_index) as usize
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_const_hands() {
        let mut index = 0;

        for a in Card::all_by_rank() {
            for b in Card::all_by_rank() {
                if b.cmp_by_rank(a).is_gt() {
                    let hand = Hand::of_two_cards(a, b).unwrap();
                    assert_eq!(hand, Hand::HANDS[index]);
                    index += 1;
                }
            }
        }

        assert_eq!(index, Hand::COUNT);
    }
}
