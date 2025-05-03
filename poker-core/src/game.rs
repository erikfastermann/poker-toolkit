use std::cmp::min;
use std::collections::HashSet;
use std::{array, fmt, usize};

use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::bitset::Bitset;
use crate::card::Card;
use crate::cards::{Cards, Score};
use crate::deck::Deck;
use crate::hand::Hand;
use crate::result::Result;

// TODO:
// - Bet/raise steps.
// - Mucking
// - Poison game on error or ensure every error is recoverable
// - Type alias or wrapper for player and amount, use u8 for player everywhere
// - Nicer handling of state method with multiple runouts

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Post {
        player: u8,
        amount: u32,
    },
    Straddle {
        player: u8,
        amount: u32,
    },
    Fold(u8),
    Check(u8),
    Call {
        player: u8,
        amount: u32,
    },
    Bet {
        player: u8,
        amount: u32,
    },
    Raise {
        player: u8,
        old_stack: u32,
        amount: u32,
        to: u32,
    },
    Flop([Card; 3]),
    Turn(Card),
    River(Card),
    UncalledBet {
        player: u8,
        amount: u32,
    },
    Shows {
        player: u8,
        hand: Hand,
    },
}

impl Action {
    fn is_street(self) -> bool {
        self.street().is_some()
    }

    fn street(self) -> Option<Street> {
        match self {
            Action::Flop(_) => Some(Street::Flop),
            Action::Turn(_) => Some(Street::Turn),
            Action::River(_) => Some(Street::River),
            _ => None,
        }
    }

    fn is_player(self) -> bool {
        self.player().is_some()
    }

    fn player(self) -> Option<usize> {
        let player = match self {
            Action::Post { player, .. } => player,
            Action::Straddle { player, .. } => player,
            Action::Fold(player) => player,
            Action::Check(player) => player,
            Action::Call { player, .. } => player,
            Action::Bet { player, .. } => player,
            Action::Raise { player, .. } => player,
            _ => return None,
        };
        Some(usize::from(player))
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Street {
    PreFlop = 0,
    Flop = 1,
    Turn = 2,
    River = 3,
}

impl Street {
    pub const COUNT: usize = 4;

    pub const STREETS: [Street; Self::COUNT] = [Self::PreFlop, Self::Flop, Self::Turn, Self::River];

    pub fn previous(self) -> Option<Self> {
        match self {
            Street::PreFlop => None,
            Street::Flop => Some(Street::PreFlop),
            Street::Turn => Some(Street::Flop),
            Street::River => Some(Street::Turn),
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            Street::PreFlop => Some(Street::Flop),
            Street::Flop => Some(Street::Turn),
            Street::Turn => Some(Street::River),
            Street::River => None,
        }
    }

    pub fn to_usize(self) -> usize {
        self as usize
    }

    pub fn community_card_count(self) -> usize {
        match self {
            Street::PreFlop => 0,
            Street::Flop => 3,
            Street::Turn => 4,
            Street::River => 5,
        }
    }
}

impl fmt::Display for Street {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)] // TODO: Custom Debug
pub struct Board {
    cards: [Card; 5],
    street: Street,
}

impl Board {
    const EMPTY: Self = Self {
        cards: [Card::MIN; 5],
        street: Street::PreFlop,
    };

    pub fn from_cards(given_cards: &[Card]) -> Result<Self> {
        let street = match given_cards.len() {
            0 => Street::PreFlop,
            3 => Street::Flop,
            4 => Street::Turn,
            5 => Street::River,
            _ => return Err("board: bad cards length".into()),
        };

        if Cards::from_slice(given_cards).is_none() {
            return Err("board: duplicate card".into());
        }

        let mut cards = [Card::MIN; 5];
        (&mut cards[..given_cards.len()]).copy_from_slice(&given_cards);

        Ok(Self { cards, street })
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards[..self.street.community_card_count()]
    }

    pub fn street(&self) -> Street {
        self.street
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Post,
    Player(usize),
    Street(Street),
    UncalledBet { player: usize, amount: u32 },
    ShowOrMuck(usize),
    Showdown,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    actions: Vec<Action>,
    boards: [Board; Self::MAX_RUNOUTS],
    current_board: u8,
    names: Vec<String>,
    seats: Vec<u8>,
    starting_stacks: Vec<u32>,
    stacks_in_street: [Vec<u32>; Street::COUNT],
    showdown_stacks: Vec<u32>,
    button_index: u8,
    current_street_index: usize,
    current_action_index: usize,
    /// Set to u8::MAX if no current player is set.
    current_player: u8,
    players_in_hand: Bitset<2>,
    small_blind: u32,
    big_blind: u32,
    /// Using Hand::UNDEFINED if a hand is not known.
    hands: Vec<Hand>,
    hand_shown: Bitset<2>,
    at_end: bool,
    in_next: bool,
}

impl Game {
    const MAX_RUNOUTS: usize = 4;

    pub const MIN_PLAYERS: usize = 2;
    pub const MAX_PLAYERS: usize = 9;

    pub const TOTAL_CARDS: usize = 5;

