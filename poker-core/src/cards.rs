use std::{
    cmp::Ordering,
    collections::HashMap,
    fmt,
    ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not, Shl},
    ptr::addr_of_mut,
};

use crate::{card::Card, hand::Hand, rank::Rank, result::Result, suite::Suite};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score(u32);

impl Score {
    pub const ZERO: Score = Score(0);

    fn from_ranking_cards(ranking: HandRanking, cards: Cards) -> Self {
        let hand_ranking = ranking.to_u16();
        debug_assert_eq!(hand_ranking & 0xfff, hand_ranking);
        let mut n = u32::from(hand_ranking) << 20;
        assert!(cards.count() <= 5);
        let is_wheel = matches!(ranking, HandRanking::Straight | HandRanking::StraightFlush)
            && cards.by_rank() == CardsByRank::WHEEL;
        if !is_wheel {
            for (i, rank) in cards.by_rank().iter().enumerate() {
                n |= rank.to_u32() << (16 - i * 4);
            }
        }
        Score(n)
    }

    fn from_counts(counts: &[u8; Rank::COUNT]) -> Self {
        let mut cards = Cards::EMPTY;
        let mut suite = Suite::Diamonds;
        for rank in Rank::RANKS.iter().copied() {
            for _ in 0..counts[rank.to_usize()] {
                cards.add(Card::of(rank, suite));
                let next_suite_index = (suite.to_usize() + 1) % Suite::COUNT;
                suite = Suite::try_from(i8::try_from(next_suite_index).unwrap()).unwrap();
            }
        }
        assert!({
            let count = cards.count();
            count >= 5 && count <= 7
        });
        assert!(cards.flush().is_none());
        let top5 = cards.top5();
        assert!(matches!(
            top5.ranking,
            HandRanking::HighCard
                | HandRanking::OnePair(_)
                | HandRanking::TwoPair { .. }
                | HandRanking::ThreeOfAKind(_)
                | HandRanking::Straight
                | HandRanking::FullHouse { .. }
                | HandRanking::FourOfAKind(_)
        ));
        top5.to_score()
    }

    fn add_unchecked(self, rhs: Score) -> Score {
        Score(self.0 + rhs.0)
    }

