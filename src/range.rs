use std::cmp::{max, min};
use std::collections::HashSet;
use std::fmt;
use std::ops::BitAndAssign;

use crate::card::Card;
use crate::cards::{Cards, CardsByRank};
use crate::hand::Hand;
use crate::rank::Rank;
use crate::result::Result;
use crate::suite::Suite;

#[derive(Clone, Copy)]
struct RangeEntry {
    high: Rank,
    low: Rank,
    suited: bool,
}

impl fmt::Display for RangeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.high, self.low)?;
        if self.high == self.low {
            write!(f, "-")
        } else if self.suited {
            write!(f, "s")
        } else {
            write!(f, "o")
        }
    }
}

impl RangeEntry {
    fn from_hand(hand: Hand) -> Self {
        RangeEntry {
            high: hand.high().rank(),
            low: hand.low().rank(),
            suited: hand.suited(),
        }
    }

    fn first_second(self) -> (Rank, Rank) {
        debug_assert!(self.high >= self.low);
        if self.suited {
            (self.high, self.low)
        } else {
            (self.low, self.high)
        }
    }
}

#[derive(Clone)]
pub struct RangeTable {
    table: [CardsByRank; Rank::COUNT],
}

impl fmt::Display for RangeTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in Rank::RANKS.iter().rev().copied() {
            let mut iter = Rank::RANKS.iter().rev().copied().peekable();
            while let Some(column) = iter.next() {
                let entry = RangeEntry {
                    high: max(row, column),
                    low: min(row, column),
                    suited: column < row,
                };
                let contains = if self.contains_entry(entry) { "T" } else { "F" };
                write!(f, "{} ({})", entry, contains)?;
                if iter.peek().is_some() {
                    write!(f, " ")?;
                }
            }
            write!(f, "\n")?;
        }
        Ok(())
    }
}

impl RangeTable {
    pub fn empty() -> Self {
        Self {
            table: [CardsByRank::EMPTY; Rank::COUNT],
        }
    }

    pub fn full() -> Self {
        let mut range = Self::empty();
        for row in Rank::RANKS.iter().rev().copied() {
            for column in Rank::RANKS.iter().rev().copied() {
                let high = max(row, column);
                let low = min(row, column);
                let suited = column < row;
                range.add(RangeEntry { high, low, suited });
            }
        }
        range
    }

    pub fn parse(range_str: &str) -> Result<Self> {
        let range_str = range_str.trim();
        if range_str == "full" {
            return Ok(Self::full());
        }

        let mut range = Self::empty();
        for def in range_str.split(',') {
            let result = match def.as_bytes() {
                [pair_a, pair_b] if pair_a == pair_b => range.parse_pair(*pair_a),
                [pair_a, pair_b, b'+'] if pair_a == pair_b => range.parse_pairs_asc(*pair_a),
                [high, low, b'o'] => range.parse_one(*high, *low, false),
                [high, low, b'o', b'+'] => range.parse_asc(*high, *low, false),
                [high, low, b's'] => range.parse_one(*high, *low, true),
                [high, low, b's', b'+'] => range.parse_asc(*high, *low, true),
                _ => Err("parsing failed".into()),
            };

            if let Err(err) = result {
                return Err(format!(
                    "invalid range '{}': invalid entry '{}': {}",
                    range_str, def, err,
                )
                .into());
            }
        }

        Ok(range)
    }

    fn contains_entry(&self, entry: RangeEntry) -> bool {
        let (a, b) = entry.first_second();
        self.table[a.to_usize()].has(b)
    }

