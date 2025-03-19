use std::cmp::{max, min};
use std::usize;

use rand::Rng;

use crate::card::Card;
use crate::cards::{Cards, Score};
use crate::deck::Deck;
use crate::hand::Hand;
use crate::result::Result;

#[derive(Debug, Clone, Copy)]
enum Action {
    Fold(u8),
    Check(u8),
    Call(u8, u32),
    /// Bet is also used for raise, all-in and blinds.
    Bet(u8, u32),
    Flop([Card; 3]),
    Turn(Card),
    River(Card),
}

impl Action {
    fn is_street(self) -> bool {
        matches!(self, Action::Flop(_) | Action::Turn(_) | Action::River(_))
    }

    fn is_player(self) -> bool {
        matches!(
            self,
            Action::Fold(_) | Action::Check(_) | Action::Call(_, _) | Action::Bet(_, _)
        )
    }

    fn to_bet(self) -> Option<u32> {
        match self {
            Action::Bet(_, amount) => Some(amount),
            _ => None,
        }
    }

    fn to_amount(self) -> Option<u32> {
        match self {
            Action::Call(_, amount) => Some(amount),
            Action::Bet(_, amount) => Some(amount),
            _ => None,
        }
    }

    fn player(self) -> Option<usize> {
        let player = match self {
            Action::Fold(player) => player,
            Action::Check(player) => player,
            Action::Call(player, _) => player,
            Action::Bet(player, _) => player,
            _ => return None,
        };
        Some(usize::from(player))
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Street {
    PreFlop,
    Flop,
    Turn,
    River,
}

impl Street {
    fn next(self) -> Option<Self> {
        match self {
            Street::PreFlop => Some(Street::Flop),
            Street::Flop => Some(Street::Turn),
            Street::Turn => Some(Street::River),
            Street::River => None,
        }
    }
}

#[derive(Debug, Clone, Copy)] // TODO: Custom Debug
pub struct Board {
    cards: [Card; 5],
    street: Street,
}

impl Board {
    const EMPTY: Self = Self {
        cards: [Card::MIN; 5],
        street: Street::PreFlop,
    };

    pub fn cards(&self) -> &[Card] {
        match self.street {
            Street::PreFlop => &self.cards[..0],
            Street::Flop => &self.cards[..3],
            Street::Turn => &self.cards[..4],
            Street::River => &self.cards[..5],
        }
    }

    pub fn street(&self) -> Street {
        self.street
    }
}

#[derive(Debug, Clone, Copy)] // TODO: Custom Debug
struct Bitset<const SIZE: usize>([u8; SIZE]);

impl<const SIZE: usize> Bitset<SIZE> {
    const EMPTY: Self = Self([0; SIZE]);

    fn ones(n: usize) -> Self {
        let mut s = Self::EMPTY;
        for i in 0..n {
            s.set(i);
        }
        s
    }

    fn set(&mut self, index: usize) {
        self.0[index / 8] |= 1 << (index % 8);
    }

    fn remove(&mut self, index: usize) {
        self.0[index / 8] &= !(1 << (index % 8));
    }

    fn has(&self, index: usize) -> bool {
        let i = index / 8;
        if i >= self.0.len() {
            false
        } else {
            (self.0[i] & (1 << (index % 8))) != 0
        }
    }

    fn iter(&self, max_exclusive: usize) -> impl Iterator<Item = usize> + '_ {
        (0..max_exclusive)
            .into_iter()
            .filter(|index| self.has(*index))
    }

    fn count(&self) -> u32 {
        self.0.iter().map(|n| n.count_ones()).sum::<u32>()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    StartOfHand,
    Player(usize),
    WaitingForStreet(Street),
    WonWithoutShowdown(usize),
    Showdown,
    End,
}

#[derive(Debug, Clone)]
pub struct Game {
    actions: Vec<Action>,
    board: Board,
    starting_stacks: Vec<u32>,
    current_stacks: Vec<u32>,
    button_index: u8,
    current_street_index: usize,
    current_action_index: usize,
    /// Set to u8::MAX if no current player is set.
    current_player: u8,
    players_in_hand: Bitset<2>,
    small_blind: u32,
    big_blind: u32,
    hands: Vec<Hand>,
    hand_known: Bitset<2>,
}

impl Game {
    pub const MIN_PLAYERS: usize = 2;
    pub const MAX_PLAYERS: usize = 9;

    pub fn new(
        starting_stacks: impl Iterator<Item = u32>,
        button_index: usize,
        small_blind: u32,
        big_blind: u32,
    ) -> Result<Self> {
        let stacks: Vec<_> = starting_stacks.collect();
        let player_count = stacks.len();
        if player_count < Self::MIN_PLAYERS || player_count > Self::MAX_PLAYERS {
            return Err(format!(
                "not enough or too many players ({} - {})",
                Self::MIN_PLAYERS,
                Self::MAX_PLAYERS
            )
            .into());
        }
        if stacks.iter().any(|stack| *stack == 0) {
            return Err("empty stacks not allowed in hand".into());
        }
        if button_index >= player_count {
            return Err("invalid button position".into());
        }
        let total_stacks = stacks
            .iter()
            .copied()
            .fold(Some(0u32), |acc, n| acc.and_then(|acc| acc.checked_add(n)));
        if total_stacks.is_none() {
            return Err("total stacks overflows".into());
        }
        Ok(Self {
            actions: Vec::new(),
            starting_stacks: stacks.clone(),
            current_stacks: stacks,
            board: Board::EMPTY,
            button_index: u8::try_from(button_index).unwrap(),
            current_street_index: 0,
            current_action_index: 0,
            current_player: u8::MAX,
            players_in_hand: Bitset::ones(player_count),
            small_blind,
            big_blind,
            hands: vec![Hand::MIN; player_count],
            hand_known: Bitset::EMPTY,
        })
    }

    pub fn is_heads_up(&self) -> bool {
        self.player_count() == Self::MIN_PLAYERS
    }

    fn small_blind_index(&self) -> usize {
        let button_offset = if self.is_heads_up() { 0 } else { 1 };
        usize::from((self.button_index + button_offset) % self.player_count_u8())
    }

    fn big_blind_index(&self) -> usize {
        let button_offset = if self.is_heads_up() { 1 } else { 2 };
        usize::from((self.button_index + button_offset) % self.player_count_u8())
    }

    pub fn board(&self) -> Board {
        self.board
    }

    pub fn total_pot(&self) -> u32 {
        self.invested_per_player().sum::<u32>()
    }

    pub fn invested_per_player(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.player_count())
            .into_iter()
            .map(|player| self.invested(player))
    }

    fn invested(&self, player: usize) -> u32 {
        assert!(player < self.player_count());
        self.starting_stacks[player] - self.current_stacks[player]
    }

    pub fn invested_in_street(&self, player: usize) -> u32 {
        self.actions_in_street()
            .iter()
            .copied()
            .filter(|action| action.player().is_some_and(|p| p == player))
            .filter_map(|action| action.to_amount())
            .sum::<u32>()
    }

    pub fn has_cards(&self, index: usize) -> bool {
        assert!(index < self.player_count());
        self.players_in_hand.has(index)
    }

    pub fn players_in_hand(&self) -> impl Iterator<Item = usize> + '_ {
        self.players_in_hand.iter(self.player_count())
    }

