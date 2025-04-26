use core::fmt;

use rand::{rngs::SmallRng, seq::SliceRandom, SeedableRng};

use crate::{
    card::Card,
    cards::{Cards, Score},
    deck::Deck,
    hand::Hand,
    range::{RangeTable, RangeTableWith},
};

fn try_u64_to_f64(n: u64) -> Option<f64> {
    const F64_MAX_SAFE_INT: u64 = 2 << 53;
    if (F64_MAX_SAFE_INT - 1) & n != n {
        None
    } else {
        Some(n as f64)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Equity {
    win_percent: f64,
    tie_percent: f64,
}

impl fmt::Display for Equity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "equity={:2.2} win={:2.2} tie={:2.2}",
            self.equity_percent() * 100.0,
            self.win_percent() * 100.0,
            self.tie_percent() * 100.0,
        )
    }
}

fn valid_input(community_cards: Cards, ranges: &[impl AsRef<RangeTable>]) -> bool {
    community_cards.count() <= 5
        && ranges.len() >= 2
        && ranges.len() <= 9
        && ranges.iter().all(|range| !range.as_ref().is_empty())
}

fn total_combos_upper_bound(community_cards: Cards, ranges: &[impl AsRef<RangeTable>]) -> u128 {
    assert!(ranges.len() <= 9);
    assert!(ranges.iter().all(|range| !range.as_ref().is_empty()));
    let community_cards_count = community_cards.count();
    assert!(community_cards_count <= 5);
    let mut remaining_cards = {
        let remaining_cards = Card::COUNT - usize::from(community_cards_count);
        u128::try_from(remaining_cards).unwrap()
    };
    let mut count = 1u128;

    for _ in community_cards_count..5 {
        count *= remaining_cards;
        remaining_cards -= 1;
    }

    for range in ranges {
        count = count
            .checked_mul(u128::from(range.as_ref().count()))
            .unwrap();
    }

    count
}

impl Equity {
    fn from_total_wins_ties(total: u64, wins: &[u64], ties: &[f64]) -> Vec<Self> {
        assert_ne!(total, 0);
        assert_eq!(wins.len(), ties.len());
        let total = try_u64_to_f64(total).unwrap();
        let mut equities = Vec::with_capacity(wins.len());
        for (wins, ties) in wins.iter().copied().zip(ties.iter().copied()) {
            equities.push(Equity {
                win_percent: try_u64_to_f64(wins).unwrap() / total,
                tie_percent: ties / total,
            });
        }
        equities
    }

    fn from_total_wins_ties_simulate(total: f64, wins: &[f64], ties: &[f64]) -> Vec<Self> {
        assert_ne!(total, 0.0);
        assert_eq!(wins.len(), ties.len());
        let mut equities = Vec::with_capacity(wins.len());
        for (wins, ties) in wins.iter().copied().zip(ties.iter().copied()) {
            equities.push(Equity {
                win_percent: wins / total,
                tie_percent: ties / total,
            });
        }
        equities
    }

    pub fn enumerate(
        community_cards: Cards,
        ranges: &[impl AsRef<RangeTable>],
    ) -> Option<Vec<Equity>> {
        let mut wins = vec![0; ranges.len()];
        let mut ties = vec![0.0; ranges.len()];
        let equity_calculator = EquityCalculator::new(community_cards, ranges, |_, scores| {
            showdown(scores, &mut wins, &mut ties)
        })?;
        let total = equity_calculator.enumerate()?;
        Some(Self::from_total_wins_ties(total, &wins, &ties))
    }

    pub fn simulate(
        start_community_cards: Cards,
        ranges: &[impl AsRef<RangeTable>],
        rounds: u64,
    ) -> Option<Vec<Equity>> {
        let mut wins = vec![0.0; ranges.len()];
        let mut ties = vec![0.0; ranges.len()];
        let total = simulate(start_community_cards, ranges, rounds, |_, scores, diff| {
            showdown_simulate(scores, &mut wins, &mut ties, diff);
        })?;
        Some(Self::from_total_wins_ties_simulate(total, &wins, &ties))
    }

    pub fn equity_percent(self) -> f64 {
        self.win_percent + self.tie_percent
    }