    pub fn for_each_hand(&self, mut f: impl FnMut(Hand)) {
        for row_rank in Rank::RANKS {
            let mut row = self.table[row_rank.to_usize()];
            while let Some(column_rank) = row.highest_rank() {
                row.remove(column_rank);
                let suited = row_rank > column_rank;
                debug_assert!({
                    let entry = RangeEntry {
                        high: max(row_rank, column_rank),
                        low: min(row_rank, column_rank),
                        suited,
                    };
                    self.contains_entry(entry)
                });
                if suited {
                    for suite in Suite::SUITES {
                        let hand = Hand::of_two_cards(
                            Card::of(row_rank, suite),
                            Card::of(column_rank, suite),
                        );
                        f(hand);
                    }
                } else {
                    for suite_a in Suite::SUITES {
                        for suite_b in Suite::SUITES[suite_a.to_usize() + 1..].iter().copied() {
                            let hand = Hand::of_two_cards(
                                Card::of(row_rank, suite_a),
                                Card::of(column_rank, suite_b),
                            );
                            f(hand);
                            if row_rank != column_rank {
                                let hand = Hand::of_two_cards(
                                    Card::of(row_rank, suite_b),
                                    Card::of(column_rank, suite_a),
                                );
                                f(hand);
                            }
                        }
                    }
                }
            }
        }
    }

    fn add(&mut self, entry: RangeEntry) {
        let (a, b) = entry.first_second();
        self.table[a.to_usize()].add(b)
    }

    fn try_add(&mut self, entry: RangeEntry) -> Result<()> {
        let (a, b) = entry.first_second();
        if self.table[a.to_usize()].try_add(b) {
            Ok(())
        } else {
            Err(format!("range table add failed: duplicate entry {}", entry).into())
        }
    }

    pub fn contains(&self, hand: Hand) -> bool {
        self.contains_entry(RangeEntry::from_hand(hand))
    }

    pub fn is_empty(&self) -> bool {
        self.table.iter().all(|row| *row == CardsByRank::EMPTY)
    }

    pub fn count(&self) -> u8 {
        self.table.iter().map(|row| row.count_u8()).sum()
    }

    pub fn count_hands(&self) -> u32 {
        let mut count = 0u32;
        self.for_each_hand(|_| count += 1);
        count
    }

    pub fn card_set(&self) -> Cards {
        let mut cards = Cards::EMPTY;
        self.for_each_hand(|hand| {
            cards.try_add(hand.high());
            cards.try_add(hand.low());
        });
        cards
    }

    pub fn to_vec(&self) -> Vec<Hand> {
        let mut hands = Vec::new();
        self.for_each_hand(|hand| hands.push(hand));
        hands
    }

    pub fn to_set(&self) -> HashSet<Hand> {
        let mut hands = HashSet::new();
        for high in Rank::RANKS.iter().rev().copied() {
            for low in Rank::RANKS[..=high.to_usize()].iter().rev().copied() {
                for suite_a in Suite::SUITES {
                    for suite_b in Suite::SUITES {
                        let suited = suite_a == suite_b;
                        if suited && high == low {
                            continue;
                        }
                        if !self.contains_entry(RangeEntry { high, low, suited }) {
                            continue;
                        }
                        let hand =
                            Hand::of_two_cards(Card::of(high, suite_a), Card::of(low, suite_b));
                        hands.insert(hand);
                    }
                }
            }
        }
        hands
    }

    fn parse_pair(&mut self, raw_rank: u8) -> Result<()> {
        let rank = Rank::from_ascii(raw_rank)?;
        self.try_add(RangeEntry {
            high: rank,
            low: rank,
            suited: false,
        })?;
        Ok(())
    }

    fn parse_pairs_asc(&mut self, raw_rank: u8) -> Result<()> {
        let from = Rank::from_ascii(raw_rank)?;
        for rank in Rank::range(from, Rank::Ace) {
            let entry = RangeEntry {
                high: rank,
                low: rank,
                suited: false,
            };
            self.try_add(entry)?;
        }
        Ok(())
    }

    fn parse_one(&mut self, raw_high: u8, raw_low: u8, suited: bool) -> Result<()> {
        let high = Rank::from_ascii(raw_high)?;
        let low = Rank::from_ascii(raw_low)?;
        if low >= high {
            Err("low greater or equals to high".into())
        } else {
            self.try_add(RangeEntry { high, low, suited })
        }
    }

