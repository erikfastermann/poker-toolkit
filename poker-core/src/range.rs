use std::cmp::{max, min};
use std::collections::{BTreeMap, HashSet};
use std::ops::{BitAndAssign, Index, IndexMut};
use std::str::FromStr;
use std::sync::Arc;
use std::{array, fmt, iter};

use rand::distributions::WeightedIndex;
use rand::prelude::Distribution;
use rand::Rng;
use serde::{de, Deserialize, Serialize, Serializer};

use crate::card::Card;
use crate::cards::{Cards, CardsByRank};
use crate::game::{Action, Game, MilliBigBlind, Player, State, Street};
use crate::hand::Hand;
use crate::rank::Rank;
use crate::result::{Error, Result};
use crate::suite::Suite;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RangeEntry {
    high: Rank,
    low: Rank,
    suited: bool,
}

impl Serialize for RangeEntry {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_regular_string())
    }
}

impl FromStr for RangeEntry {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::from_bytes(s.as_bytes())
    }
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
    pub fn new(high: Rank, low: Rank, suited: bool) -> Option<Self> {
        if high < low || (high == low && suited) {
            return None;
        }

        Some(Self { high, low, suited })
    }

    pub fn paired(rank: Rank) -> Self {
        Self {
            high: rank,
            low: rank,
            suited: false,
        }
    }

    pub fn from_hand(hand: Hand) -> Self {
        RangeEntry {
            high: hand.high().rank(),
            low: hand.low().rank(),
            suited: hand.suited(),
        }
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        let (high, low, suited) = match b {
            [high, low] if *high == *low => (*high, *low, false),
            [high, low, b'-'] if *high == *low => (*high, *low, false),
            [high, low, b'o'] if *high != *low => (*high, *low, false),
            [high, low, b's'] if *high != *low => (*high, *low, true),
            _ => return Err("range entry: invalid format".into()),
        };
        let high = Rank::from_ascii(high)?;
        let low = Rank::from_ascii(low)?;
        Self::new(high, low, suited).ok_or_else(|| "range entry: invalid format".into())
    }

    fn to_regular_string(self) -> String {
        // TODO: Static str.

        let suite_info = if self.high == self.low {
            ""
        } else if self.suited {
            "s"
        } else {
            "o"
        };
        format!("{}{}{}", self.high, self.low, suite_info)
    }

    fn from_row_column(row: Rank, column: Rank) -> Self {
        Self {
            high: max(row, column),
            low: min(row, column),
            suited: column < row,
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

    pub fn suited(self) -> bool {
        self.suited
    }

    pub fn pair(self) -> bool {
        self.high == self.low
    }

    pub fn combo_count(self) -> u8 {
        if self.pair() {
            6
        } else if self.suited() {
            4
        } else {
            12
        }
    }
}

#[derive(Clone)]
pub struct PreFlopRangeTable {
    table: [CardsByRank; Rank::COUNT],
}

impl fmt::Display for PreFlopRangeTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in Rank::RANKS.iter().rev().copied() {
            let mut iter = Rank::RANKS.iter().rev().copied().peekable();
            while let Some(column) = iter.next() {
                let entry = RangeEntry::from_row_column(row, column);
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

impl PreFlopRangeTable {
    pub const COUNT: usize = Rank::COUNT * Rank::COUNT;

    pub fn entries() -> impl Iterator<Item = RangeEntry> {
        Rank::RANKS.into_iter().rev().flat_map(|row| {
            Rank::RANKS
                .into_iter()
                .rev()
                .map(move |column| RangeEntry::from_row_column(row, column))
        })
    }

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

    pub fn contains_entry(&self, entry: RangeEntry) -> bool {
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
                        )
                        .unwrap();
                        f(hand);
                    }
                } else {
                    for suite_a in Suite::SUITES {
                        for suite_b in Suite::SUITES[suite_a.to_usize() + 1..].iter().copied() {
                            let hand = Hand::of_two_cards(
                                Card::of(row_rank, suite_a),
                                Card::of(column_rank, suite_b),
                            )
                            .unwrap();
                            f(hand);
                            if row_rank != column_rank {
                                let hand = Hand::of_two_cards(
                                    Card::of(row_rank, suite_b),
                                    Card::of(column_rank, suite_a),
                                )
                                .unwrap();
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
                            Hand::of_two_cards(Card::of(high, suite_a), Card::of(low, suite_b))
                                .unwrap();
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

#[derive(Default, Clone)]
pub struct PreFlopRangeTableWith<T> {
    table: [[T; Rank::COUNT]; Rank::COUNT],
}

impl<T: Serialize> Serialize for PreFlopRangeTableWith<T> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut state = serializer.serialize_map(Some(PreFlopRangeTable::COUNT))?;
        for (entry, value) in self.iter() {
            state.serialize_entry(&entry, value)?;
        }
        state.end()
    }
}

impl<'de, T: Deserialize<'de> + Default> Deserialize<'de> for PreFlopRangeTableWith<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let range: BTreeMap<String, T> = BTreeMap::deserialize(deserializer)?;

        if range.len() != PreFlopRangeTable::COUNT {
            return Err(de::Error::invalid_length(
                range.len(),
                &PreFlopRangeTable::COUNT.to_string().as_str(),
            ));
        }

        let mut out = Self::default();
        for (key, value) in range {
            let entry =
                RangeEntry::from_str(&key).map_err(|err| de::Error::custom(err.to_string()))?;
            out[entry] = value;
        }
        Ok(out)
    }
}

impl<T: fmt::Debug> fmt::Debug for PreFlopRangeTableWith<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut state = f.debug_map();

        for (entry, value) in self.iter() {
            state.entry(&entry.to_regular_string(), value);
        }

        state.finish()
    }
}