    pub fn can_act(&self, index: usize) -> bool {
        assert!(index < self.player_count());
        self.players_in_hand.has(index) && self.current_stacks[index] > 0
    }

    fn actions_in_street(&self) -> &[Action] {
        assert!(
            self.current_street_index == 0
                || self.actions[self.current_street_index - 1].is_street()
        );
        for index in self.current_street_index..self.current_action_index {
            if !self.actions[index].is_player() {
                return &self.actions[self.current_street_index..index];
            }
        }
        &self.actions[self.current_street_index..self.current_action_index]
    }

    pub fn can_check(&self) -> bool {
        let Some(player) = self.current_player() else {
            return false;
        };
        if self.board.street == Street::PreFlop && player == self.big_blind_index() {
            self.actions_in_street()[2..]
                .iter()
                .all(|action| matches!(action, Action::Fold(_) | Action::Call(_, _)))
        } else {
            self.actions_in_street()
                .iter()
                .all(|action| matches!(action, Action::Check(_) | Action::Fold(_)))
        }
    }

    pub fn can_call(&self) -> Option<u32> {
        let player = self.current_player()?;
        let can_call = self
            .actions_in_street()
            .iter()
            .any(|action| matches!(action, Action::Bet(_, _)));
        if can_call {
            let amount = self.call_amount(player);
            Some(min(self.current_stacks[player], amount))
        } else {
            None
        }
    }