    fn parse_asc(&mut self, raw_high: u8, raw_low: u8, suited: bool) -> Result<()> {
        let high = Rank::from_ascii(raw_high)?;
        let low = Rank::from_ascii(raw_low)?;
        if low >= high {
            return Err("low greater or equals to high".into());
        }
        for rank in Rank::range(low, high.predecessor().unwrap()) {
            self.try_add(RangeEntry {
                high,
                low: rank,
                suited,
            })?;
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FullRangeTable {
    table: [u64; 21],
}

static mut HANDS: [Hand; FullRangeTable::COUNT] = [Hand::MIN; FullRangeTable::COUNT];

impl FullRangeTable {
    pub const EMPTY: Self = Self { table: [0; 21] };

    pub const FULL: Self = {
        let mut table = [u64::MAX; 21];
        table[20] = u64::MAX >> 64 - (Self::COUNT - 20 * 64);
        Self { table }
    };

    pub const COUNT: usize = 52 * 51 / 2;

    pub unsafe fn init() {
        let mut index = 0;
        Self::FULL.for_each_hand(|hand| {
            HANDS[index] = hand;
            index += 1;
        });
        assert_eq!(index, Self::COUNT);
    }

    pub fn from_range_table(table: &RangeTable) -> Self {
        let mut out = FullRangeTable::EMPTY;
        table.for_each_hand(|hand| out.add_hand(hand));
        out
    }

    pub fn parse(range_str: &str) -> Result<Self> {
        let range_str = range_str.trim();
        if range_str == "full" {
            return Ok(Self::FULL);
        }

        let mut range = Self::EMPTY;
        for def in range_str.split(',') {
            let result = match def.as_bytes() {
                [pair_a, pair_b] if pair_a == pair_b => range.parse_pair(*pair_a),
                [pair_a, pair_b, b'+'] if pair_a == pair_b => range.parse_pairs_asc(*pair_a),
                [high, low, b'o'] => range.parse_one(*high, *low, false),
                [high, low, b'o', b'+'] => range.parse_asc(*high, *low, false),
                [high, low, b's'] => range.parse_one(*high, *low, true),
                [high, low, b's', b'+'] => range.parse_asc(*high, *low, true),
                hand if def.len() == 4 => range.parse_hand(hand),
                _ => Err("parsing failed".into()),
            };

            if let Err(err) = result {
                return Err(format!(
                    "invalid range '{}': invalid entry '{}': {}",
                    range_str, def, err,
                )
                .into());
            }
        }

        Ok(range)
    }

    fn parse_pair(&mut self, raw_rank: u8) -> Result<()> {
        let rank = Rank::from_ascii(raw_rank)?;
        self.add_pairs(rank)
    }

    fn parse_pairs_asc(&mut self, raw_rank: u8) -> Result<()> {
        let from = Rank::from_ascii(raw_rank)?;
        for rank in Rank::range(from, Rank::Ace) {
            self.add_pairs(rank)?;
        }
        Ok(())
    }

    fn parse_one(&mut self, raw_high: u8, raw_low: u8, suited: bool) -> Result<()> {
        let high = Rank::from_ascii(raw_high)?;
        let low = Rank::from_ascii(raw_low)?;
        self.add_unpaired(high, low, suited)
    }

    fn parse_asc(&mut self, raw_high: u8, raw_low: u8, suited: bool) -> Result<()> {
        let high = Rank::from_ascii(raw_high)?;
        let low = Rank::from_ascii(raw_low)?;
        for rank in Rank::range(low, high.predecessor().unwrap()) {
            self.add_unpaired(high, rank, suited)?;
        }
        Ok(())
    }

    fn parse_hand(&mut self, hand: &[u8]) -> Result<()> {
        let hand = Hand::from_bytes(hand)?;
        self.try_add_hand(hand)
    }

    fn add_pairs(&mut self, rank: Rank) -> Result<()> {
        for suite_a in Suite::SUITES {
            for suite_b in Suite::SUITES {
                if suite_b.to_usize() > suite_a.to_usize() {
                    let hand = Hand::of_two_cards(Card::of(rank, suite_a), Card::of(rank, suite_b));
                    self.try_add_hand(hand)?;
                }
            }
        }
        Ok(())
    }

    fn add_unpaired(&mut self, high: Rank, low: Rank, suited: bool) -> Result<()> {
        if low >= high {
            return Err(format!("{low} >= {high}").into());
        }
        for suite_low in Suite::SUITES {
            for suite_high in Suite::SUITES {
                if (suite_low == suite_high) != suited {
                    continue;
                }
                let hand = Hand::of_two_cards(Card::of(high, suite_high), Card::of(low, suite_low));
                self.try_add_hand(hand)?;
            }
        }
        Ok(())
    }

    pub fn has_hand(&self, hand: Hand) -> bool {
        let index = Self::hand_to_index(hand);
        self.has_index(index)
    }

    fn has_index(&self, index: usize) -> bool {
        let u = self.table[index / 64];
        (u & (1 << index % 64)) != 0
    }

    fn add_index_unchecked(&mut self, index: usize) {
        self.table[index / 64] |= 1 << index % 64;
    }

    pub fn add_hand(&mut self, hand: Hand) {
        let index = Self::hand_to_index(hand);
        assert!(!self.has_index(index));
        self.add_index_unchecked(index);
    }

    pub fn add_hand_unchecked(&mut self, hand: Hand) {
        let index = Self::hand_to_index(hand);
        self.add_index_unchecked(index);
    }

    pub fn try_add_hand(&mut self, hand: Hand) -> Result<()> {
        let index = Self::hand_to_index(hand);
        if self.has_index(index) {
            Err(format!("range table add failed: duplicate hand {}", hand).into())
        } else {
            self.add_index_unchecked(index);
            Ok(())
        }
    }

    fn hand_to_index(hand: Hand) -> usize {
        let low = hand.low().to_index_52_by_rank() as isize;
        let high = hand.high().to_index_52_by_rank() as isize;
        let start = 52 - low;
        let end = 51;
        let n = end - start + 1;
        let low_index = n * (start + end) / 2;
        let high_index = high - low - 1;
        (low_index + high_index) as usize
    }

    fn index_to_hand(index: usize) -> Hand {
        unsafe { HANDS[index] }
    }

    fn for_each_hand(&self, mut f: impl FnMut(Hand)) {
        for a in Card::all_by_rank() {
            for b in Card::all_by_rank() {
                if b.cmp_by_rank(a).is_gt() {
                    f(Hand::of_two_cards(a, b));
                }
            }
        }
    }

    pub fn count(&self) -> u32 {
        self.table.iter().map(|u| u.count_ones()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self == &Self::EMPTY
    }
}

impl<'a> IntoIterator for &'a FullRangeTable {
    type Item = Hand;

    type IntoIter = RangeTableIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        RangeTableIter {
            table: self,
            index: 0,
        }
    }
}

pub struct RangeTableIter<'a> {
    table: &'a FullRangeTable,
    index: usize,
}

impl<'a> Iterator for RangeTableIter<'a> {
    type Item = Hand;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < FullRangeTable::COUNT {
            let has_hand = self.table.has_index(self.index);
            let hand = FullRangeTable::index_to_hand(self.index);
            self.index += 1;
            if has_hand {
                return Some(hand);
            }
        }
        None
    }
}

impl FromIterator<Hand> for FullRangeTable {
    fn from_iter<T: IntoIterator<Item = Hand>>(iter: T) -> Self {
        let mut t = Self::EMPTY;
        for hand in iter {
            t.add_hand_unchecked(hand);
        }
        t
    }
}

impl BitAndAssign for FullRangeTable {
    fn bitand_assign(&mut self, rhs: Self) {
        for i in 0..self.table.len() {
            self.table[i] &= rhs.table[i];
        }
    }
}

impl fmt::Debug for FullRangeTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v: Vec<_> = self.into_iter().collect();
        fmt::Debug::fmt(&v, f)
    }
}
