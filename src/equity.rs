use core::fmt;
use std::cmp::min;

use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};

use crate::{
    card::Card,
    cards::{Cards, Score},
    hand::Hand,
    range::RangeTable,
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

fn valid_input(
    community_cards: Cards,
    hero_cards: Cards,
    villain_ranges: &[impl AsRef<RangeTable>],
) -> bool {
    valid_input_without_ranges(community_cards, hero_cards, villain_ranges.len())
        && villain_ranges
            .iter()
            .all(|range| !range.as_ref().is_empty())
}

fn valid_input_without_ranges(
    community_cards: Cards,
    hero_cards: Cards,
    villain_count: usize,
) -> bool {
    let known_cards = community_cards | hero_cards;
    hero_cards.count() == 2
        && community_cards.count() <= 5
        && known_cards.count() == community_cards.count() + hero_cards.count()
        && villain_count >= 1
        && villain_count <= 8
}

pub fn total_combos_upper_bound(
    community_cards: Cards,
    villain_ranges: &[impl AsRef<RangeTable>],
) -> u128 {
    // TODO: Refactor this.

    assert!(villain_ranges.len() <= 8);
    assert!(villain_ranges
        .iter()
        .all(|range| !range.as_ref().is_empty()));
    let community_cards_count = community_cards.count();
    assert!(community_cards_count <= 5);
    let mut remaining_cards = {
        let remaining_cards = Card::COUNT - usize::from(community_cards_count) - 2;
        u128::try_from(remaining_cards).unwrap()
    };
    let mut count = 1u128;

    for _ in community_cards_count..5 {
        count *= remaining_cards;
        remaining_cards -= 1;
    }

    let mut max_count = count;
    for _ in 0..villain_ranges.len() * 2 {
        max_count *= remaining_cards;
        remaining_cards -= 1;
    }

    for range in villain_ranges {
        let next_count = count.checked_mul(u128::from(range.as_ref().count_hands()));
        match next_count {
            Some(n) => count = n,
            None => return u128::MAX,
        };
    }

    min(count, max_count)
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
        hero_hand: Hand,
        villain_ranges: &[impl AsRef<RangeTable>],
    ) -> Option<Vec<Equity>> {
        EquityCalculator::new(community_cards, hero_hand.to_cards(), villain_ranges)?.enumerate()
    }

    pub fn simulate(
        start_community_cards: Cards,
        hero_hand: Hand,
        villain_count: usize,
        rounds: u64,
    ) -> Option<Vec<Equity>> {
        let hero_cards = hero_hand.to_cards();
        if !valid_input_without_ranges(start_community_cards, hero_cards, villain_count) {
            return None;
        }
        if rounds == 0 {
            return None;
        }

        let mut rng = SmallRng::from_entropy();
        let remaining_community_cards = 5 - start_community_cards.count();
        let player_count = villain_count + 1;

        let mut scores = vec![Score::ZERO; player_count];
        let mut wins = vec![0u64; player_count];
        let mut ties = vec![0.0; player_count];
        let mut deck = Deck::from_cards(&mut rng, start_community_cards | hero_cards);

        for _ in 0..rounds {
            deck.reset();

            let community_cards = {
                let mut community_cards = start_community_cards;
                for _ in 0..remaining_community_cards {
                    community_cards.add(deck.draw(&mut rng).unwrap());
                }
                community_cards
            };

            scores[0] = (community_cards | hero_cards).score_fast();
            for i in 1..player_count {
                let hand = deck.hand(&mut rng).unwrap();
                let player_cards = community_cards.with(hand.high()).with(hand.low());
                scores[i] = player_cards.score_fast();
            }

            showdown(&scores, &mut wins, &mut ties);
        }

        Some(Self::from_total_wins_ties(rounds, &wins, &ties))
    }

    pub fn simulate_ranges(
        start_community_cards: Cards,
        hero_hand: Hand,
        villain_ranges: &[impl AsRef<RangeTable>],
        rounds: u64,
    ) -> Option<Vec<Equity>> {
        let hero_cards = hero_hand.to_cards();
        if !valid_input(start_community_cards, hero_cards, villain_ranges) {
            return None;
        }
        if rounds == 0 {
            return None;
        }

        let mut rng = SmallRng::from_entropy();
        let remaining_community_cards = 5 - start_community_cards.count();
        let player_count = villain_ranges.len() + 1;
        let villain_full_ranges_original: Vec<_> =
            villain_ranges.iter().map(|r| r.as_ref().to_vec()).collect();
        let mut villain_full_ranges = villain_full_ranges_original.clone();

        let mut scores = vec![Score::ZERO; player_count];
        let mut wins = vec![0.0; player_count];
        let mut ties = vec![0.0; player_count];
        let mut deck = Deck::from_cards(&mut rng, start_community_cards | hero_cards);
        let mut total = 0.0;

        let community_card_factor: u64 = {
            let x = u64::try_from(Card::COUNT).unwrap()
                - u64::from((start_community_cards | hero_cards).count());
            ((x - u64::from(remaining_community_cards))..x).product()
        };
        let upper_bound = {
            let upper_bound = total_combos_upper_bound(start_community_cards, villain_ranges);
            let upper_bound = u64::try_from(upper_bound).ok()?;
            try_u64_to_f64(upper_bound)?
        };

        'outer: for _ in 0..rounds {
            deck.reset();

            let community_cards = {
                let mut community_cards = start_community_cards;
                for _ in 0..remaining_community_cards {
                    community_cards.add(deck.draw(&mut rng).unwrap());
                }
                community_cards
            };

            let mut seen_cards = community_cards | hero_cards;
            scores[0] = seen_cards.score_fast();
            let mut factor = community_card_factor;
            for (i, range) in villain_full_ranges.iter_mut().enumerate() {
                range.truncate(0);
                range.extend(
                    villain_full_ranges_original[i]
                        .iter()
                        .copied()
                        .filter(|hand| !seen_cards.has(hand.high()) && !seen_cards.has(hand.low())),
                );
                factor *= u64::try_from(range.len()).unwrap();
                let Some(hand) = range.choose(&mut rng).copied() else {
                    continue 'outer;
                };
                scores[i + 1] = community_cards
                    .with(hand.high())
                    .with(hand.low())
                    .score_fast();
                seen_cards.add(hand.high());
                seen_cards.add(hand.low());
            }

            let diff = try_u64_to_f64(factor).unwrap() / upper_bound;
            showdown_simulate(&scores, &mut wins, &mut ties, diff);
            total += diff;
        }

        if total == 0.0 {
            None
        } else {
            Some(Self::from_total_wins_ties_simulate(total, &wins, &ties))
        }
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

struct EquityCalculator<'a, RT: AsRef<RangeTable>> {
    known_cards: Cards,
    hero_cards: Cards,
    visited_community_cards: Cards,
    community_cards: Cards,
    villain_ranges: &'a [RT],
    hand_ranking_scores: Vec<Score>,
    total: u64,
    wins: Vec<u64>,
    ties: Vec<f64>,
}