    fn call_amount(&self, player: usize) -> u32 {
        self.invested_per_player().max().unwrap() - self.invested(player)
    }

    pub fn can_bet(&self) -> Option<u32> {
        let player = self.current_player()?;
        let can_bet = self
            .actions_in_street()
            .iter()
            .all(|action| matches!(action, Action::Check(_) | Action::Fold(_)));
        if can_bet {
            Some(min(self.current_stacks[player], self.big_blind))
        } else {
            None
        }
    }

    pub fn can_raise(&self) -> Option<u32> {
        let player = self.current_player()?;
        let actions = self.actions_in_street();
        let can_raise = actions
            .iter()
            .any(|action| matches!(action, Action::Bet(_, _)));
        if can_raise {
            let mut last_raise = 0;
            let mut diff = 0;
            for action in actions.iter().copied() {
                let Some(bet) = action.to_bet() else {
                    continue;
                };
                assert!(bet > last_raise);
                let new_diff = bet - last_raise;
                if new_diff >= diff {
                    diff = new_diff;
                }
                last_raise = bet;
            }
            if self.board.street == Street::PreFlop {
                diff = max(self.big_blind, diff);
            }

            let raise_amount = (last_raise + diff)
                .checked_sub(self.invested_in_street(player))
                .unwrap();
            let call_amount = self.call_amount(player);
            if call_amount >= self.current_stacks[player] {
                None
            } else {
                Some(min(self.current_stacks[player], raise_amount))
            }
        } else {
            None
        }
    }

    pub fn current_stack(&self) -> Option<u32> {
        self.current_player().map(|i| self.current_stacks[i])
    }

    pub fn current_stacks(&self) -> &[u32] {
        &self.current_stacks
    }

    pub fn button_index(&self) -> usize {
        usize::from(self.button_index)
    }

    pub fn player_count(&self) -> usize {
        self.current_stacks.len()
    }

    fn player_count_u8(&self) -> u8 {
        u8::try_from(self.current_stacks.len()).unwrap()
    }

    pub fn state(&self) -> State {
        if self.at_start() {
            State::StartOfHand
        } else if let Some(player) = self.current_player() {
            State::Player(player)
        } else if let Some(street) = self.can_next_street() {
            State::WaitingForStreet(street)
        } else {
            let is_end = self
                .starting_stacks
                .iter()
                .zip(&self.current_stacks)
                .any(|(start, current)| *current > *start);
            if is_end {
                // TODO: Not correct
                State::End
            } else if self.players_in_hand.count() == 1 {
                let winner = self
                    .players_in_hand
                    .iter(self.player_count())
                    .next()
                    .unwrap();
                State::WonWithoutShowdown(winner)
            } else {
                State::Showdown
            }
        }
    }

    pub fn current_player(&self) -> Option<usize> {
        if self.current_player == u8::MAX {
            None
        } else {
            Some(usize::from(self.current_player))
        }
    }