impl<T> PreFlopRangeTableWith<T> {
    pub fn iter(&self) -> impl Iterator<Item = (RangeEntry, &T)> {
        self.table
            .iter()
            .enumerate()
            .rev()
            .flat_map(|(row_index, row)| {
                row.iter()
                    .enumerate()
                    .rev()
                    .map(move |(column_index, value)| {
                        let entry = RangeEntry::from_row_column(
                            Rank::RANKS[row_index],
                            Rank::RANKS[column_index],
                        );
                        (entry, value)
                    })
            })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (RangeEntry, &mut T)> {
        self.table
            .iter_mut()
            .enumerate()
            .rev()
            .flat_map(|(row_index, row)| {
                row.iter_mut()
                    .enumerate()
                    .rev()
                    .map(move |(column_index, value)| {
                        let entry = RangeEntry::from_row_column(
                            Rank::RANKS[row_index],
                            Rank::RANKS[column_index],
                        );
                        (entry, value)
                    })
            })
    }
}

impl<T> Index<RangeEntry> for PreFlopRangeTableWith<T> {
    type Output = T;

    fn index(&self, entry: RangeEntry) -> &Self::Output {
        let (a, b) = entry.first_second();
        &self.table[a.to_usize()][b.to_usize()]
    }
}

impl<T> IndexMut<RangeEntry> for PreFlopRangeTableWith<T> {
    fn index_mut(&mut self, entry: RangeEntry) -> &mut Self::Output {
        let (a, b) = entry.first_second();
        &mut self.table[a.to_usize()][b.to_usize()]
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RangeTable {
    table: [u64; 21],
}

impl RangeTable {
    pub const EMPTY: Self = Self { table: [0; 21] };

    pub const FULL: Self = {
        let mut table = [u64::MAX; 21];
        table[20] = u64::MAX >> 64 - (Hand::COUNT - 20 * 64);
        Self { table }
    };

    pub fn from_range_table(table: &PreFlopRangeTable) -> Self {
        let mut out = RangeTable::EMPTY;
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
                    let hand = Hand::of_two_cards(Card::of(rank, suite_a), Card::of(rank, suite_b))
                        .unwrap();
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
                let hand = Hand::of_two_cards(Card::of(high, suite_high), Card::of(low, suite_low))
                    .unwrap();
                self.try_add_hand(hand)?;
            }
        }
        Ok(())
    }