    const POSITION_NAMES: [&[(&str, &str)]; Self::MAX_PLAYERS - Self::MIN_PLAYERS + 1] = [
        &[("BTN", "Small Blind / Dealer"), ("BB", "Big Blind")],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
        ],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
        ],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
            ("CO", "Cutoff"),
        ],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
            ("HJ", "Hijack"),
            ("CO", "Cutoff"),
        ],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
            ("LJ", "Lowjack"),
            ("HJ", "Hijack"),
            ("CO", "Cutoff"),
        ],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
            ("UTG+1", "Under the Gun +1"),
            ("LJ", "Lowjack"),
            ("HJ", "Hijack"),
            ("CO", "Cutoff"),
        ],
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
            ("UTG+1", "Under the Gun +1"),
            ("UTG+2", "Under the Gun +2"),
            ("LJ", "Lowjack"),
            ("HJ", "Hijack"),
            ("CO", "Cutoff"),
        ],
    ];

    pub fn position_names(player_count: usize) -> Option<&'static [(&'static str, &'static str)]> {
        if player_count < Self::MIN_PLAYERS || player_count > Self::MAX_PLAYERS {
            None
        } else {
            Some(Self::POSITION_NAMES[player_count - Self::MIN_PLAYERS])
        }
    }

    pub fn position_name(
        player_count: usize,
        button_index: usize,
        player: usize,
    ) -> Option<(&'static str, &'static str)> {
        let names = Self::position_names(player_count)?;
        if button_index >= player_count || player >= player_count {
            None
        } else {
            let index = (player_count - button_index + player) % player_count;
            Some(names[index])
        }
    }

    pub fn new(
        players: &[Player],
        button_index: usize,
        small_blind: u32,
        big_blind: u32,
    ) -> Result<Self> {
        let player_count = players.len();
        if player_count < Self::MIN_PLAYERS || player_count > Self::MAX_PLAYERS {
            return Err(format!(
                "not enough or too many players ({} - {})",
                Self::MIN_PLAYERS,
                Self::MAX_PLAYERS
            )
            .into());
        }
        if button_index >= player_count {
            return Err("invalid button position".into());
        }

        let stacks: Vec<_> = players.iter().map(|player| player.starting_stack).collect();
        if stacks.iter().any(|stack| *stack == 0) {
            return Err("empty stacks not allowed in hand".into());
        }
        let total_stacks = stacks
            .iter()
            .copied()
            .fold(Some(0u32), |acc, n| acc.and_then(|acc| acc.checked_add(n)));
        if total_stacks.is_none() {
            return Err("total stacks overflows".into());
        }

        let names: Vec<_> = players
            .iter()
            .enumerate()
            .map(|(index, player)| {
                player.name.clone().unwrap_or_else(|| {
                    Self::position_name(player_count, button_index, index)
                        .unwrap()
                        .0
                        .to_string()
                })
            })
            .collect();
        if names.iter().cloned().collect::<HashSet<String>>().len() != player_count {
            return Err("duplicate player name".into());
        }
        if names.iter().any(|name| name.is_empty()) {
            return Err("empty player name".into());
        }

        let seats = {
            let seats: Vec<_> = players.iter().filter_map(|player| player.seat).collect();
            if seats.len() == 0 {
                (0..u8::try_from(player_count).unwrap()).collect()
            } else if seats.len() != player_count {
                return Err("all players need a seat config (or none)".into());
            } else if seats
                .iter()
                .any(|seat| usize::from(*seat) >= Self::MAX_PLAYERS)
            {
                return Err(
                    "all player seat configs must be smaller than the max amount of players".into(),
                );
            } else if seats.iter().collect::<HashSet<_>>().len() != player_count {
                return Err("duplicate player seat config".into());
            } else {
                seats
            }
        };

        let hands: Vec<_> = players
            .iter()
            .map(|player| player.hand.unwrap_or(Hand::UNDEFINED))
            .collect();

        let game = Self {
            actions: Vec::new(),
            names,
            seats,
            starting_stacks: stacks.clone(),
            stacks_in_street: array::from_fn(|_| stacks.clone()),
            showdown_stacks: vec![0; player_count],
            boards: [Board::EMPTY; Self::MAX_RUNOUTS],
            current_board: 0,
            button_index: u8::try_from(button_index).unwrap(),
            current_street_index: 0,
            current_action_index: 0,
            current_player: u8::MAX,
            players_in_hand: Bitset::ones(player_count),
            small_blind,
            big_blind,
            hands,
            hand_shown: Bitset::EMPTY,
            at_end: false,
            in_next: false,
        };
        game.check_cards()?;
        Ok(game)
    }

    pub fn from_game_data(data: &GameData) -> Result<Game> {
        let mut game = Self::new(
            &data.players,
            usize::from(data.button_index),
            data.small_blind,
            data.big_blind,
        )?;

        if !data.actions.is_empty() {
            game.post_small_and_big_blind()?;

            let mut current_action_index = 2;
            while current_action_index < data.actions.len() {
                let Action::Post { player, .. } = data.actions[current_action_index] else {
                    break;
                };
                game.additional_post(usize::from(player))?;
                current_action_index += 1;
            }

            for action in data.actions[current_action_index..].iter().copied() {
                game.apply_action(action)?;
            }
        }

        if &game.actions != &data.actions {
            return Err("from game data: actions don't match".into());
        }

        if let Some(showdown_stacks) = &data.showdown_stacks {
            game.showdown_stacks(&showdown_stacks)?;
        }
        Ok(game)
    }

    pub fn to_game_data(&self) -> GameData {
        let players = self
            .names
            .iter()
            .zip(self.seats.iter().copied())
            .zip(self.hands.iter().copied().map(|hand| {
                if hand == Hand::UNDEFINED {
                    None
                } else {
                    Some(hand)
                }
            }))
            .zip(self.starting_stacks.iter().copied())
            .map(|(((name, seat), hand), stack)| Player {
                name: Some(name.clone()),
                seat: Some(seat),
                hand,
                starting_stack: stack,
            })
            .collect();

        GameData {
            players,
            button_index: self.button_index,
            small_blind: self.small_blind,
            big_blind: self.big_blind,
            actions: self.actions.clone(),
            showdown_stacks: if self.showdown_stacks.iter().copied().any(|stack| stack != 0) {
                Some(self.showdown_stacks.clone())
            } else {
                None
            },
        }
    }

    pub fn to_validation_data(&mut self) -> GameValidationData {
        let game_data = self.to_game_data();
        self.rewind();
        let mut validations = Vec::new();
        loop {
            validations.push(GameValidationEntry {
                state: self.state(),
                check: self.can_check(),
                call: self.can_call(),
                bet: self.can_bet(),
                raise: self.can_raise(),
            });
            if !self.next() {
                break;
            }
        }
        GameValidationData {
            game: game_data,
            validations,
        }
    }

    pub fn reset(&mut self) {
        self.actions.clear();
        for street_stacks in self.stacks_in_street.iter_mut() {
            street_stacks.copy_from_slice(&self.starting_stacks);
        }
        self.showdown_stacks.iter_mut().for_each(|stack| *stack = 0);
        self.boards = [Board::EMPTY; Self::MAX_RUNOUTS];
        self.current_board = 0;
        self.current_street_index = 0;
        self.current_action_index = 0;
        self.current_player = u8::MAX;
        self.players_in_hand = Bitset::ones(self.player_count());
        self.hand_shown = Bitset::EMPTY;
        self.at_end = false;
        self.in_next = false;
    }

    pub fn player_names(&self) -> &[String] {
        &self.names
    }

    pub fn player_name(&self, player: usize) -> &str {
        assert!(player < self.player_count());
        &self.names[player]
    }

    pub fn player_by_name(&self, name: &str) -> Option<usize> {
        self.player_names().iter().position(|n| n == name)
    }

    pub fn seat(&self, player: usize) -> usize {
        assert!(player < self.player_count());
        usize::from(self.seats[player])
    }

    pub fn is_heads_up_table(&self) -> bool {
        self.player_count() == Self::MIN_PLAYERS
    }

    pub fn small_blind(&self) -> u32 {
        self.small_blind
    }

    pub fn small_blind_index(&self) -> usize {
        let button_offset = if self.is_heads_up_table() { 0 } else { 1 };
        (self.button_index() + button_offset) % self.player_count()
    }

    pub fn big_blind(&self) -> u32 {
        self.big_blind
    }

    pub fn big_blind_index(&self) -> usize {
        let button_offset = if self.is_heads_up_table() { 1 } else { 2 };
        (self.button_index() + button_offset) % self.player_count()
    }

    fn first_to_act_post_flop(&self) -> usize {
        (self.button_index() + 1) % self.player_count()
    }

    pub fn board(&self) -> Board {
        self.boards[usize::from(self.current_board)]
    }

    fn board_mut(&mut self) -> &mut Board {
        &mut self.boards[usize::from(self.current_board)]
    }

    fn current_street_stacks(&self) -> &[u32] {
        &self.stacks_in_street[self.board().street().to_usize()]
    }

    fn current_street_stacks_mut(&mut self) -> &mut [u32] {
        &mut self.stacks_in_street[self.board().street().to_usize()]
    }

    pub fn previous_street_stacks(&self) -> &[u32] {
        match self.board().street().previous() {
            Some(street) => &self.stacks_in_street[street.to_usize()],
            None => &self.starting_stacks,
        }
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
        self.starting_stacks[player] - self.current_street_stacks()[player]
    }

    pub fn invested_in_street(&self, player: usize) -> u32 {
        assert!(player < self.player_count());
        self.previous_street_stacks()[player] - self.current_street_stacks()[player]
    }

    pub fn has_cards(&self, index: usize) -> bool {
        assert!(index < self.player_count());
        self.players_in_hand.has(index)
    }

    pub fn players_in_hand(&self) -> impl Iterator<Item = usize> + '_ {
        self.players_in_hand.iter(self.player_count())
    }

    fn in_hand_not_all_in(&self, index: usize) -> bool {
        assert!(index < self.player_count());
        self.players_in_hand.has(index) && !self.is_all_in(index)
    }

    pub fn actions_in_street(&self) -> &[Action] {
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

    pub fn can_straddle(&self, player: usize) -> Result<u32> {
        if player >= self.player_count() {
            return Err("straddle: invalid player index".into());
        }
        if self.at_start() || self.board().street() != Street::PreFlop {
            return Err("straddle: only allowed pre flop after small/big blind post".into());
        }
        if self.is_all_in(player) {
            return Err("straddle: player is already all-in".into());
        }

        // Arbitrary decision, so the straddle is at least two big blinds.
        let mut last_full_straddle = self.big_blind;
        for action in self.actions_in_street().iter().copied() {
            let straddle = match action {
                Action::Straddle { amount, .. } => amount,
                Action::Post { .. } => continue,
                _ => {
                    return Err(
                        "straddle: only allowed after posters and before other actions".into(),
                    )
                }
            };

            // Arbitrary decision to require the next straddle
            // to be double the size of the last straddle.
            let Some(required_straddle) = last_full_straddle.checked_mul(2) else {
                return Err("straddle: overflow while computing next straddle".into());
            };
            if straddle >= required_straddle {
                last_full_straddle = straddle;
            }
        }

        let Some(required_straddle) = last_full_straddle.checked_mul(2) else {
            return Err("straddle: overflow while computing next straddle".into());
        };
        let min_straddle = min(self.starting_stacks[player], required_straddle);
        Ok(min_straddle)
    }

    pub fn can_check(&self) -> bool {
        let Some(player) = self.current_player() else {
            return false;
        };
        self.call_amount(player) == 0
    }

    pub fn can_call(&self) -> Option<u32> {
        let player = self.current_player()?;
        let amount = self.call_amount(player);
        if amount == 0 {
            None
        } else {
            Some(min(self.current_street_stacks()[player], amount))
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
            Some(min(self.current_street_stacks()[player], self.big_blind))
        } else {
            None
        }
    }

    pub fn can_raise(&self) -> Option<(u32, u32)> {
        // TODO: Should raise be allowed after all other players are all in?

        let player = self.current_player()?;
        let actions = self.actions_in_street();
        let mut last_amount = 0;
        let mut last_to = 0;
        for action in actions.iter().copied() {
            let amount_to = match action {
                Action::Bet { amount, .. } => Some((amount, amount)),
                Action::Raise { amount, to, .. } => Some((amount, to)),
                _ => None,
            };
            let Some((amount, to)) = amount_to else {
                continue;
            };
            if amount > last_amount {
                last_amount = amount;
            }
            last_to = to;
        }

        if last_amount == 0 {
            match self.board().street() {
                Street::PreFlop => {
                    last_amount = actions
                        .iter()
                        .copied()
                        .filter_map(|action| match action {
                            Action::Straddle { amount, .. } => Some(amount),
                            Action::Post { amount, .. } => Some(amount),
                            _ => None,
                        })
                        .max()
                        .unwrap();
                    last_to = last_amount;
                }
                _ => return None,
            }
        }
        assert_ne!(last_to, 0);
        if last_amount < self.big_blind {
            last_amount = self.big_blind;
        }

        let call_amount = self.call_amount(player);
        let old_stack = self.previous_street_stacks()[player];
        let current_stack = self.current_street_stacks()[player];
        let to = last_to + last_amount;
        if call_amount >= current_stack {
            None
        } else if to > old_stack {
            Some((current_stack, old_stack))
        } else {
            Some((last_amount, to))
        }
    }

    fn is_all_in(&self, player: usize) -> bool {
        self.current_street_stacks()[player] == 0
    }

    fn all_in_count(&self) -> usize {
        self.players_in_hand()
            .filter(|player| self.is_all_in(*player))
            .count()
    }

    fn action_ended(&self) -> bool {
        self.players_in_hand.count() == 1
            || (self.current_player().is_none() && self.board().street() == Street::River)
            || (self.current_player().is_none() && self.all_in_terminated_hand())
    }

    fn all_in_terminated_hand(&self) -> bool {
        self.players_in_hand.count() - 1 <= u32::try_from(self.all_in_count()).unwrap()
    }

    pub fn current_stack(&self) -> Option<u32> {
        self.current_player()
            .map(|player| self.current_street_stacks()[player])
    }

    pub fn current_stacks(&self) -> &[u32] {
        match self.state() {
            State::End => &self.showdown_stacks,
            _ => self.current_street_stacks(),
        }
    }

    pub fn button_index(&self) -> usize {
        usize::from(self.button_index)
    }

    pub fn player_count(&self) -> usize {
        self.starting_stacks.len()
    }

    fn player_count_u8(&self) -> u8 {
        u8::try_from(self.starting_stacks.len()).unwrap()
    }

    fn can_uncalled_bet(&self) -> Option<(usize, u32)> {
        if !self.action_ended() {
            return None;
        }
        let mut player_by_investment_array: [u8; Self::MAX_PLAYERS] =
            array::from_fn(|index| u8::try_from(index).unwrap());
        let player_by_investment = &mut player_by_investment_array[..self.player_count()];
        player_by_investment.sort_by_key(|player| self.invested(usize::from(*player)));
        let max_invested_player = player_by_investment[player_by_investment.len() - 1];
        let second_max_invested_player = player_by_investment[player_by_investment.len() - 2];
        let max_invested = self.invested(usize::from(max_invested_player));
        let second_max_invested = self.invested(usize::from(second_max_invested_player));
        if max_invested == second_max_invested {
            None
        } else {
            Some((
                usize::from(max_invested_player),
                max_invested - second_max_invested,
            ))
        }
    }

    fn next_show_or_muck(&self) -> Option<usize> {
        let not_allowed = !self.action_ended()
            || self.players_in_hand.count() == 1
            || self.hand_shown.count() == self.players_in_hand.count();
        if not_allowed {
            return None;
        }
        let start_index = self
            .actions_in_street()
            .iter()
            .rev()
            .copied()
            .find(|action| matches!(action, Action::Bet { .. } | Action::Raise { .. }))
            .and_then(|action| action.player())
            .unwrap_or_else(|| self.first_to_act_post_flop());
        (start_index..self.player_count())
            .chain(0..start_index)
            .filter(|player| self.has_cards(*player))
            .filter(|player| !self.hand_shown.has(*player))
            .next()
    }

    pub fn state(&self) -> State {
        if self.at_start() {
            State::Post
        } else if let Some(player) = self.current_player() {
            State::Player(player)
        } else if let Some((player, amount)) = self.can_uncalled_bet() {
            State::UncalledBet { player, amount }
        } else if let Some(player) = self.next_show_or_muck() {
            State::ShowOrMuck(player)
        } else if let Some(street) = self.can_next_street() {
            State::Street(street)
        } else if self.at_end {
            State::End
        } else {
            State::Showdown
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
            None => Err("currently no player selected".into()),
        }
    }

    fn at_start(&self) -> bool {
        self.current_action_index == 0
    }

    fn check_pre_update(&self) -> Result<()> {
        if !self.in_next && self.current_action_index != self.actions.len() {
            return Err("can't apply action: not at final action".into());
        }
        Ok(())
    }

    fn next_player(&mut self) {
        assert!(self.current_player().is_some());
        let actions = self.actions_in_street();

        let players_in_hand_not_all_in = self
            .players_in_hand()
            .filter(|player| !self.is_all_in(*player))
            .fold(Bitset::<2>::EMPTY, |set, player| set.with(player));

        let players_with_action = actions
            .iter()
            .filter(|action| !matches!(action, Action::Post { .. } | Action::Straddle { .. }))
            .filter_map(|action| action.player())
            .fold(Bitset::<2>::EMPTY, |set, player| set.with(player));

        let invested = self.invested_per_player().max().unwrap();
        let all_equal_investments = players_in_hand_not_all_in
            .iter(self.player_count())
            .map(|player| self.invested(player))
            .all(|n| n == invested);

        let can_skip = (players_in_hand_not_all_in.count() == 1
            || players_in_hand_not_all_in & players_with_action == players_in_hand_not_all_in)
            && all_equal_investments;
        if can_skip {
            self.current_player = u8::MAX;
            return;
        }

        self.next_player_in_hand_not_all_in();
    }

    fn next_player_in_hand_not_all_in(&mut self) {
        assert!(self.current_player().is_some());
        let current_player_start = self.current_player;
        loop {
            self.current_player = (self.current_player + 1) % self.player_count_u8();
            if self.current_player == current_player_start {
                self.current_player = u8::MAX;
                return;
            }
            if self.in_hand_not_all_in(usize::from(self.current_player)) {
                return;
            }
        }
    }

    fn players_in_hand_not_all_in(&self) -> usize {
        (0..self.player_count())
            .filter(|player| self.in_hand_not_all_in(*player))
            .count()
    }

    fn add_action(&mut self, action: Action) {
        if self.in_next {
            let next_action = self.actions[self.current_action_index];
            assert_eq!(action, next_action);
        } else {
            assert_eq!(self.current_action_index, self.actions.len());
            self.actions.push(action);
        }
        self.current_action_index += 1;
    }

    fn update_stack(&mut self, amount: u32) -> Result<()> {
        let player = self.current_player_result()?;
        if amount > self.current_street_stacks()[player] {
            return Err("player cannot afford sizing".into());
        }
        self.current_street_stacks_mut()[player] -= amount;
        Ok(())
    }

    fn action_post(&mut self, amount: u32) -> Result<()> {
        let player = self.current_player_result()?;
        let amount = min(self.current_street_stacks()[player], amount);
        self.update_stack(amount)?;
        self.add_action(Action::Post {
            player: self.current_player,
            amount,
        });
        self.next_player();
        Ok(())
    }

    pub fn post_small_and_big_blind(&mut self) -> Result<()> {
        self.check_pre_update()?;
        if !self.at_start() {
            return Err("can only post small and big blind before other actions".into());
        }
        self.current_player = self.button_index;
        if !self.is_heads_up_table() {
            self.current_player = (self.current_player + 1) % self.player_count_u8();
        }
        self.action_post(self.small_blind)?;
        self.action_post(self.big_blind)?;
        Ok(())
    }

    pub fn additional_post(&mut self, player: usize) -> Result<()> {
        self.check_pre_update()?;
        if player >= self.player_count() {
            return Err("additional post: invalid player index".into());
        }
        if self.at_start() || self.board().street() != Street::PreFlop {
            return Err("additional post: only allowed pre flop after small/big blind post".into());
        }

        let mut poster = Bitset::<2>::EMPTY;
        for action in self.actions_in_street().iter().copied() {
            match action {
                Action::Post { player, .. } => poster.set(usize::from(player)),
                _ => return Err("additional post: only allowed before all other actions".into()),
            }
        }
        if poster.has(usize::from(player)) {
            return Err("additional post: duplicate post not allowed".into());
        }

        let players =
            (self.small_blind_index()..self.player_count()).chain(0..self.small_blind_index());
        for current_player in players.rev() {
            if poster.has(current_player) {
                return Err(
                    "additional post: all players posted already or post not in order".into(),
                );
            }
            if current_player == player {
                break;
            }
        }

        let amount = min(self.current_street_stacks()[player], self.big_blind);
        self.current_street_stacks_mut()[player] -= amount;
        self.add_action(Action::Post {
            player: u8::try_from(player).unwrap(),
            amount,
        });
        Ok(())
    }

    pub fn straddle(&mut self, player: usize, amount: u32) -> Result<()> {
        self.check_pre_update()?;
        let required_straddle = self.can_straddle(player)?;
        if amount < required_straddle {
            return Err("straddle: amount too small".into());
        }
        if amount > self.starting_stacks[player] {
            return Err("straddle: player cannot afford amount".into());
        }

        self.current_street_stacks_mut()[player] = self.starting_stacks[player] - amount;
        let player = u8::try_from(player).unwrap();
        self.add_action(Action::Straddle { player, amount });
        // Always start left of the last straddler.
        self.current_player = (player + 1) % self.player_count_u8();
        Ok(())
    }

    pub fn fold(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        assert!(self.players_in_hand.has(player));
        self.players_in_hand.remove(player);
        self.add_action(Action::Fold(self.current_player));
        self.next_player();
        Ok(())
    }

    pub fn check(&mut self) -> Result<()> {
        self.check_pre_update()?;
        self.current_player_result()?;
        if !self.can_check() {
            return Err("player is not allowed to check".into());
        }
        self.add_action(Action::Check(self.current_player));
        self.next_player();
        Ok(())
    }

    pub fn call(&mut self) -> Result<()> {
        self.check_pre_update()?;
        self.current_player_result()?;
        let Some(amount) = self.can_call() else {
            return Err("player is not allowed to call".into());
        };
        self.update_stack(amount)?;
        self.add_action(Action::Call {
            player: self.current_player,
            amount,
        });
        self.next_player();
        Ok(())
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
        self.add_action(Action::Bet {
            player: self.current_player,
            amount,
        });
        self.next_player();
        Ok(())
    }

    pub fn raise(&mut self, to: u32) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        let Some((min_amount, min_to)) = self.can_raise() else {
            return Err("player is not allowed to raise".into());
        };
        if to < min_to {
            return Err("raise is smaller than the minimum".into());
        }

        let amount = min_amount + to - min_to;
        let previous_street_stack = self.previous_street_stacks()[player];
        if to > previous_street_stack {
            return Err("player cannot afford raise".into());
        }
        let old_stack = self.current_street_stacks()[player];
        self.current_street_stacks_mut()[player] = previous_street_stack - to;

        self.add_action(Action::Raise {
            player: self.current_player,
            old_stack,
            amount,
            to,
        });
        self.next_player();
        Ok(())
    }

    pub fn uncalled_bet(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let State::UncalledBet { player, amount } = self.state() else {
            return Err("uncalled bet: cannot return uncalled bet in current state".into());
        };
        self.current_street_stacks_mut()[player] += amount;
        self.add_action(Action::UncalledBet {
            player: u8::try_from(player).unwrap(),
            amount,
        });
        Ok(())
    }

    fn can_next_street(&self) -> Option<Street> {
        let allowed = self.next_show_or_muck().is_none()
            && self.current_player().is_none()
            && (!self.actions_in_street().is_empty() || self.players_in_hand_not_all_in() <= 1)
            && self.players_in_hand.count() > 1
            && self.board().street() != Street::River;
        if allowed {
            Some(self.board().street().next().unwrap())
        } else {
            None
        }
    }

    fn can_next_street_multiple_runouts(&self) -> Option<(Street, bool)> {
        match self.state() {
            State::Street(street) => Some((street, false)),
            State::Showdown => {
                if usize::from(self.current_board) >= Self::MAX_RUNOUTS - 1 {
                    None
                } else {
                    self.all_in_street()
                        .and_then(|street| street.next())
                        .map(|street| (street, true))
                }
            }
            _ => None,
        }
    }

    fn all_in_street(&self) -> Option<Street> {
        assert_eq!(self.state(), State::Showdown);
        if !self.all_in_terminated_hand() {
            return None;
        }

        let street = Street::STREETS[1..]
            .iter()
            .copied()
            .rev()
            .filter(|street| {
                let current_stacks = &self.stacks_in_street[street.to_usize()];
                let previous_stacks = &self.stacks_in_street[street.previous().unwrap().to_usize()];
                current_stacks != previous_stacks
            })
            .next()
            .unwrap_or(Street::PreFlop);
        Some(street)
    }

    fn prepare_new_street(&mut self, expected_street: Option<Street>) -> Result<Street> {
        let Some((street, new_runout)) = self.can_next_street_multiple_runouts() else {
            return Err("street: cannot go to next street".into());
        };
        if expected_street.is_some_and(|s| s != street) {
            return Err("street: cannot go to requested street".into());
        }

        if new_runout {
            self.current_board += 1;
            assert!(usize::from(self.current_board) < Self::MAX_RUNOUTS);

            let (previous_boards, next_boards) =
                self.boards.split_at_mut(usize::from(self.current_board));
            let previous_board = previous_boards.last().unwrap();
            let next_board = next_boards.first_mut().unwrap();

            let previous_street = street.previous().unwrap();
            let card_copy_count = previous_street.community_card_count();
            (&mut next_board.cards[..card_copy_count])
                .copy_from_slice(&previous_board.cards[..card_copy_count]);
            next_board.street = previous_street;

            let (previous_stacks, current_stacks) =
                self.stacks_in_street.split_at_mut(street.to_usize());
            let previous_stack = previous_stacks.last().unwrap();
            for stacks in current_stacks {
                stacks.copy_from_slice(previous_stack);
            }
        }

        Ok(street)
    }

    fn check_cards(&self) -> Result<Cards> {
        let mut known_cards = Cards::EMPTY;
        for hand in self
            .hands
            .iter()
            .copied()
            .filter(|hand| *hand != Hand::UNDEFINED)
        {
            if known_cards.has(hand.high()) {
                return Err(format!("duplicate card {} in hand", hand.high()).into());
            }
            if known_cards.has(hand.low()) {
                return Err(format!("duplicate card {} in hand", hand.low()).into());
            }
            known_cards.add(hand.high());
            known_cards.add(hand.low());
        }

        for card in self.boards[0].cards().iter().copied() {
            if known_cards.has(card) {
                return Err(format!("duplicate card {} on board", card).into());
            }
            known_cards.add(card);
        }

        if self.current_board > 0 {
            // Figure out which streets were run more than once.
            let mut street_counts = [0u8; Street::COUNT];
            for action in self.actions.iter().copied() {
                if let Some(street) = action.street() {
                    street_counts[street.to_usize()] += 1;
                }
            }

            let matching_community_cards = if street_counts[Street::Flop.to_usize()] > 1 {
                Street::PreFlop.community_card_count()
            } else if street_counts[Street::Turn.to_usize()] > 1 {
                Street::Flop.community_card_count()
            } else if street_counts[Street::River.to_usize()] > 1 {
                Street::Turn.community_card_count()
            } else {
                unreachable!()
            };

            for board in &self.boards[1..usize::from(self.current_board + 1)] {
                assert_eq!(
                    &board.cards()[..matching_community_cards],
                    &self.boards[0].cards()[..matching_community_cards]
                );

                for card in board.cards()[matching_community_cards..].iter().copied() {
                    if known_cards.has(card) {
                        return Err(format!("duplicate card {} on board", card).into());
                    }
                    known_cards.add(card);
                }
            }
        }

        Ok(known_cards)
    }

    fn next_street_final(&mut self) -> Result<()> {
        self.check_cards()?;

        let current_street = self.board().street();
        let (previous, next) = self
            .stacks_in_street
            .split_at_mut(current_street.to_usize());
        for stacks in next {
            stacks.copy_from_slice(previous.last().unwrap().as_slice());
        }

        if self.all_in_terminated_hand() {
            self.current_player = u8::MAX;
        } else {
            self.current_player = self.button_index;
            self.next_player_in_hand_not_all_in();
        }
        self.current_street_index = self.current_action_index;
        Ok(())
    }

    pub fn flop(&mut self, flop: [Card; 3]) -> Result<()> {
        self.check_pre_update()?;
        self.prepare_new_street(Some(Street::Flop))?;
        self.add_action(Action::Flop(flop));
        self.board_mut().street = Street::Flop;
        self.board_mut().cards[..3].copy_from_slice(flop.as_slice());
        self.next_street_final()?;
        Ok(())
    }

    pub fn turn(&mut self, turn: Card) -> Result<()> {
        self.check_pre_update()?;
        self.prepare_new_street(Some(Street::Turn))?;
        self.add_action(Action::Turn(turn));
        self.board_mut().street = Street::Turn;
        self.board_mut().cards[3] = turn;
        self.next_street_final()?;
        Ok(())
    }

    pub fn river(&mut self, river: Card) -> Result<()> {
        self.check_pre_update()?;
        self.prepare_new_street(Some(Street::River))?;
        self.add_action(Action::River(river));
        self.board_mut().street = Street::River;
        self.board_mut().cards[4] = river;
        self.next_street_final()?;
        Ok(())
    }

    pub fn draw_next_street(&mut self, rng: &mut impl Rng) -> Result<()> {
        self.check_pre_update()?;
        let street = self.prepare_new_street(None)?;
        let mut deck = Deck::from_cards(rng, self.known_cards());
        match street {
            Street::PreFlop => unreachable!(),
            Street::Flop => self.flop([
                deck.draw(rng).unwrap(),
                deck.draw(rng).unwrap(),
                deck.draw(rng).unwrap(),
            ]),
            Street::Turn => self.turn(deck.draw(rng).unwrap()),
            Street::River => self.river(deck.draw(rng).unwrap()),
        }
    }

    pub fn showdown_custom(
        &mut self,
        total_rake: u32,
        player_pot_share: impl Iterator<Item = (usize, u32)>,
    ) -> Result<()> {
        // TODO: Check winners are correct.

        self.check_pre_update()?;
        if self.state() != State::Showdown {
            return Err("showdown: not in showdown state".into());
        }

        let street = self.board().street();
        self.showdown_stacks
            .copy_from_slice(&self.stacks_in_street[street.to_usize()]);

        let mut total_pot = 0u32;
        for (player, pot_share) in player_pot_share {
            let Some(new_total_pot) = total_pot.checked_add(pot_share) else {
                return Err("showdown: amount won too large".into());
            };
            let Some(new_stack) = self.showdown_stacks[player].checked_add(pot_share) else {
                return Err("showdown: amount won too large".into());
            };
            total_pot = new_total_pot;
            self.showdown_stacks[player] = new_stack;
        }

        if total_pot.checked_add(total_rake) != Some(self.total_pot()) {
            return Err("showdown: total pot and supplied pot shares with rake don't match".into());
        }
        self.at_end = true;
        Ok(())
    }

    pub fn showdown_stacks(&mut self, stacks: &[u32]) -> Result<()> {
        // TODO: Check winners are correct.

        if stacks.len() != self.player_count() {
            return Err("showdown stacks: given stack count does not match player count".into());
        }
        let total = stacks.iter().copied().fold(Some(0u32), |total, stack| {
            total.and_then(|total| total.checked_add(stack))
        });
        let Some(total) = total else {
            return Err("showdown stacks: stack sum overflows".into());
        };
        if total > self.starting_stacks.iter().sum() {
            return Err("showdown stacks: showdown stacks are larger than starting stacks".into());
        }

        self.check_pre_update()?;
        if self.state() != State::Showdown {
            return Err("showdown stacks: not in showdown state".into());
        }
        self.showdown_stacks.copy_from_slice(stacks);
        self.at_end = true;
        Ok(())
    }

    pub fn showdown_simple(&mut self) -> Result<()> {
        // TODO: Custom rake.

        self.check_pre_update()?;
        if self.state() != State::Showdown {
            return Err("showdown: not in showdown state".into());
        }

        let street = self.board().street();
        self.showdown_stacks
            .copy_from_slice(&self.stacks_in_street[street.to_usize()]);

        for (pot, winners) in self.showdown_winners_by_pot()? {
            let winner_count = u32::try_from(winners.len()).unwrap();

            let won_per_player = pot / winner_count;
            for player in winners.iter().copied() {
                self.showdown_stacks[usize::from(player)] += won_per_player;
            }

            let n = usize::try_from(pot % winner_count).unwrap();
            for player in winners.iter().copied().take(n) {
                self.showdown_stacks[usize::from(player)] += 1;
            }
        }

        self.at_end = true;
        Ok(())
    }

    fn showdown_winners_by_pot(&self) -> Result<Vec<(u32, Vec<u8>)>> {
        if self.players_in_hand.count() == 1 {
            let winner = self
                .players_in_hand()
                .map(|player| u8::try_from(player).unwrap())
                .next()
                .unwrap();
            return Ok(vec![(self.total_pot(), vec![winner])]);
        }

        let mut scores_array = [Score::ZERO; Self::MAX_PLAYERS];
        let scores = &mut scores_array[..self.player_count()];
        // TODO: Showdown boards.
        let board = Cards::from_slice(self.board().cards()).unwrap();
        for player in self.players_in_hand() {
            let Some(hand) = self.get_hand(player) else {
                return Err(format!("showdown: missing hand for player {player}").into());
            };
            scores[player] = board.with(hand.high()).with(hand.low()).score_fast();
        }

        let mut investments_array = [0u32; Self::MAX_PLAYERS];
        let investments = &mut investments_array[..self.player_count()];
        for player in 0..self.player_count() {
            investments[player] = self.invested(player);
        }

        let winners = self.showdown_winners_by_pot_inner(scores, investments);
        assert_eq!(
            winners.iter().map(|(pot, _)| *pot).sum::<u32>(),
            self.total_pot()
        );
        Ok(winners)
    }

    fn showdown_winners_by_pot_inner(
        &self,
        scores: &[Score],
        investments: &mut [u32],
    ) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new();
        loop {
            let min_investment = investments
                .iter()
                .copied()
                .enumerate()
                .filter(|(player, investment)| self.players_in_hand.has(*player) && *investment > 0)
                .map(|(_, investment)| investment)
                .min();
            let Some(min_investment) = min_investment else {
                return out;
            };
            let winners = self.showdown_winners(scores, investments);
            let mut pot = 0;
            for investment in investments.iter_mut() {
                pot += min_investment - min_investment.saturating_sub(*investment);
                *investment = investment.saturating_sub(min_investment);
            }
            out.push((pot, winners));
        }
    }

    fn showdown_winners(&self, scores: &[Score], investments: &[u32]) -> Vec<u8> {
        let max_score = (0..self.player_count())
            .filter(|player| self.players_in_hand.has(*player) && investments[*player] > 0)
            .map(|player| scores[usize::from(player)])
            .max()
            .unwrap();
        let mut players: Vec<_> = (0..self.player_count_u8())
            .filter(|player| {
                self.players_in_hand.has(usize::from(*player))
                    && investments[usize::from(*player)] > 0
                    && scores[usize::from(*player)] == max_score
            })
            .collect();
        players.sort_by_key(|player| {
            let player_count = self.player_count_u8();
            (player_count - (self.button_index + 1) + *player) % player_count
        });
        players
    }

    pub fn draw_unset_hands(&mut self, rng: &mut impl Rng) {
        let mut deck = Deck::from_cards(rng, self.known_cards());
        for hand in &mut self.hands {
            if *hand != Hand::UNDEFINED {
                continue;
            }
            *hand = deck.hand(rng).unwrap();
        }
        self.check_cards().unwrap();
    }

    pub fn set_hand(&mut self, index: usize, hand: Hand) -> Result<()> {
        if index >= self.player_count() {
            return Err(format!("set hand: unknown player index {index}").into());
        }
        if self.hands[index] != Hand::UNDEFINED && self.hands[index] != hand {
            return Err(
                format!("set hand: cannot set different hand for player index {index}").into(),
            );
        }
        self.hands[index] = hand;
        self.check_cards()?;
        Ok(())
    }

    pub fn hand_shown(&self, player: usize) -> bool {
        assert!(player < self.player_count());
        self.hand_shown.has(player)
    }

    pub fn show_hand(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let State::ShowOrMuck(player) = self.state() else {
            return Err("show: cannot show hand in current state".into());
        };
        if self.hands[player] == Hand::UNDEFINED {
            return Err(format!("show: hand for player index {player} not set").into());
        }
        self.hand_shown.set(player);
        self.add_action(Action::Shows {
            player: u8::try_from(player).unwrap(),
            hand: self.hands[player],
        });
        Ok(())
    }

    pub fn get_hand(&self, index: usize) -> Option<Hand> {
        if index >= self.player_count() || self.hands[index] == Hand::UNDEFINED {
            None
        } else {
            Some(self.hands[index])
        }
    }

    pub fn apply_action(&mut self, action: Action) -> Result<()> {
        self.check_pre_update()?;
        if !matches!(action, Action::Straddle { .. }) && action.player() != self.current_player() {
            return Err("apply action: current player and action don't match".into());
        }
        match action {
            Action::Post { .. } => Err("apply action: cannot apply post".into()),
            Action::Straddle { player, amount } => self.straddle(usize::from(player), amount),
            Action::Fold(_) => self.fold(),
            Action::Check(_) => self.check(),
            Action::Call { amount, .. } => {
                if self.can_call() != Some(amount) {
                    return Err("apply action: cannot call or amount mismatch".into());
                }
                self.call()
            }
            Action::Bet { amount, .. } => self.bet(amount),
            // TODO: Check other raise values.
            Action::Raise { to, .. } => self.raise(to),
            Action::Flop(flop) => self.flop(flop),
            Action::Turn(turn) => self.turn(turn),
            Action::River(river) => self.river(river),
            Action::UncalledBet { player, amount } => {
                let expected_state = State::UncalledBet {
                    player: usize::from(player),
                    amount,
                };
                if self.state() != expected_state {
                    return Err("apply action: uncalled bet not allowed or invalid".into());
                }
                self.uncalled_bet()
            }
            Action::Shows { player, hand } => {
                if self.state() != State::ShowOrMuck(usize::from(player))
                    || self.get_hand(usize::from(player)) != Some(hand)
                {
                    return Err("apply action: show not allowed or invalid".into());
                }
                self.show_hand()
            }
        }
    }

    pub fn known_cards(&self) -> Cards {
        self.check_cards().unwrap()
    }

    pub fn can_previous(&self) -> bool {
        self.current_action_index > 0
    }

    pub fn rewind(&mut self) {
        while self.previous() {}
    }

    pub fn previous(&mut self) -> bool {
        if !self.can_previous() {
            return false;
        }
        match self.state() {
            State::Post => unreachable!(),
            State::End => {
                self.at_end = false;
                return true;
            }
            _ => (),
        }

        let action = self.actions[self.current_action_index - 1];
        match action {
            Action::Post { .. } | Action::Straddle { .. } => {
                self.stacks_in_street[Street::PreFlop.to_usize()]
                    .copy_from_slice(&self.starting_stacks);
                self.current_player = u8::MAX;
                self.current_action_index = 0;
            }
            Action::Fold(player) => {
                self.current_player = player;
                self.players_in_hand.set(usize::from(player));
            }
            Action::Check(player) => self.current_player = player,
            Action::Call { player, amount } | Action::Bet { player, amount } => {
                self.current_player = player;
                self.current_street_stacks_mut()[usize::from(player)] += amount;
            }
            Action::Raise {
                player, old_stack, ..
            } => {
                self.current_player = player;
                self.current_street_stacks_mut()[usize::from(player)] = old_stack;
            }
            Action::Flop(_) => self.previous_street(),
            Action::Turn(_) => self.previous_street(),
            Action::River(_) => self.previous_street(),
            Action::UncalledBet { player, amount } => {
                self.current_street_stacks_mut()[usize::from(player)] -= amount;
            }
            Action::Shows { player, .. } => {
                self.hand_shown.remove(usize::from(player));
            }
        }
        if !matches!(action, Action::Post { .. } | Action::Straddle { .. }) {
            self.current_action_index -= 1;
        }
        true
    }

    fn previous_street(&mut self) {
        let current_street = self.board().street();
        let action_before = self.actions[self.current_action_index.checked_sub(2).unwrap()];
        let previous_runout_street = action_before.street();

        if previous_runout_street.is_some_and(|s| s == Street::River) {
            // Start of new runout.
            *self.board_mut() = Board::EMPTY;
            assert!(self.current_board > 0);
            self.current_board -= 1;
        } else {
            let previous_street = current_street.previous().unwrap();
            self.board_mut().street = previous_street;
            self.board_mut().cards[previous_street.community_card_count()..]
                .iter_mut()
                .for_each(|card| *card = Card::MIN);
        }

        self.current_street_index = self.actions[..self.current_street_index - 1]
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, action)| action.is_street())
            .filter_map(|(index, _)| index.checked_add(1))
            .next()
            .unwrap_or(0);
        self.current_player = u8::MAX;
    }

    pub fn can_next(&self) -> bool {
        let showdown_stacks_set = self.showdown_stacks.iter().copied().any(|stack| stack != 0);
        let at_final_action = self.current_action_index == self.actions.len();
        match self.state() {
            State::Showdown if showdown_stacks_set => true,
            State::Showdown if !at_final_action => true,
            State::Showdown => false,
            _ if at_final_action => false,
            _ => true,
        }
    }

    pub fn next(&mut self) -> bool {
        if !self.can_next() {
            return false;
        }
        match self.state() {
            State::Showdown if self.current_action_index == self.actions.len() => {
                self.at_end = true;
                return true;
            }
            State::End => unreachable!(),
            _ => (),
        }

        let next_action = self.actions[self.current_action_index];
        self.in_next = true;
        let result = match next_action {
            Action::Post { .. } => self.next_posts_straddles(),
            Action::Straddle { .. } => unreachable!(),
            Action::Fold(_) => self.fold(),
            Action::Check(_) => self.check(),
            Action::Call { .. } => self.call(),
            Action::Bet { amount, .. } => self.bet(amount),
            Action::Raise { to, .. } => self.raise(to),
            Action::Flop(flop) => self.flop(flop),
            Action::Turn(turn) => self.turn(turn),
            Action::River(river) => self.river(river),
            Action::UncalledBet { .. } => self.uncalled_bet(),
            Action::Shows { .. } => self.show_hand(),
        };
        self.in_next = false;
        result.unwrap();
        true
    }

    fn next_posts_straddles(&mut self) -> Result<()> {
        self.post_small_and_big_blind()?;

        while self.current_action_index < self.actions.len() {
            match self.actions[self.current_action_index] {
                Action::Post { player, .. } => self.additional_post(usize::from(player))?,
                Action::Straddle { player, amount } => {
                    self.straddle(usize::from(player), amount)?;
                }
                _ => break,
            }
        }

        Ok(())
    }

    pub fn internal_asserts_full(&self) {
        self.internal_asserts_state();
        self.internal_asserts_parse_roundtrip();
        self.internal_asserts_history();
    }

    pub(crate) fn internal_asserts_state(&self) {
        if let Some(player) = self.current_player() {
            assert_eq!(self.state(), State::Player(player));
            assert!(self.has_cards(player));
            assert!(!self.is_all_in(player));
            assert!(self.can_next_street().is_none());
            assert!(self.next_show_or_muck().is_none());
            assert!(
                self.can_check()
                    || self.can_call().is_some()
                    || self.can_bet().is_some()
                    || self.can_raise().is_some()
            );
        }

        if self.can_check() {
            let player = self.current_player().unwrap();
            assert!(self.can_call().is_none());

            if self.board().street() == Street::PreFlop {
                assert_eq!(self.call_amount(player), 0);
                assert!(!self
                    .actions_in_street()
                    .iter()
                    .any(|action| matches!(action, Action::Raise { .. })));
            } else {
                assert!(self.can_bet().is_some());
            }
        }

        if self.can_call().is_some() {
            assert!(!self.can_check());
            assert!(self.can_bet().is_none());
        }

        if self.can_bet().is_some() {
            assert_ne!(self.board().street(), Street::PreFlop);
            assert!(self.can_check());
            assert!(self.can_call().is_none());
            assert!(self.can_raise().is_none());
        }

        if let Some((_, to)) = self.can_raise() {
            let player = self.current_player().unwrap();
            if let Some(call_amount) = self.can_call() {
                let raise_investment = to.checked_sub(self.invested_in_street(player)).unwrap();
                assert!(call_amount < raise_investment);
            } else {
                assert_eq!(self.board().street(), Street::PreFlop);
            }
        }
    }

    fn internal_asserts_parse_roundtrip(&self) {
        let data = serde_json::to_string_pretty(&self.to_game_data()).unwrap();
        let parsed_game = Game::from_game_data(&serde_json::from_str(&data).unwrap()).unwrap();
        assert_eq!(self, &parsed_game);
    }

    fn internal_asserts_history(&self) {
        assert_eq!(self.state(), State::End);
        let mut games = Vec::new();
        let mut new_game = self.clone();
        new_game.reset();
        games.push(new_game.clone());
        assert!(!new_game.previous());
        assert!(!new_game.next());

        new_game.post_small_and_big_blind().unwrap();
        for action in self.actions[2..].iter().copied() {
            match action {
                Action::Post { player, .. } => {
                    new_game.additional_post(usize::from(player)).unwrap();
                }
                Action::Straddle { player, amount } => {
                    new_game.straddle(usize::from(player), amount).unwrap();
                }
                _ => break,
            }
        }
        games.push(new_game.clone());
        assert!(new_game.previous());
        assert!(new_game.next());
        assert_eq!(&new_game, games.last().unwrap());

        let action_iter =
            self.actions.iter().copied().skip_while(|action| {
                matches!(action, Action::Post { .. } | Action::Straddle { .. })
            });
        for action in action_iter {
            new_game.apply_action(action).unwrap();
            games.push(new_game.clone());
            assert!(new_game.previous());
            assert!(new_game.next());
            assert_eq!(&new_game, games.last().unwrap());
        }

        assert!(!new_game.next());
        new_game.showdown_stacks(&self.showdown_stacks).unwrap();
        games.push(new_game.clone());
        assert_eq!(self, &new_game);

        for expected in games.iter().rev().skip(1) {
            assert!(new_game.previous());
            new_game.internal_asserts_history_compare(expected);
        }
        assert!(!new_game.previous());

        for expected in games.iter().skip(1) {
            assert!(new_game.next());
            new_game.internal_asserts_history_compare(expected);
        }
        assert!(!new_game.next());
    }

    fn internal_asserts_history_compare(&self, expected: &Game) {
        self.internal_asserts_state();

        assert_eq!(expected.boards, self.boards);
        assert_eq!(expected.current_board, self.current_board);
        assert_eq!(expected.names, self.names);
        assert_eq!(expected.starting_stacks, self.starting_stacks);
        let current_street = expected.board().street();
        assert_eq!(
            &expected.stacks_in_street[..current_street.to_usize()],
            &self.stacks_in_street[..current_street.to_usize()],
        );
        assert_eq!(expected.button_index, self.button_index);
        assert_eq!(expected.current_street_index, self.current_street_index);
        assert_eq!(expected.current_action_index, self.current_action_index);
        assert_eq!(expected.current_player, self.current_player);
        assert_eq!(expected.players_in_hand, self.players_in_hand);
        assert_eq!(expected.small_blind, self.small_blind);
        assert_eq!(expected.big_blind, self.big_blind);
        assert_eq!(expected.hands, self.hands);
        assert_eq!(expected.hand_shown, self.hand_shown);

        assert_eq!(expected.actions_in_street(), self.actions_in_street());
        assert_eq!(expected.state(), self.state());
        assert_eq!(expected.can_check(), self.can_check());
        assert_eq!(expected.can_call(), self.can_call());
        assert_eq!(expected.can_bet(), self.can_bet());
        assert_eq!(expected.can_raise(), self.can_raise());
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub name: Option<String>,
    pub seat: Option<u8>,
    pub hand: Option<Hand>,
    pub starting_stack: u32,
}