    fn current_player_result(&self) -> Result<usize> {
        match self.current_player() {
            Some(player) => Ok(player),
            None => Err("can't perform player action: not started or end of betting round".into()),
        }
    }

    fn at_start(&self) -> bool {
        self.current_action_index == 0
    }

    fn check_pre_update(&self) -> Result<()> {
        if self.current_action_index != self.actions.len() {
            return Err("can't apply action: not at final action".into());
        }
        Ok(())
    }

    fn next_player(&mut self) -> Result<()> {
        assert!(self.current_player().is_some());
        if self.players_in_hand.count() == 1 {
            self.current_player = u8::MAX;
            return Ok(());
        }

        let actions = self.actions_in_street();
        let acting_players = self.acting_players();
        let check_count = actions
            .iter()
            .copied()
            .filter(|action| matches!(action, Action::Check(_)))
            .count();
        let all_check_or_fold = actions
            .iter()
            .copied()
            .all(|action| matches!(action, Action::Check(_) | Action::Fold(_)));
        if all_check_or_fold && check_count == acting_players {
            self.current_player = u8::MAX;
            return Ok(());
        }

        let acting_last_bettor_callers = {
            let mut s = Bitset::<2>::EMPTY;
            for action in actions.iter().copied() {
                match action {
                    Action::Fold(_) | Action::Check(_) => (),
                    Action::Call(player, _) => {
                        if self.can_act(usize::from(player)) {
                            s.set(usize::from(player))
                        }
                    }
                    Action::Bet(player, _) => {
                        s = Bitset::EMPTY;
                        if self.can_act(usize::from(player)) {
                            s.set(usize::from(player));
                        }
                    }
                    _ => unreachable!(),
                }
            }
            s
        };
        if usize::try_from(acting_last_bettor_callers.count()).unwrap() == acting_players {
            if self.board.street == Street::PreFlop
                && self.current_player().unwrap() == self.small_blind_index()
            {
                self.current_player = self.big_blind_index() as u8;
                let can_check = self.can_check();
                assert!(can_check);
                return Ok(());
            }
            self.current_player = u8::MAX;
            return Ok(());
        }

        self.next_acting_player()
    }

    fn next_acting_player(&mut self) -> Result<()> {
        self.current_player_result()?;
        let current_player_start = self.current_player;
        loop {
            self.current_player = (self.current_player + 1) % self.player_count_u8();
            if self.current_player == current_player_start {
                self.current_player = u8::MAX;
                return Ok(());
            }
            if self.can_act(usize::from(self.current_player)) {
                return Ok(());
            }
        }
    }

    fn acting_players(&self) -> usize {
        (0..self.player_count())
            .filter(|player| self.can_act(*player))
            .count()
    }

    fn action_bet_max(&mut self, amount: u32) -> Result<()> {
        let player = self.current_player_result()?;
        self.action_bet(min(self.current_stacks[player], amount))
    }

    fn action_bet(&mut self, amount: u32) -> Result<()> {
        self.check_pre_update()?;
        self.update_stack(amount)?;
        self.add_action(Action::Bet(self.current_player, amount));
        self.next_player()
    }

    fn add_action(&mut self, action: Action) {
        assert_eq!(self.current_action_index, self.actions.len());
        self.actions.push(action);
        self.current_action_index += 1;
    }

    fn update_stack(&mut self, amount: u32) -> Result<()> {
        let player = self.current_player_result()?;
        if amount > self.current_stacks[player] {
            return Err("player cannot afford sizing".into());
        }
        self.current_stacks[player] -= amount;
        Ok(())
    }

    pub fn post_small_and_big_blind(&mut self) -> Result<()> {
        self.check_pre_update()?;
        if !self.at_start() {
            return Err("can only post small and big blind before other actions".into());
        }
        self.current_player = self.button_index;
        if !self.is_heads_up() {
            self.current_player = (self.current_player + 1) % self.player_count_u8();
        }
        self.action_bet_max(self.small_blind)?;
        self.action_bet_max(self.big_blind)?;
        Ok(())
    }