    pub fn has_hand(&self, hand: Hand) -> bool {
        let index = hand.to_index();
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
        let index = hand.to_index();
        assert!(!self.has_index(index));
        self.add_index_unchecked(index);
    }

    pub fn add_hand_unchecked(&mut self, hand: Hand) {
        let index = hand.to_index();
        self.add_index_unchecked(index);
    }

    pub fn try_add_hand(&mut self, hand: Hand) -> Result<()> {
        let index = hand.to_index();
        if self.has_index(index) {
            Err(format!("range table add failed: duplicate hand {}", hand).into())
        } else {
            self.add_index_unchecked(index);
            Ok(())
        }
    }

    pub fn count(&self) -> u32 {
        self.table.iter().map(|u| u.count_ones()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self == &Self::EMPTY
    }
}

impl<'a> IntoIterator for &'a RangeTable {
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
    table: &'a RangeTable,
    index: usize,
}

impl<'a> Iterator for RangeTableIter<'a> {
    type Item = Hand;

    fn next(&mut self) -> Option<Self::Item> {
        while self.index < Hand::COUNT {
            let has_hand = self.table.has_index(self.index);
            let hand = Hand::from_index(self.index);
            self.index += 1;
            if has_hand {
                return Some(hand);
            }
        }
        None
    }
}

impl FromIterator<Hand> for RangeTable {
    fn from_iter<T: IntoIterator<Item = Hand>>(iter: T) -> Self {
        let mut t = Self::EMPTY;
        for hand in iter {
            t.add_hand_unchecked(hand);
        }
        t
    }
}

impl BitAndAssign for RangeTable {
    fn bitand_assign(&mut self, rhs: Self) {
        for i in 0..self.table.len() {
            self.table[i] &= rhs.table[i];
        }
    }
}

impl fmt::Debug for RangeTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v: Vec<_> = self.into_iter().collect();
        fmt::Debug::fmt(&v, f)
    }
}

#[derive(Clone)]
pub struct RangeTableWith<T> {
    table: [T; Hand::COUNT],
}

impl<T: Default> Default for RangeTableWith<T> {
    fn default() -> Self {
        Self {
            table: array::from_fn(|_| Default::default()),
        }
    }
}

impl<T> RangeTableWith<T> {
    pub fn iter(&self) -> impl Iterator<Item = (Hand, &T)> {
        self.table
            .iter()
            .enumerate()
            .map(|(index, value)| (Hand::from_index(index), value))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Hand, &mut T)> {
        self.table
            .iter_mut()
            .enumerate()
            .map(|(index, value)| (Hand::from_index(index), value))
    }
}

impl<T> Index<Hand> for RangeTableWith<T> {
    type Output = T;

    fn index(&self, hand: Hand) -> &Self::Output {
        let index = hand.to_index();
        &self.table[index]
    }
}