impl<'a, RT: AsRef<RangeTable>> EquityCalculator<'a, RT> {
    fn new(community_cards: Cards, hero_cards: Cards, villain_ranges: &'a [RT]) -> Option<Self> {
        if !valid_input(community_cards, hero_cards, villain_ranges) {
            None
        } else {
            Some(Self {
                known_cards: Cards::EMPTY,
                hero_cards,
                community_cards,
                visited_community_cards: community_cards | hero_cards,
                villain_ranges,
                hand_ranking_scores: vec![Score::ZERO; villain_ranges.len() + 1],
                total: 0,
                wins: vec![0; villain_ranges.len() + 1],
                ties: vec![0.0; villain_ranges.len() + 1],
            })
        }
    }

    fn enumerate(mut self) -> Option<Vec<Equity>> {
        let upper_bound = total_combos_upper_bound(self.community_cards, self.villain_ranges);
        if u64::try_from(upper_bound).is_err() {
            return None;
        }
        let remaining_community_cards = 5 - self.community_cards.count();
        self.community_cards(remaining_community_cards.into());
        if self.total != 0 {
            Some(Equity::from_total_wins_ties(
                self.total, &self.wins, &self.ties,
            ))
        } else {
            None
        }
    }

    fn community_cards(&mut self, remainder: usize) {
        if remainder == 0 {
            let known_cards = self.hero_cards | self.community_cards;
            self.hand_ranking_scores[0] = known_cards.top5().to_score();
            self.known_cards = known_cards;
            self.players(self.villain_ranges.len() - 1);
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
        let player_index = self.villain_ranges.len() - remainder - 1;
        let villain = self.villain_ranges[player_index].as_ref();
        let current_known_cards = self.known_cards;
        villain.for_each_hand(|hand| {
            if current_known_cards.has(hand.high()) || current_known_cards.has(hand.low()) {
                return;
            }

            self.hand_ranking_scores[player_index + 1] = self
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
        });
    }

    fn showdown(&mut self) {
        self.total += 1;
        showdown(&self.hand_ranking_scores, &mut self.wins, &mut self.ties)
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

pub struct Deck {
    cards: [Card; Card::COUNT],
    max_len: usize,
    len: usize,
}

impl Deck {
    pub fn from_cards(rng: &mut impl Rng, known_cards: Cards) -> Self {
        let mut cards = [Card::MIN; Card::COUNT];
        let mut index = 0;
        for card in Card::all() {
            if known_cards.has(card) {
                continue;
            }
            cards[index] = card;
            index += 1;
        }
        cards[..index].shuffle(rng);
        Deck {
            cards,
            max_len: index,
            len: index,
        }
    }

    pub fn draw(&mut self, rng: &mut impl Rng) -> Option<Card> {
        if self.len == 0 {
            None
        } else {
            let index = rng.gen_range(0..self.len);
            let card = self.cards[index];
            self.cards.swap(index, self.len - 1);
            self.len -= 1;
            Some(card)
        }
    }

    pub fn hand(&mut self, rng: &mut impl Rng) -> Option<Hand> {
        let a = self.draw(rng)?;
        let b = self.draw(rng)?;
        Some(Hand::of_two_cards(a, b))
    }

    pub fn reset(&mut self) {
        self.len = self.max_len;
    }
}