    pub fn fold(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        assert!(self.players_in_hand.has(player));
        self.players_in_hand.remove(player);
        self.add_action(Action::Fold(self.current_player));
        self.next_player()
    }

    pub fn check(&mut self) -> Result<()> {
        self.check_pre_update()?;
        self.current_player_result()?;
        if !self.can_check() {
            return Err("player is not allowed to check".into());
        }
        self.add_action(Action::Check(self.current_player));
        self.next_player()
    }

    pub fn call(&mut self) -> Result<()> {
        self.check_pre_update()?;
        self.current_player_result()?;
        let Some(amount) = self.can_call() else {
            return Err("player is not allowed to call".into());
        };
        self.update_stack(amount)?;
        self.add_action(Action::Call(self.current_player, amount));
        self.next_player()
    }

    pub fn bet(&mut self, amount: u32) -> Result<()> {
        self.check_pre_update()?;
        self.current_player_result()?;
        let Some(min_amount) = self.can_bet() else {
            return Err("player is not allowed to bet".into());
        };
        if amount < min_amount {
            return Err("bet is smaller than the minimum".into());
        }
        self.update_stack(amount)?;
        self.add_action(Action::Bet(self.current_player, amount));
        self.next_player()
    }

    pub fn raise(&mut self, amount: u32) -> Result<()> {
        self.check_pre_update()?;
        self.current_player_result()?;
        let Some(min_amount) = self.can_raise() else {
            return Err("player is not allowed to raise".into());
        };
        if amount < min_amount {
            dbg!((amount, min_amount));
            return Err("raise is smaller than the minimum".into());
        }
        self.update_stack(amount)?;
        self.add_action(Action::Bet(self.current_player, amount));
        self.next_player()
    }

    pub fn all_in(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        assert!(self.can_bet().or_else(|| self.can_raise()).is_some());
        let amount = self.current_stacks[player];
        self.update_stack(amount)?;
        self.add_action(Action::Bet(self.current_player, amount));
        self.next_player()
    }

    pub fn uncalled_bet(&mut self, player: usize, amount: u32) -> Result<()> {
        // TODO: Workaround
        self.check_pre_update()?;
        if self.state() != State::WonWithoutShowdown(player) {
            return Err("uncalled bet: not allowed".into());
        }
        // TODO: Check amount valid
        let Some(new_stack) = self.current_stacks[player].checked_add(amount) else {
            return Err("uncalled bet: invalid amount".into());
        };
        self.current_stacks[player] = new_stack;
        Ok(())
    }

    fn can_next_street(&self) -> Option<Street> {
        let allowed = self.current_player().is_none()
            && (!self.actions_in_street().is_empty() || self.acting_players() == 0)
            && self.players_in_hand.count() > 1
            && self.board.street != Street::River;
        if allowed {
            Some(self.board.street.next().unwrap())
        } else {
            None
        }
    }

    fn check_new_street(&self, street: Street) -> Result<()> {
        if self.can_next_street() != Some(street) {
            Err("cannot go to next street".into())
        } else {
            Ok(())
        }
    }

    fn check_cards(&mut self) -> Result<()> {
        let mut known_cards = Cards::EMPTY;
        for index in self.hand_known.iter(self.player_count()) {
            let hand = self.hands[index];
            if known_cards.has(hand.high()) {
                return Err(format!("duplicate card {} in hand", hand.high()).into());
            }
            if known_cards.has(hand.low()) {
                return Err(format!("duplicate card {} in hand", hand.low()).into());
            }
            known_cards.add(hand.high());
            known_cards.add(hand.low());
        }
        for card in self.board.cards().iter().copied() {
            if known_cards.has(card) {
                return Err(format!("duplicate card {} on board", card).into());
            }
            known_cards.add(card);
        }
        Ok(())
    }