    pub fn win_percent(self) -> f64 {
        self.win_percent
    }

    pub fn tie_percent(self) -> f64 {
        self.tie_percent
    }
}

pub struct EquityTable {
    total_win_percent: f64,
    total_tie_percent: f64,
    totals: RangeTableWith<f64>,
    wins: RangeTableWith<f64>,
    ties: RangeTableWith<f64>,
}

impl EquityTable {
    fn from_totals_wins_ties(
        total: f64,
        totals: Vec<RangeTableWith<f64>>,
        wins: Vec<RangeTableWith<f64>>,
        ties: Vec<RangeTableWith<f64>>,
    ) -> Vec<Self> {
        assert_ne!(total, 0.0);
        assert_eq!(totals.len(), wins.len());
        assert_eq!(wins.len(), ties.len());

        let mut equity_tables = Vec::with_capacity(wins.len());
        let iter = totals
            .into_iter()
            .zip(wins.into_iter())
            .zip(ties.into_iter());

        for ((totals, wins), ties) in iter {
            let total_wins: f64 = wins.iter().map(|(_, wins)| *wins).sum();
            let total_ties: f64 = ties.iter().map(|(_, ties)| *ties).sum();

            equity_tables.push(EquityTable {
                total_win_percent: total_wins / total,
                total_tie_percent: total_ties / total,
                totals,
                wins,
                ties,
            });
        }

        equity_tables
    }

    pub fn enumerate(
        community_cards: Cards,
        ranges: &[impl AsRef<RangeTable>],
    ) -> Option<Vec<EquityTable>> {
        let mut totals = vec![RangeTableWith::default(); ranges.len()];
        let mut wins = vec![RangeTableWith::default(); ranges.len()];
        let mut ties = vec![RangeTableWith::default(); ranges.len()];
        let equity_calculator = EquityCalculator::new(community_cards, ranges, |hands, scores| {
            showdown_table(hands, scores, &mut totals, &mut wins, &mut ties, 1.0);
        })?;
        let total = equity_calculator.enumerate()?;
        let total = try_u64_to_f64(total).unwrap();
        Some(Self::from_totals_wins_ties(total, totals, wins, ties))
    }

    pub fn simulate(
        start_community_cards: Cards,
        ranges: &[impl AsRef<RangeTable>],
        rounds: u64,
    ) -> Option<Vec<Self>> {
        let mut totals = vec![RangeTableWith::default(); ranges.len()];
        let mut wins = vec![RangeTableWith::default(); ranges.len()];
        let mut ties = vec![RangeTableWith::default(); ranges.len()];
        let total = simulate(
            start_community_cards,
            ranges,
            rounds,
            |hands, scores, diff| {
                showdown_table(hands, scores, &mut totals, &mut wins, &mut ties, diff);
            },
        )?;
        Some(Self::from_totals_wins_ties(total, totals, wins, ties))
    }

    pub fn total_equity(&self) -> Equity {
        Equity {
            win_percent: self.total_win_percent(),
            tie_percent: self.total_tie_percent(),
        }
    }

    pub fn total_equity_percent(&self) -> f64 {
        self.total_win_percent() + self.total_tie_percent()
    }

    pub fn total_win_percent(&self) -> f64 {
        self.total_win_percent
    }

    pub fn total_tie_percent(&self) -> f64 {
        self.total_tie_percent
    }

    pub fn equity(&self, hand: Hand) -> Equity {
        Equity {
            win_percent: self.win_percent(hand),
            tie_percent: self.tie_percent(hand),
        }
    }

    pub fn equity_percent(&self, hand: Hand) -> f64 {
        self.win_percent(hand) + self.tie_percent(hand)
    }

    pub fn win_percent(&self, hand: Hand) -> f64 {
        if self.totals[hand] == 0.0 {
            0.0
        } else {
            self.wins[hand] / self.totals[hand]
        }
    }

    pub fn tie_percent(&self, hand: Hand) -> f64 {
        if self.totals[hand] == 0.0 {
            0.0
        } else {
            self.ties[hand] / self.totals[hand]
        }
    }

    pub fn has_data(&self, hand: Hand) -> bool {
        self.totals[hand] != 0.0
    }
}