impl<T> IndexMut<Hand> for RangeTableWith<T> {
    fn index_mut(&mut self, hand: Hand) -> &mut Self::Output {
        let index = hand.to_index();
        &mut self.table[index]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreFlopAction {
    Post { player: u8, amount: MilliBigBlind },
    Straddle { player: u8, amount: MilliBigBlind },
    Fold,
    Check,
    Call,
    Raise(MilliBigBlind),
}

impl PreFlopAction {
    fn is_action_kind(self, action: Action) -> bool {
        match (self, action) {
            (PreFlopAction::Post { .. }, Action::Post { .. }) => true,
            (PreFlopAction::Straddle { .. }, Action::Straddle { .. }) => true,
            (PreFlopAction::Fold, Action::Fold(_)) => true,
            (PreFlopAction::Check, Action::Check(_)) => true,
            (PreFlopAction::Call, Action::Call { .. }) => true,
            (PreFlopAction::Raise(_), Action::Raise { .. }) => true,
            _ => false,
        }
    }

    fn apply_to_game(self, game: &mut Game) -> Result<()> {
        match self {
            PreFlopAction::Post { player, amount } => {
                game.additional_post(usize::from(player), u32::try_from(amount)?, false)
            }
            PreFlopAction::Straddle { player, amount } => {
                game.straddle(usize::from(player), u32::try_from(amount)?)
            }
            PreFlopAction::Fold => game.fold(),
            PreFlopAction::Check => game.check(),
            PreFlopAction::Call => game.call(),
            // TODO:
            // In the GTO Wizard crawler some raises are smaller than the minimum,
            // so we sadly have to use this function.
            PreFlopAction::Raise(to) => game.unsafe_raise_min_bet_unchecked(u32::try_from(to)?),
        }
    }
}

// The value might not be valid after deserialization,
// but is checked again in the parent struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeAction {
    action: PreFlopAction,
    frequency: u64,
    /// Frequencies valid from 0 to 10_000, divide by 100 to get the percentage.
    range: PreFlopRangeTableWith<u16>,
    ev: Option<PreFlopRangeTableWith<MilliBigBlind>>,
}

const MAX_FREQUENCY: u16 = 10_000;

impl RangeAction {
    pub fn new(
        action: PreFlopAction,
        total_range: &PreFlopRangeTableWith<u16>,
        range: PreFlopRangeTableWith<u16>,
        ev: Option<PreFlopRangeTableWith<MilliBigBlind>>,
    ) -> Self {
        let mut action = Self {
            action,
            frequency: 0,
            range,
            ev,
        };
        action.frequency = action.init_frequency(total_range);
        action
    }

    fn init_frequency(&self, total_range: &PreFlopRangeTableWith<u16>) -> u64 {
        self.range
            .iter()
            .map(|(entry, frequency)| {
                u64::from(total_range[entry])
                    * u64::from(entry.combo_count())
                    * u64::from(*frequency)
            })
            .sum()
    }

    fn frequency(&self, total_frequency: u64) -> f64 {
        self.frequency as f64 / (total_frequency * u64::from(MAX_FREQUENCY)) as f64
    }

    pub fn action(&self) -> PreFlopAction {
        self.action
    }

    pub fn range(&self) -> &PreFlopRangeTableWith<u16> {
        &self.range
    }

    pub fn ev(&self) -> Option<&PreFlopRangeTableWith<MilliBigBlind>> {
        self.ev.as_ref()
    }
}

#[derive(Debug, Clone)]
pub struct RangeConfigEntry {
    /// The initial small and big blind post is skipped.
    previous_actions: Vec<PreFlopAction>,
    total_range: PreFlopRangeTableWith<u16>,
    total_frequency: u64,
    actions: Vec<RangeAction>,
}

impl RangeConfigEntry {
    pub fn new(
        previous_actions: Vec<PreFlopAction>,
        total_range: PreFlopRangeTableWith<u16>,
        actions: Vec<RangeAction>,
        max_players: usize,
        depth: MilliBigBlind,
        small_blind: MilliBigBlind,
        round_up_frequencies: bool,
    ) -> Result<Self> {
        let mut entry = Self {
            previous_actions,
            total_range,
            total_frequency: 0,
            actions,
        };

        if round_up_frequencies {
            entry.round_up_frequencies()?;

            for action in &mut entry.actions {
                action.frequency = action.init_frequency(&entry.total_range);
            }
        }

        entry.total_frequency = entry.init_total_frequency();

        entry.validate(max_players, depth, small_blind)?;
        Ok(entry)
    }

    fn init_total_frequency(&self) -> u64 {
        self.total_range
            .iter()
            .map(|(entry, frequency)| u64::from(entry.combo_count()) * u64::from(*frequency))
            .sum()
    }

    fn round_up_frequencies(&mut self) -> Result<()> {
        if !self
            .actions
            .iter()
            .any(|action| action.action == PreFlopAction::Fold)
        {
            return Ok(());
        }

        for entry in PreFlopRangeTable::entries() {
            let total_frequencies = self
                .actions
                .iter()
                .map(|action| action.range[entry])
                .fold(Some(0u16), |acc, n| acc.and_then(|acc| acc.checked_add(n)));

            let Some(total_frequencies) = total_frequencies else {
                return Err(
                    "round frequencies: total frequencies of range entry overflowed".into(),
                );
            };

            if total_frequencies < MAX_FREQUENCY {
                let max_frequency_action = self
                    .actions
                    .iter_mut()
                    .max_by_key(|action| action.range()[entry])
                    .unwrap();

                max_frequency_action.range[entry] += MAX_FREQUENCY - total_frequencies;
            }
        }

        Ok(())
    }