    fn next_street_final(&mut self) -> Result<()> {
        self.check_cards()?;
        self.current_player = self.button_index;
        self.next_acting_player()?;
        self.current_street_index = self.actions.len();
        Ok(())
    }

    pub fn flop(&mut self, flop: [Card; 3]) -> Result<()> {
        self.check_pre_update()?;
        self.check_new_street(Street::Flop)?;
        self.add_action(Action::Flop(flop));
        self.board.street = Street::Flop;
        self.board.cards[..3].copy_from_slice(flop.as_slice());
        self.next_street_final()?;
        Ok(())
    }

    pub fn turn(&mut self, turn: Card) -> Result<()> {
        self.check_pre_update()?;
        self.check_new_street(Street::Turn)?;
        self.add_action(Action::Turn(turn));
        self.board.street = Street::Turn;
        self.board.cards[3] = turn;
        self.next_street_final()?;
        Ok(())
    }

    pub fn river(&mut self, river: Card) -> Result<()> {
        self.check_pre_update()?;
        self.check_new_street(Street::River)?;
        self.add_action(Action::River(river));
        self.board.street = Street::River;
        self.board.cards[4] = river;
        self.next_street_final()?;
        Ok(())
    }

    pub fn draw_next_street_from(&mut self, deck: &mut Deck, rng: &mut impl Rng) -> Result<()> {
        self.check_pre_update()?;
        let Some(street) = self.can_next_street() else {
            return Err("cannot go to next street".into());
        };
        match street {
            Street::PreFlop => unreachable!(),
            Street::Flop => self.flop([
                deck.draw_result(rng)?,
                deck.draw_result(rng)?,
                deck.draw_result(rng)?,
            ]),
            Street::Turn => self.turn(deck.draw_result(rng)?),
            Street::River => self.river(deck.draw_result(rng)?),
        }
    }

    pub fn showdown(&mut self, total_rake: u32) -> Result<()> {
        self.check_pre_update()?;
        match self.state() {
            State::WonWithoutShowdown(player) => {
                let pot = self.total_pot();
                let Some(pot) = pot.checked_sub(total_rake) else {
                    return Err("showdown: rake too high".into());
                };
                self.current_stacks[player] += pot;
                Ok(())
            }
            State::Showdown => self.showdown_hands(total_rake),
            _ => Err("showdown not possible in this state".into()),
        }
    }

    fn showdown_hands(&mut self, total_rake: u32) -> Result<()> {
        // TODO:
        // - Different investments per player at showdown
        // - Multiple winners
        // - Rake
        // - Exactly calculate pot split
        // - Manually apply won amounts
        // - Showdown order

        let mut scores = [Score::ZERO; Self::MAX_PLAYERS];
        let board = Cards::from_slice(self.board.cards()).unwrap();
        for player in self.players_in_hand.iter(self.player_count()) {
            let Some(hand) = self.get_hand(player) else {
                return Err(format!("showdown: missing hand for player {player}").into());
            };
            scores[player] = board.with(hand.high()).with(hand.low()).score_fast();
        }
        let max_score = scores.iter().copied().max().unwrap();
        let winners = scores
            .iter()
            .copied()
            .filter(|score| *score == max_score)
            .count();
        assert_eq!(winners, 1);
        let winner_index = scores.iter().position(|score| *score == max_score).unwrap();
        let pot = self
            .invested_per_player()
            .sum::<u32>()
            .checked_sub(total_rake)
            .unwrap(); // TODO
        self.current_stacks[winner_index] += pot;
        Ok(())
    }

    pub fn set_hand(&mut self, index: usize, hand: Hand) -> Result<()> {
        if index >= self.player_count() {
            return Err(format!("unknown player index {index}").into());
        }
        self.hands[index] = hand;
        self.hand_known.set(index);
        self.check_cards()
    }

    pub fn get_hand(&self, index: usize) -> Option<Hand> {
        if index >= self.player_count() || !self.hand_known.has(index) {
            None
        } else {
            Some(self.hands[index])
        }
    }
}