fn simulate(
    start_community_cards: Cards,
    ranges: &[impl AsRef<RangeTable>],
    rounds: u64,
    mut f: impl FnMut(&[Hand], &[Score], f64),
) -> Option<f64> {
    if !valid_input(start_community_cards, ranges) {
        return None;
    }
    if rounds == 0 {
        return None;
    }

    let mut rng = SmallRng::from_entropy();
    let remaining_community_cards = 5 - start_community_cards.count();
    let player_count = ranges.len();
    let full_ranges_original: Vec<_> = ranges
        .iter()
        .map(|r| r.as_ref().into_iter().collect::<Vec<_>>())
        .collect();
    let mut full_ranges = full_ranges_original.clone();

    let mut hands = vec![Hand::UNDEFINED; player_count];
    let mut scores = vec![Score::ZERO; player_count];
    let mut deck = Deck::from_cards(&mut rng, start_community_cards);
    let mut total = 0.0;

    let community_card_factor: u64 = {
        let x = u64::try_from(Card::COUNT).unwrap() - u64::from((start_community_cards).count());
        ((x - u64::from(remaining_community_cards)) + 1..=x).product()
    };
    // We accept that this might loose precision here.
    let upper_bound = total_combos_upper_bound(start_community_cards, ranges) as f64;

    'outer: for _ in 0..rounds {
        deck.reset();

        let community_cards = {
            let mut community_cards = start_community_cards;
            for _ in 0..remaining_community_cards {
                community_cards.add(deck.draw(&mut rng).unwrap());
            }
            community_cards
        };

        let mut seen_cards = community_cards;
        let mut factor = u128::from(community_card_factor);
        for (i, range) in full_ranges.iter_mut().enumerate() {
            let range = filter_hands(&full_ranges_original[i], range, seen_cards);
            factor *= u128::try_from(range.len()).unwrap();
            let Some(hand) = range.choose(&mut rng).copied() else {
                continue 'outer;
            };
            hands[i] = hand;
            scores[i] = community_cards
                .with_unchecked(hand.high())
                .with_unchecked(hand.low())
                .score_fast();
            seen_cards.try_add(hand.high());
            seen_cards.try_add(hand.low());
        }

        // We accept that this might loose precision here.
        let diff = factor as f64 / upper_bound;
        f(&hands, &scores, diff);
        total += diff;
    }

    if total == 0.0 {
        None
    } else {
        Some(total)
    }
}

fn filter_hands<'a>(
    original_range: &[Hand],
    output_range: &'a mut [Hand],
    seen_cards: Cards,
) -> &'a [Hand] {
    let mut out_index = 0;
    for hand in original_range.iter().copied() {
        output_range[out_index] = hand;
        let valid = !seen_cards.has(hand.high()) & !seen_cards.has(hand.low());
        out_index += usize::from(valid);
    }
    &output_range[..out_index]
}

struct EquityCalculator<'a, RT: AsRef<RangeTable>, F: FnMut(&[Hand], &[Score])> {
    known_cards: Cards,
    visited_community_cards: Cards,
    community_cards: Cards,
    ranges: &'a [RT],
    hands: Vec<Hand>,
    hand_ranking_scores: Vec<Score>,
    total: u64,
    f: F,
}

impl<'a, RT: AsRef<RangeTable>, F: FnMut(&[Hand], &[Score])> EquityCalculator<'a, RT, F> {
    fn new(community_cards: Cards, ranges: &'a [RT], f: F) -> Option<Self> {
        if !valid_input(community_cards, ranges) {
            None
        } else {
            Some(Self {
                known_cards: Cards::EMPTY,
                community_cards,
                visited_community_cards: community_cards,
                ranges,
                hands: vec![Hand::UNDEFINED; ranges.len()],
                hand_ranking_scores: vec![Score::ZERO; ranges.len()],
                total: 0,
                f,
            })
        }
    }

    fn enumerate(mut self) -> Option<u64> {
        let upper_bound = total_combos_upper_bound(self.community_cards, self.ranges);
        let upper_bound = u64::try_from(upper_bound).ok()?;
        try_u64_to_f64(upper_bound)?;
        let remaining_community_cards = 5 - self.community_cards.count();
        self.community_cards(remaining_community_cards.into());
        assert!(self.total <= upper_bound);
        if self.total != 0 {
            Some(self.total)
        } else {
            None
        }
    }