    fn validate(
        &self,
        max_players: usize,
        depth: MilliBigBlind,
        small_blind: MilliBigBlind,
    ) -> Result<()> {
        let mut game = Self::build_game(max_players, depth, small_blind, &self.previous_actions)?;

        let actions: HashSet<_> = self.actions.iter().map(|action| action.action).collect();
        if actions.len() != self.actions.len() {
            return Err("range config entry: duplicate action".into());
        }

        for (_, frequency) in self.total_range.iter() {
            if *frequency > MAX_FREQUENCY {
                return Err(
                    "range config entry: range frequencies must be less than 10_000 (100%)".into(),
                );
            }
        }

        let mut total_frequencies: PreFlopRangeTableWith<u16> = PreFlopRangeTableWith::default();

        for action in &self.actions {
            action.action.apply_to_game(&mut game)?;
            game.undo().unwrap();

            for (entry, frequency) in action.range.iter() {
                if *frequency > MAX_FREQUENCY {
                    return Err(
                        "range config entry: all frequencies must be less than 10_000 (100%)"
                            .into(),
                    );
                }

                let Some(next_frequency) = total_frequencies[entry].checked_add(*frequency) else {
                    return Err("range config entry: total frequencies overflow".into());
                };
                if next_frequency > MAX_FREQUENCY {
                    return Err("range config entry: total frequencies overflow".into());
                }
                total_frequencies[entry] = next_frequency;
            }

            if action.frequency != action.init_frequency(&self.total_range) {
                return Err(format!(
                    "range config entry: bad action frequency: expected {}, got {}",
                    action.init_frequency(&self.total_range),
                    action.frequency
                )
                .into());
            }
        }

        let contains_fold = self
            .actions
            .iter()
            .any(|action| action.action == PreFlopAction::Fold);

        let valid_total_frequencies = if contains_fold {
            total_frequencies
                .iter()
                .all(|(_, frequency)| *frequency == MAX_FREQUENCY)
        } else {
            total_frequencies
                .iter()
                .all(|(_, frequency)| *frequency <= MAX_FREQUENCY)
        };
        if !valid_total_frequencies {
            return Err("range config entry: invalid total frequencies".into());
        }

        if self.total_frequency != self.init_total_frequency() {
            return Err("range config entry: bad total frequency".into());
        }

        Ok(())
    }

    pub fn build_game(
        max_players: usize,
        depth: MilliBigBlind,
        small_blind: MilliBigBlind,
        previous_actions: &[PreFlopAction],
    ) -> Result<Game> {
        let depth = u32::try_from(depth)?;
        let players = vec![Player::with_starting_stack(depth); max_players];
        let mut game = Game::new(&players, 0, u32::try_from(small_blind)?, 1_000)?;
        game.post_small_and_big_blind()?;

        for action in previous_actions.iter().copied() {
            action.apply_to_game(&mut game)?;
        }

        Ok(game)
    }

    pub fn possible_next_actions(
        &self,
        max_players: usize,
        depth: MilliBigBlind,
        small_blind: MilliBigBlind,
        min_frequency: f64,
    ) -> Result<Vec<PreFlopAction>> {
        let mut game = Self::build_game(max_players, depth, small_blind, &self.previous_actions)?;
        let mut possible_next_actions = Vec::new();

        for action in &self.actions {
            action.action.apply_to_game(&mut game)?;
            let next_state = game.state();
            game.undo().unwrap();

            if matches!(next_state, State::Player(_))
                && self.frequency(action.action) >= min_frequency
            {
                possible_next_actions.push(action.action);
            }
        }

        Ok(possible_next_actions)
    }