    pub fn to_hand_ranking(self) -> HandRanking {
        assert_ne!(self, Self::ZERO);
        let n = u16::try_from((self.0 >> 20) & 0xfff).unwrap();
        HandRanking::from_u16(n).unwrap()
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HandRanking {
    HighCard = 0,
    OnePair(Rank) = 1,
    TwoPair { first: Rank, second: Rank } = 2,
    ThreeOfAKind(Rank) = 3,
    Straight = 4,
    Flush = 5,
    FullHouse { trips: Rank, pair: Rank } = 6,
    FourOfAKind(Rank) = 7,
    StraightFlush = 8,
    RoyalFlush = 9,
}

impl HandRanking {
    fn to_u16(self) -> u16 {
        match self {
            HandRanking::HighCard => 0,
            HandRanking::OnePair(pair) => (1 << 8) | pair.to_u16(),
            HandRanking::TwoPair { first, second } => {
                (2 << 8) | (first.to_u16() << 4) | second.to_u16()
            }
            HandRanking::ThreeOfAKind(trips) => (3 << 8) | trips.to_u16(),
            HandRanking::Straight => 4 << 8,
            HandRanking::Flush => 5 << 8,
            HandRanking::FullHouse { trips, pair } => {
                (6 << 8) | (trips.to_u16() << 4) | pair.to_u16()
            }
            HandRanking::FourOfAKind(quads) => (7 << 8) | quads.to_u16(),
            HandRanking::StraightFlush => 8 << 8,
            HandRanking::RoyalFlush => 9 << 8,
        }
    }

    fn from_u16(n: u16) -> Option<Self> {
        let ranking = match n >> 8 {
            0 => HandRanking::HighCard,
            1 => {
                let pair = Rank::try_from(i8::try_from(n & 0xf).unwrap()).ok()?;
                HandRanking::OnePair(pair)
            }
            2 => {
                let first = Rank::try_from(i8::try_from((n >> 4) & 0xf).unwrap()).ok()?;
                let second = Rank::try_from(i8::try_from(n & 0xf).unwrap()).ok()?;
                HandRanking::TwoPair { first, second }
            }
            3 => {
                let trips = Rank::try_from(i8::try_from(n & 0xf).unwrap()).ok()?;
                HandRanking::ThreeOfAKind(trips)
            }
            4 => HandRanking::Straight,
            5 => HandRanking::Flush,
            6 => {
                let trips = Rank::try_from(i8::try_from((n >> 4) & 0xf).unwrap()).ok()?;
                let pair = Rank::try_from(i8::try_from(n & 0xf).unwrap()).ok()?;
                HandRanking::FullHouse { trips, pair }
            }
            7 => {
                let quads = Rank::try_from(i8::try_from(n & 0xf).unwrap()).ok()?;
                HandRanking::FourOfAKind(quads)
            }
            8 => HandRanking::StraightFlush,
            9 => HandRanking::RoyalFlush,
            _ => return None,
        };
        Some(ranking)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Top5 {
    ranking: HandRanking,
    cards: Cards,
}

impl Top5 {
    pub fn worst() -> Self {
        let cards = Cards::from_slice(&[
            Card::of(Rank::Seven, Suite::Clubs),
            Card::of(Rank::Five, Suite::Diamonds),
            Card::of(Rank::Four, Suite::Hearts),
            Card::of(Rank::Three, Suite::Spades),
            Card::of(Rank::Two, Suite::Clubs),
        ])
        .unwrap();
        let top5 = cards.top5();
        debug_assert!(matches!(top5.ranking, HandRanking::HighCard));
        debug_assert!(top5.cards.by_rank().highest_rank() == Some(Rank::Seven));
        top5
    }

    fn of(ranking: HandRanking, cards: Cards) -> Self {
        debug_assert!(cards.count() <= 5);
        Self { ranking, cards }
    }

    pub fn to_score(self) -> Score {
        Score::from_ranking_cards(self.ranking, self.cards)
    }
}

impl PartialOrd for Top5 {
    fn partial_cmp(&self, villain: &Self) -> Option<Ordering> {
        Some(self.cmp(villain))
    }
}

impl Ord for Top5 {
    fn cmp(&self, villain: &Self) -> Ordering {
        self.to_score().cmp(&villain.to_score())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cards(u64);

impl fmt::Display for Cards {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in self.by_rank().iter() {
            for suite in Suite::SUITES {
                let card = Card::of(rank, suite);
                if self.has(card) {
                    write!(f, "{card}")?;
                }
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Cards {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self, f)
    }
}

impl BitAnd<Cards> for Cards {
    type Output = Cards;

    fn bitand(self, rhs: Cards) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitOr<Cards> for Cards {
    type Output = Cards;

    fn bitor(self, rhs: Cards) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign<Cards> for Cards {
    fn bitor_assign(&mut self, rhs: Cards) {
        self.0 |= rhs.0;
    }
}

impl Not for Cards {
    type Output = Cards;

    fn not(self) -> Self::Output {
        Self((!self.0) & Self::MASK_FULL)
    }
}

fn interleave_first_32_bits_with_zeros(mut n: u64) -> u64 {
    n = (n ^ (n << 16)) & 0x0000ffff0000ffff;
    n = (n ^ (n << 8)) & 0x00ff00ff00ff00ff;
    n = (n ^ (n << 4)) & 0x0f0f0f0f0f0f0f0f;
    n = (n ^ (n << 2)) & 0x3333333333333333;
    n = (n ^ (n << 1)) & 0x5555555555555555;
    n
}

static mut CARDS_SCORE_MAP: Option<&'static HashMap<u64, Score>> = None;

const FLUSH_MAP_SIZE: usize = (Cards::MASK_SINGLE + 1) as usize;

static mut CARDS_FLUSH_MAP: [Score; FLUSH_MAP_SIZE] = [Score::ZERO; FLUSH_MAP_SIZE];

impl Cards {
    pub const EMPTY: Self = Cards(0);

    pub const MASK_SINGLE: u64 = 0b1_1111_1111_1111;

    pub const MASK_FULL: u64 = Cards::MASK_SINGLE << 48
        | Cards::MASK_SINGLE << 32
        | Cards::MASK_SINGLE << 16
        | Cards::MASK_SINGLE;

    pub fn from_str(s: &str) -> Result<Self> {
        Self::from_bytes(s.as_bytes())
    }

    pub fn from_bytes(s: &[u8]) -> Result<Self> {
        if s == b"none" {
            return Ok(Cards::EMPTY);
        }

        if s.len() % 2 != 0 {
            return Err(
                format!("invalid cards '{}': bad length", String::from_utf8_lossy(s)).into(),
            );
        }
        if !s.is_ascii() {
            return Err(
                format!("invalid cards '{}': not ascii", String::from_utf8_lossy(s)).into(),
            );
        }
        let mut cards = Self::EMPTY;
        for i in (0..s.len()).step_by(2) {
            let card_raw = &s[i..i + 2];
            let card = Card::from_bytes(card_raw)?;
            if !cards.try_add(card) {
                return Err(format!(
                    "invalid cards '{}': duplicate card {card}",
                    String::from_utf8_lossy(s),
                )
                .into());
            };
        }
        Ok(cards)
    }

    pub fn from_slice(s: &[Card]) -> Option<Self> {
        let mut cards = Self::EMPTY;
        for card in s.iter().copied() {
            if cards.has(card) {
                return None;
            }
            cards.add(card);
        }
        Some(cards)
    }

    fn of_rank(rank: Rank) -> Self {
        Cards::from_slice(&[
            Card::of(rank, Suite::Diamonds),
            Card::of(rank, Suite::Spades),
            Card::of(rank, Suite::Hearts),
            Card::of(rank, Suite::Clubs),
        ])
        .unwrap()
    }

    pub fn to_hand(self) -> Option<Hand> {
        let mut iter = self.iter();
        let (a, b) = (iter.next()?, iter.next()?);
        if iter.next().is_some() {
            None
        } else {
            Hand::of_two_cards(a, b)
        }
    }

    fn to_u64(self) -> u64 {
        self.0
    }

    pub fn first(self) -> Option<Card> {
        let index = 63 - self.0.leading_zeros() as i8;
        Card::from_index(index)
    }

    pub fn has(self, card: Card) -> bool {
        (self.0 & (1 << card.to_index_u64())) != 0
    }

    pub fn try_add(&mut self, card: Card) -> bool {
        if self.has(card) {
            false
        } else {
            self.0 |= 1 << card.to_index_u64();
            true
        }
    }

    pub fn add(&mut self, card: Card) {
        assert!(self.try_add(card));
    }

    pub fn with(self, card: Card) -> Self {
        assert!(!self.has(card));
        self.with_unchecked(card)
    }

    pub fn with_unchecked(self, card: Card) -> Self {
        Self(self.0 | (1 << card.to_index_u64()))
    }

    pub fn remove(&mut self, card: Card) {
        assert!(self.has(card));
        self.0 &= !(1 << card.to_index_u64())
    }

    pub fn without(mut self, card: Card) -> Self {
        self.remove(card);
        self
    }

    fn without_rank(self, rank: Rank) -> Self {
        self & !Self::of_rank(rank)
    }

    pub fn count(self) -> u8 {
        self.0.count_ones() as u8
    }

    pub fn by_rank(self) -> CardsByRank {
        CardsByRank::from_cards(self)
    }

    fn take_n(self, n: u8) -> Self {
        let mut out = Self::EMPTY;
        for card in self.iter().take(n.into()) {
            out.add(card);
        }
        out
    }

    fn suites(self) -> impl Iterator<Item = (Suite, CardsByRank)> {
        Suite::SUITES
            .iter()
            .copied()
            .map(move |suite| (suite, CardsByRank::from_cards_suite(self, suite)))
    }

    fn score_map() -> &'static HashMap<u64, Score> {
        unsafe { CARDS_SCORE_MAP.unwrap() }
    }

    fn flush_map_get(cards: CardsByRank) -> Score {
        unsafe { CARDS_FLUSH_MAP[cards.to_usize()] }
    }

    pub fn score_fast(self) -> Score {
        assert!({
            let count = self.count();
            count >= 5 && count <= 7
        });
        let counts_n = self.counts_n_fast();
        let score = Self::score_map()[&counts_n];
        if !self.is_flush() {
            return score;
        }

        let mut score = Score::ZERO;
        for suite in Suite::SUITES {
            let cards = CardsByRank::from_cards_suite(self, suite);
            let suite_score = Self::flush_map_get(cards);
            score = score.add_unchecked(suite_score);
        }
        debug_assert_eq!(self.top5().to_score(), score);
        score
    }

    pub(crate) unsafe fn init() {
        {
            assert_eq!(CARDS_FLUSH_MAP[0b11111], Score::ZERO);
            let flush_map = &mut (*addr_of_mut!(CARDS_FLUSH_MAP));
            Self::init_flush_map(flush_map);
        }
        let score_map = Self::build_score_map();
        {
            let score_map = &(*addr_of_mut!(CARDS_SCORE_MAP));
            assert!(score_map.is_none());
        }
        CARDS_SCORE_MAP = Some(Box::leak(Box::new(score_map)));
    }

    fn init_flush_map(map: &mut [Score; FLUSH_MAP_SIZE]) {
        for n in 0..FLUSH_MAP_SIZE {
            if n.count_ones() < 5 {
                map[n] = Score::ZERO;
            } else {
                let cards = CardsByRank::from_raw(i16::try_from(n).unwrap())
                    .to_cards_suite(Suite::Diamonds);
                let top5 = cards.top5();
                assert!(matches!(
                    top5.ranking,
                    HandRanking::Flush | HandRanking::StraightFlush | HandRanking::RoyalFlush
                ));
                map[n] = top5.to_score();
            }
        }
    }

    fn build_score_map() -> HashMap<u64, Score> {
        let mut map = HashMap::new();
        Self::score_map_recursive(&mut map, 0, &mut [0u8; Rank::COUNT], Rank::COUNT);
        map
    }

    fn score_map_recursive(
        map: &mut HashMap<u64, Score>,
        old_count: u8,
        counts: &mut [u8; Rank::COUNT],
        remainder: usize,
    ) {
        for n in 0..=4 {
            let index = Rank::COUNT - remainder;
            counts[index] = n;
            let next_count = old_count + n;
            if remainder == 1 {
                if next_count < 5 || next_count > 7 {
                    continue;
                }
                let counts_n = Self::counts_n(counts);
                let score = Score::from_counts(counts);
                assert!(map.insert(counts_n, score).is_none());
            } else {
                if next_count > 7 {
                    break;
                }
                Self::score_map_recursive(map, next_count, counts, remainder - 1);
            }
        }
    }

    fn counts_n(counts: &[u8; Rank::COUNT]) -> u64 {
        let mut counts_n = 0u64;
        for (index, count) in counts.iter().copied().enumerate() {
            counts_n |= u64::from(count) << (index * 4);
        }
        counts_n
    }

    fn counts_n_fast(self) -> u64 {
        let mut counts_n = 0u64;
        for suite in Suite::SUITES {
            let cards = CardsByRank::from_cards_suite(self, suite);
            let n = interleave_first_32_bits_with_zeros(interleave_first_32_bits_with_zeros(
                cards.to_u64(),
            ));
            counts_n += n;
        }
        debug_assert_eq!(counts_n, Self::counts_n(&self.counts()));
        counts_n
    }

    pub fn top5(self) -> Top5 {
        let counts = self.counts();
        if let Some(cards) = self.straight_flush() {
            let by_rank = cards.by_rank();
            if by_rank.highest_rank().is_some_and(|rank| rank == Rank::Ace)
                && by_rank != CardsByRank::WHEEL
            {
                Top5::of(HandRanking::RoyalFlush, cards)
            } else {
                Top5::of(HandRanking::StraightFlush, cards)
            }
        } else if let Some((rank, cards)) = self.quads(counts) {
            Top5::of(HandRanking::FourOfAKind(rank), cards)
        } else if let Some((trips, pair, cards)) = self.full_house(counts) {
            Top5::of(HandRanking::FullHouse { trips, pair }, cards)
        } else if let Some(cards) = self.flush() {
            Top5::of(HandRanking::Flush, cards)
        } else if let Some(cards) = self.straight() {
            Top5::of(HandRanking::Straight, cards)
        } else if let Some((rank, cards)) = self.trips(counts) {
            Top5::of(HandRanking::ThreeOfAKind(rank), cards)
        } else if let Some(top5) = self.pair(counts) {
            top5
        } else {
            Top5::of(HandRanking::HighCard, self.kickers(5))
        }
    }

    fn kickers(self, count: u8) -> Self {
        let mut kickers = Self::EMPTY;
        let mut remaining = count;
        for rank in self.by_rank().iter() {
            let next = (self & Self::of_rank(rank)).take_n(remaining);
            remaining -= next.count();
            kickers |= next;
            if remaining == 0 {
                return kickers;
            }
        }
        kickers
    }

    fn pair(self, counts: [u8; Rank::COUNT]) -> Option<Top5> {
        let Some(first_pair_rank) = Self::best_n(counts, 2) else {
            return None;
        };
        let first_pair = (self & Cards::of_rank(first_pair_rank)).take_n(2);

        let second_pair_rank = {
            let mut counts_without_first_pair = counts;
            counts_without_first_pair[first_pair_rank.to_usize()] = 0;
            match Self::best_n(counts_without_first_pair, 2) {
                Some(rank) => rank,
                None => {
                    let kickers = self.without_rank(first_pair_rank).kickers(3);
                    let cards = first_pair | kickers;
                    return Some(Top5::of(HandRanking::OnePair(first_pair_rank), cards));
                }
            }
        };
        let second_pair = (self & Cards::of_rank(second_pair_rank)).take_n(2);

        let kicker = self
            .without_rank(first_pair_rank)
            .without_rank(second_pair_rank)
            .kickers(1);
        let cards = first_pair | second_pair | kicker;
        let ranking = HandRanking::TwoPair {
            first: first_pair_rank,
            second: second_pair_rank,
        };
        Some(Top5::of(ranking, cards))
    }

    fn trips(self, counts: [u8; Rank::COUNT]) -> Option<(Rank, Self)> {
        if let Some(trips_rank) = Self::best_n(counts, 3) {
            let trips = (self & Cards::of_rank(trips_rank)).take_n(3);
            let kickers = self.without_rank(trips_rank).kickers(2);
            Some((trips_rank, trips | kickers))
        } else {
            None
        }
    }

    fn straight(self) -> Option<Self> {
        let Some(straight) = self.by_rank().straight() else {
            return None;
        };
        let mut out = Self::EMPTY;
        for rank in straight.iter() {
            out |= (self & Self::of_rank(rank)).take_n(1);
        }
        Some(out)
    }

    fn is_flush(self) -> bool {
        let mut is_flush = false;
        for suite in Suite::SUITES {
            let cards = CardsByRank::from_cards_suite(self, suite);
            is_flush |= cards.count() >= 5;
        }
        is_flush
    }

    fn flush(self) -> Option<Self> {
        let mut flush = None;
        for (suite, cards) in self.suites() {
            if cards.count() >= 5 {
                assert!(flush.is_none());
                flush = Some(cards.take_top_n(5).to_cards_suite(suite));
            }
        }
        flush
    }

    fn full_house(self, counts: [u8; Rank::COUNT]) -> Option<(Rank, Rank, Self)> {
        let Some(trips_rank) = Self::best_n(counts, 3) else {
            return None;
        };
        let pair_rank = {
            let mut counts_without_trips = counts;
            counts_without_trips[trips_rank.to_usize()] = 0;
            match Self::best_n(counts_without_trips, 2) {
                Some(rank) => rank,
                None => return None,
            }
        };
        let trips = (self & Cards::of_rank(trips_rank)).take_n(3);
        let pair = (self & Cards::of_rank(pair_rank)).take_n(2);
        Some((trips_rank, pair_rank, trips | pair))
    }

    fn quads(self, counts: [u8; Rank::COUNT]) -> Option<(Rank, Self)> {
        if let Some(quads_rank) = Self::best_n(counts, 4) {
            let mut quads = Cards::of_rank(quads_rank);
            let mut kicker_cards = self.by_rank();
            kicker_cards.remove(quads_rank);
            if let Some(kicker) = kicker_cards.highest_rank() {
                quads |= (self & Self::of_rank(kicker)).take_n(1);
            }
            Some((quads_rank, quads))
        } else {
            None
        }
    }

    fn best_n(counts: [u8; Rank::COUNT], n: u8) -> Option<Rank> {
        let mut best_index = None;
        for (index, count) in counts.iter().copied().enumerate() {
            if count >= n {
                let index = i8::try_from(index).unwrap();
                best_index = Some(index);
            }
        }
        best_index.map(|index| Rank::try_from(index).unwrap())
    }

    fn counts(self) -> [u8; Rank::COUNT] {
        let mut counts = [0; Rank::COUNT];
        for card in self.iter() {
            counts[card.rank().to_usize()] += 1;
        }
        counts
    }

    fn straight_flush(self) -> Option<Self> {
        let mut straight_flush = None;
        for (suite, cards) in self.suites() {
            if let Some(straight) = cards.straight() {
                assert!(straight_flush.is_none());
                straight_flush = Some(straight.to_cards_suite(suite));
            }
        }
        straight_flush
    }

    pub fn iter(self) -> CardsIter {
        CardsIter(self)
    }

    pub fn unify_suites(self) -> Self {
        let mut suite_relevance = Suite::SUITES;
        suite_relevance.sort_by_key(|suite| {
            let by_rank = CardsByRank::from_cards_suite(self, *suite);
            (by_rank.count(), by_rank)
        });

        let mut suite_mapping = Suite::SUITES;
        for (index, suite) in suite_relevance.into_iter().rev().enumerate() {
            suite_mapping[suite.to_usize()] = Suite::SUITES[index];
        }

        let mut out = Self::EMPTY;
        for card in self.iter() {
            let card = Card::of(card.rank(), suite_mapping[card.suite().to_usize()]);
            out.add(card);
        }
        out
    }

    pub fn suite_count(self) -> u8 {
        let n = self
            .suites()
            .filter(|(_, by_rank)| !by_rank.is_empty())
            .count();
        u8::try_from(n).unwrap()
    }

    pub fn rank_highest_count(self) -> u8 {
        self.counts().iter().max().copied().unwrap()
    }
}

pub struct CardsIter(Cards);

impl Iterator for CardsIter {
    type Item = Card;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.first() {
            Some(card) => {
                self.0.remove(card);
                Some(card)
            }
            None => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CardsByRank(i16);

impl fmt::Display for CardsByRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut ranks = self.iter().peekable();
        write!(f, "[")?;
        while let Some(rank) = ranks.next() {
            write!(f, "{rank}")?;
            if ranks.peek().is_some() {
                write!(f, " ")?;
            }
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl BitAnd<CardsByRank> for CardsByRank {
    type Output = CardsByRank;

    fn bitand(self, rhs: CardsByRank) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign<CardsByRank> for CardsByRank {
    fn bitand_assign(&mut self, rhs: CardsByRank) {
        self.0 &= rhs.0;
    }
}

impl BitOr<CardsByRank> for CardsByRank {
    type Output = CardsByRank;

    fn bitor(self, rhs: CardsByRank) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign<CardsByRank> for CardsByRank {
    fn bitor_assign(&mut self, rhs: CardsByRank) {
        self.0 |= rhs.0;
    }
}

impl Shl<i8> for CardsByRank {
    type Output = CardsByRank;

    fn shl(self, rhs: i8) -> Self::Output {
        Self(self.0 << rhs)
    }
}

impl CardsByRank {
    pub const EMPTY: Self = CardsByRank(0);

    const WHEEL: Self = Self(0b1_0000_0000_1111);
    const STRAIGHT_SIX_HIGH: Self = Self(0b11111);

    fn from_raw(n: i16) -> Self {
        debug_assert!(if n.is_negative() {
            false
        } else {
            let n_u64 = u64::try_from(n).unwrap();
            Cards::MASK_SINGLE & n_u64 == n_u64
        });
        CardsByRank(n)
    }

    fn from_cards(cards: Cards) -> Self {
        let n = cards.to_u64();
        let collapsed = (n | (n >> 16) | (n >> 32) | (n >> 48)) & Cards::MASK_SINGLE;
        CardsByRank(collapsed as i16)
    }

    fn from_cards_suite(cards: Cards, suite: Suite) -> CardsByRank {
        let rank = (cards.to_u64() >> suite.to_index()) & Cards::MASK_SINGLE;
        CardsByRank(rank as i16)
    }

    fn to_cards_suite(self, suite: Suite) -> Cards {
        Cards((self.0 as u64) << suite.to_index_u64())
    }

    pub fn highest_rank(self) -> Option<Rank> {
        Rank::try_from(15 - self.0.leading_zeros() as i8).ok()
    }

    pub fn has(self, rank: Rank) -> bool {
        (self.0 & (1 << rank.to_i16())) != 0
    }

    pub fn add(&mut self, rank: Rank) {
        assert!(self.try_add(rank));
    }

    pub fn try_add(&mut self, rank: Rank) -> bool {
        if self.has(rank) {
            false
        } else {
            self.0 |= 1 << rank.to_i16();
            true
        }
    }

    pub fn remove(&mut self, rank: Rank) {
        assert!(self.has(rank));
        self.0 &= !(1 << rank.to_i16());
    }

    pub fn without(self, rank: Rank) -> Self {
        assert!(self.has(rank));
        Self(self.0 & !(1 << rank.to_i16()))
    }

    fn iter(self) -> CardsByRankIter {
        CardsByRankIter(self)
    }

    fn straight(self) -> Option<Self> {
        let mut best_cards = None;
        if self & Self::WHEEL == Self::WHEEL {
            best_cards = Some(Self::WHEEL);
        }
        for shift in 0..=13 - 5 {
            let straight = Self::STRAIGHT_SIX_HIGH << shift;
            if self & straight == straight {
                best_cards = Some(straight);
            }
        }
        best_cards
    }

    pub fn is_empty(self) -> bool {
        self == Self::EMPTY
    }

    pub fn count(self) -> i8 {
        self.0.count_ones() as i8
    }

    pub fn count_u8(self) -> u8 {
        self.count() as u8
    }

    fn to_u64(self) -> u64 {
        self.0 as u64
    }

    fn to_usize(self) -> usize {
        self.0 as usize
    }

    fn take_top_n(self, n: u8) -> Self {
        let mut out = Self::EMPTY;
        for rank in self.iter().take(n.into()) {
            out.add(rank);
        }
        out
    }
}

struct CardsByRankIter(CardsByRank);

impl Iterator for CardsByRankIter {
    type Item = Rank;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0.highest_rank() {
            Some(rank) => {
                self.0.remove(rank);
                Some(rank)
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_unify_suites() {
        unsafe {
            crate::init::init();
        }

        let flops: HashSet<_> = Card::all()
            .flat_map(|a| Card::all().map(move |b| (a, b)))
            .flat_map(|(a, b)| Card::all().map(move |c| (a, b, c)))
            .filter_map(|(a, b, c)| Cards::from_slice(&[a, b, c]))
            .map(|cards| cards.unify_suites())
            .collect();
        assert_eq!(flops.len(), 1755);
    }
}