    fn community_cards(&mut self, remainder: usize) {
        if remainder == 0 {
            self.known_cards = self.community_cards;
            self.players(self.ranges.len() - 1);
            return;
        }

        let current_community_cards = self.community_cards;
        let mut current_visited_community_cards = self.visited_community_cards;
        while let Some(card) = (!current_visited_community_cards).first() {
            self.community_cards = current_community_cards.with(card);
            current_visited_community_cards.add(card);
            self.visited_community_cards = current_visited_community_cards;
            self.community_cards(remainder - 1);
        }
    }

    fn players(&mut self, remainder: usize) {
        let player_index = self.ranges.len() - remainder - 1;
        let current_known_cards = self.known_cards;
        for hand in self.ranges[player_index].as_ref().into_iter() {
            if current_known_cards.has(hand.high()) || current_known_cards.has(hand.low()) {
                continue;
            }

            self.hands[player_index] = hand;
            self.hand_ranking_scores[player_index] = self
                .community_cards
                .with(hand.high())
                .with(hand.low())
                .score_fast();
            self.known_cards = current_known_cards.with(hand.high()).with(hand.low());

            if remainder != 0 {
                self.players(remainder - 1);
            } else {
                self.showdown();
            }
        }
    }

    fn showdown(&mut self) {
        self.total += 1;
        (self.f)(&self.hands, &self.hand_ranking_scores);
    }
}

fn showdown(hand_ranking_scores: &[Score], wins: &mut [u64], ties: &mut [f64]) {
    let max_score = hand_ranking_scores.iter().copied().max().unwrap();
    let winners = hand_ranking_scores
        .iter()
        .copied()
        .filter(|score| *score == max_score)
        .count();
    if winners == 1 {
        let winner_index = hand_ranking_scores
            .iter()
            .position(|score| *score == max_score)
            .unwrap();
        wins[winner_index] += 1;
    } else {
        let ratio = 1.0 / try_u64_to_f64(u64::try_from(winners).unwrap()).unwrap();
        for (index, score) in hand_ranking_scores.iter().copied().enumerate() {
            if score == max_score {
                ties[index] += ratio;
            }
        }
    }
}

fn showdown_simulate(hand_ranking_scores: &[Score], wins: &mut [f64], ties: &mut [f64], diff: f64) {
    let max_score = hand_ranking_scores.iter().copied().max().unwrap();
    let winners = hand_ranking_scores
        .iter()
        .copied()
        .filter(|score| *score == max_score)
        .count();
    if winners == 1 {
        let winner_index = hand_ranking_scores
            .iter()
            .position(|score| *score == max_score)
            .unwrap();
        wins[winner_index] += diff;
    } else {
        let ratio = 1.0 / try_u64_to_f64(u64::try_from(winners).unwrap()).unwrap();
        for (index, score) in hand_ranking_scores.iter().copied().enumerate() {
            if score == max_score {
                ties[index] += ratio * diff;
            }
        }
    }
}

fn showdown_table(
    hands: &[Hand],
    scores: &[Score],
    totals: &mut [RangeTableWith<f64>],
    wins: &mut [RangeTableWith<f64>],
    ties: &mut [RangeTableWith<f64>],
    diff: f64,
) {
    for (i, hand) in hands.iter().copied().enumerate() {
        totals[i][hand] += diff;
    }

    let max_score = scores.iter().copied().max().unwrap();
    let winners = scores
        .iter()
        .copied()
        .filter(|score| *score == max_score)
        .count();
    if winners == 1 {
        let winner_index = scores.iter().position(|score| *score == max_score).unwrap();
        wins[winner_index][hands[winner_index]] += diff;
    } else {
        let ratio = 1.0 / try_u64_to_f64(u64::try_from(winners).unwrap()).unwrap();
        for (index, score) in scores.iter().copied().enumerate() {
            if score == max_score {
                ties[index][hands[index]] += ratio * diff;
            }
        }
    }
}