    pub fn previous_actions(&self) -> &[PreFlopAction] {
        &self.previous_actions
    }

    pub fn total_range(&self) -> &PreFlopRangeTableWith<u16> {
        &self.total_range
    }

    pub fn actions(&self) -> &[RangeAction] {
        &self.actions
    }

    pub fn frequency(&self, action: PreFlopAction) -> f64 {
        let action = self
            .actions()
            .iter()
            .filter(|current_action| current_action.action == action)
            .next();

        if let Some(action) = action {
            action.frequency(self.total_frequency)
        } else {
            0.0
        }
    }

    fn raise_diff_unchecked(&self, skip_players: usize, actions: &[Action]) -> u64 {
        let allowed_actions = &actions[2..];

        self.previous_actions
            .iter()
            .skip(skip_players)
            .zip(allowed_actions)
            .filter_map(|(expected, got)| match (*expected, *got) {
                // Super simple difference.
                (PreFlopAction::Raise(current_to), Action::Raise { to, .. }) => {
                    Some(i64::from(current_to).abs_diff(i64::from(to)))
                }
                (PreFlopAction::Raise(_), _) | (_, Action::Raise { .. }) => unreachable!(),
                _ => None,
            })
            .sum()
    }

    fn has_fold(&self) -> bool {
        self.actions
            .iter()
            .any(|action| action.action == PreFlopAction::Fold)
    }

    fn fold_frequency(&self, entry: RangeEntry) -> u16 {
        let fold_action = self
            .actions
            .iter()
            .find(|action| action.action == PreFlopAction::Fold);

        if let Some(fold_action) = fold_action {
            fold_action.range[entry]
        } else {
            let entry_frequency: u16 = self.actions.iter().map(|action| action.range[entry]).sum();
            MAX_FREQUENCY - entry_frequency
        }
    }

    pub fn pick(&self, rng: &mut impl Rng, entry: RangeEntry) -> PreFlopAction {
        if self.total_range[entry] == 0 {
            return PreFlopAction::Fold;
        }

        let additional_fold_weight = {
            let fold_weight = iter::once(self.fold_frequency(entry));
            if self.has_fold() {
                fold_weight.take(0)
            } else {
                fold_weight.take(1)
            }
        };

        let weights = self
            .actions
            .iter()
            .map(|action| action.range[entry])
            .chain(additional_fold_weight);
        let weighted_index = WeightedIndex::new(weights).unwrap();

        let action_index = weighted_index.sample(rng);
        if action_index >= self.actions.len() {
            PreFlopAction::Fold
        } else {
            self.actions[action_index].action
        }
    }

    pub fn from_data(
        data: RangeConfigEntryData,
        max_players: usize,
        depth: MilliBigBlind,
        small_blind: MilliBigBlind,
    ) -> Result<Self> {
        Self::new(
            data.previous_actions,
            data.total_range,
            data.actions,
            max_players,
            depth,
            small_blind,
            false,
        )
    }

