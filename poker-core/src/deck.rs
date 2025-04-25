use rand::{seq::SliceRandom, Rng};

use crate::{card::Card, cards::Cards, hand::Hand, result::Result};

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

    pub fn draw_result(&mut self, rng: &mut impl Rng) -> Result<Card> {
        self.draw(rng).ok_or_else(|| "no more cards in deck".into())
    }

    pub fn hand(&mut self, rng: &mut impl Rng) -> Option<Hand> {
        let a = self.draw(rng)?;
        let b = self.draw(rng)?;
        Hand::of_two_cards(a, b)
    }

    pub fn reset(&mut self) {
        self.len = self.max_len;
    }
}