impl Player {
    pub fn with_starting_stack(starting_stack: u32) -> Self {
        Self {
            name: None,
            seat: None,
            hand: None,
            starting_stack,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameData {
    pub players: Vec<Player>,
    pub button_index: u8,
    pub small_blind: u32,
    pub big_blind: u32,
    pub actions: Vec<Action>,
    pub showdown_stacks: Option<Vec<u32>>,
}

impl Default for GameData {
    fn default() -> Self {
        Self {
            players: vec![Player::with_starting_stack(1_000); 6],
            button_index: 0,
            small_blind: 5,
            big_blind: 10,
            actions: Vec::new(),
            showdown_stacks: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameValidationData {
    #[serde(flatten)]
    pub game: GameData,
    pub validations: Vec<GameValidationEntry>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameValidationEntry {
    pub state: State,
    pub check: bool,
    pub call: Option<u32>,
    pub bet: Option<u32>,
    pub raise: Option<(u32, u32)>,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    #[test]
    fn test_game_with_sample_hands() {
        unsafe {
            crate::init::init();
        }

        let path = Path::new("src")
            .join("test_data")
            .join("game_validation_data.json");
        let validation_data_content = fs::read_to_string(path).unwrap();
        let validation_data: Vec<GameValidationData> =
            serde_json::from_str(&validation_data_content).unwrap();

        for game_entry in validation_data {
            let mut game = Game::from_game_data(&game_entry.game).unwrap();
            game.internal_asserts_full();

            game.rewind();
            for validation in game_entry.validations {
                assert_eq!(validation.state, game.state());
                assert_eq!(validation.check, game.can_check());
                assert_eq!(validation.call, game.can_call());
                assert_eq!(validation.bet, game.can_bet());
                assert_eq!(validation.raise, game.can_raise());
                game.next();
            }
            assert!(!game.next());
        }
    }
}