    pub fn to_data(self) -> RangeConfigEntryData {
        RangeConfigEntryData {
            previous_actions: self.previous_actions,
            total_range: self.total_range,
            total_frequency: self.total_frequency,
            actions: self.actions,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeConfigEntryData {
    pub previous_actions: Vec<PreFlopAction>,
    pub total_range: PreFlopRangeTableWith<u16>,
    pub total_frequency: u64,
    pub actions: Vec<RangeAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeConfigData {
    pub description: Option<Arc<String>>,
    pub max_players: usize,
    pub depth: MilliBigBlind,
    pub small_blind: MilliBigBlind,
    pub ranges: Vec<RangeConfigEntryData>,
}

#[derive(Debug, Clone)]
pub struct RangeConfig {
    description: Option<Arc<String>>,
    max_players: usize,
    depth: MilliBigBlind,
    small_blind: MilliBigBlind,
    ranges: Vec<RangeConfigEntry>,
}

impl Default for RangeConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl RangeConfig {
    const DEFAULT: Self = Self {
        description: None,
        max_players: Game::MAX_PLAYERS,
        depth: 100_000,
        small_blind: 500,
        ranges: Vec::new(),
    };

    pub fn from_data(data: RangeConfigData) -> Result<Self> {
        if data.depth < 1_000 {
            return Err("range config: depth must be greater or equal to one big blind".into());
        }
        if data.max_players < Game::MIN_PLAYERS || data.max_players > Game::MAX_PLAYERS {
            return Err("range config: invalid max players value".into());
        }
        if data.small_blind <= 0 || data.small_blind > 1_000 {
            return Err("range config: invalid small blind size".into());
        }

        let mut previous_actions = HashSet::new();
        let mut ranges = Vec::new();
        for entry in data.ranges {
            if !previous_actions.insert(entry.previous_actions.clone()) {
                return Err("range config: contains duplicated previous actions entry".into());
            }
            let entry =
                RangeConfigEntry::from_data(entry, data.max_players, data.depth, data.small_blind)?;
            ranges.push(entry);
        }

        Ok(Self {
            description: data.description,
            max_players: data.max_players,
            depth: data.depth,
            small_blind: data.small_blind,
            ranges,
        })
    }

    pub fn to_data(self) -> RangeConfigData {
        RangeConfigData {
            description: self.description,
            max_players: self.max_players,
            depth: self.depth,
            small_blind: self.small_blind,
            ranges: self
                .ranges
                .into_iter()
                .map(|entry| entry.to_data())
                .collect(),
        }
    }

    pub fn by_game_action_kinds<'a>(
        &'a self,
        game: &'a Game,
    ) -> Result<impl Iterator<Item = &'a RangeConfigEntry> + 'a> {
        if game.player_count() > self.max_players {
            return Err("ranges by action kinds: more players than maximally allowed".into());
        }

        if game.is_heads_up_table() && self.max_players != 2 {
            return Err("ranges by action kinds: not heads up ranges".into());
        }

        if self.small_blind != game.amount_to_milli_big_blinds_rounded(game.small_blind()) {
            return Err("ranges by action kinds: small blind does not match".into());
        }

        // TODO: Pretty big limitation.
        let stack_depth_matches = game
            .starting_stacks()
            .iter()
            .copied()
            .all(|stack| game.amount_to_milli_big_blinds_rounded(stack) == self.depth);
        if !stack_depth_matches {
            return Err("ranges by action kinds: at least on stack has an unexpected depth".into());
        }

        let actions = game.actions();

        if game.state() == State::Post {
            return Err("ranges by action kinds: missing initial posts".into());
        }

        if game.board().street() != Street::PreFlop {
            return Err("ranges by action kinds: not pre flop".into());
        }

        let skip_players = self.max_players - game.player_count();

        let ranges = self.ranges.iter().filter(move |entry| {
            // The ranges might be slightly different for shorthanded gameplay.
            let skipped_players_folded = entry
                .previous_actions
                .iter()
                .copied()
                .take(skip_players)
                .all(|action| action == PreFlopAction::Fold);

            let kinds_match = entry.previous_actions.len() == actions.len() - 2
                && entry
                    .previous_actions
                    .iter()
                    .skip(skip_players)
                    .zip(&actions[2..])
                    .all(|(a, b)| a.is_action_kind(*b));

            skipped_players_folded && kinds_match
        });

        Ok(ranges)
    }

    pub fn by_game_best_fit_raise_simple<'a>(
        &'a self,
        game: &'a Game,
    ) -> Result<(&'a RangeConfigEntry, u64)> {
        let mut ranges = self.by_game_action_kinds(game)?;

        let Some(mut best_range) = ranges.next() else {
            return Err("range by actions: no range matches".into());
        };

        let skip_players = self.max_players.checked_sub(game.player_count()).unwrap();
        let mut best_diff = best_range.raise_diff_unchecked(skip_players, game.actions());

        for range in ranges {
            let current_diff = range.raise_diff_unchecked(skip_players, game.actions());
            if current_diff < best_diff {
                best_range = range;
                best_diff = current_diff;
            }
        }

        Ok((best_range, best_diff))
    }
}
