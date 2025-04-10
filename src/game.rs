use std::cmp::min;
use std::{array, fmt, usize};

use rand::Rng;

use crate::card::Card;
use crate::cards::{Cards, Score};
use crate::deck::Deck;
use crate::hand::Hand;
use crate::result::Result;

// TODO:
// - Bet/raise steps.
// - Mucking

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Post(u8, u32),
    Fold(u8),
    Check(u8),
    Call(u8, u32),
    Bet(u8, u32),
    Raise { player: u8, amount: u32, to: u32 },
    Flop([Card; 3]),
    Turn(Card),
    River(Card),
    Shows(u8, Hand),
}

impl Action {
    fn is_street(self) -> bool {
        matches!(self, Action::Flop(_) | Action::Turn(_) | Action::River(_))
    }

    fn is_player(self) -> bool {
        matches!(
            self,
            Action::Post(_, _)
                | Action::Fold(_)
                | Action::Check(_)
                | Action::Call(_, _)
                | Action::Bet(_, _)
                | Action::Raise { .. }
        )
    }

    fn to_amount(self) -> Option<u32> {
        match self {
            Action::Post(_, amount) => Some(amount),
            Action::Call(_, amount) => Some(amount),
            Action::Bet(_, amount) => Some(amount),
            Action::Raise { amount, .. } => Some(amount),
            _ => None,
        }
    }

    fn player(self) -> Option<usize> {
        let player = match self {
            Action::Post(player, _) => player,
            Action::Fold(player) => player,
            Action::Check(player) => player,
            Action::Call(player, _) => player,
            Action::Bet(player, _) => player,
            Action::Raise { player, .. } => player,
            _ => return None,
        };
        Some(usize::from(player))
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Street {
    PreFlop = 0,
    Flop = 1,
    Turn = 2,
    River = 3,
}

impl Street {
    pub const COUNT: usize = 4;

    fn previous(self) -> Option<Self> {
        match self {
            Street::PreFlop => None,
            Street::Flop => Some(Street::PreFlop),
            Street::Turn => Some(Street::Flop),
            Street::River => Some(Street::Turn),
        }
    }

    fn next(self) -> Option<Self> {
        match self {
            Street::PreFlop => Some(Street::Flop),
            Street::Flop => Some(Street::Turn),
            Street::Turn => Some(Street::River),
            Street::River => None,
        }
    }

    fn to_usize(self) -> usize {
        self as usize
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)] // TODO: Custom Debug
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
    Post,
    Player(usize),
    Street(Street),
    ShowOrMuck(usize),
    Showdown,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    actions: Vec<Action>,
    board: Board,
    names: Vec<String>,
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
}

impl Game {
    pub const MIN_PLAYERS: usize = 2;
    pub const MAX_PLAYERS: usize = 9;

    pub const TOTAL_CARDS: usize = 5;

    pub const POSITION_NAMES: [&[(&str, &str)]; Self::MAX_PLAYERS - Self::MIN_PLAYERS + 1] = [
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

    pub fn new(
        stacks: Vec<u32>,
        names: Option<Vec<String>>,
        button_index: usize,
        small_blind: u32,
        big_blind: u32,
    ) -> Result<Self> {
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
        let names = names.unwrap_or_else(|| {
            let names = Self::POSITION_NAMES[player_count - Self::MIN_PLAYERS];
            (0..player_count)
                .map(|player| {
                    let index = (player_count - button_index + player) % player_count;
                    names[index].0.to_string()
                })
                .collect()
        });
        if names.len() != stacks.len() {
            return Err("names and stacks have different lengths".into());
        }

        Ok(Self {
            actions: Vec::new(),
            names,
            starting_stacks: stacks.clone(),
            stacks_in_street: array::from_fn(|_| stacks.clone()),
            showdown_stacks: vec![0; player_count],
            board: Board::EMPTY,
            button_index: u8::try_from(button_index).unwrap(),
            current_street_index: 0,
            current_action_index: 0,
            current_player: u8::MAX,
            players_in_hand: Bitset::ones(player_count),
            small_blind,
            big_blind,
            hands: vec![Hand::UNDEFINED; player_count],
            hand_shown: Bitset::EMPTY,
        })
    }

    pub fn player_names(&self) -> &[String] {
        &self.names
    }

    pub fn player_name(&self, player: usize) -> &str {
        assert!(player < self.player_count());
        &self.names[player]
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
        self.board
    }

    fn current_street_stacks(&self) -> &[u32] {
        &self.stacks_in_street[self.board.street.to_usize()]
    }

    fn current_street_stacks_mut(&mut self) -> &mut [u32] {
        &mut self.stacks_in_street[self.board.street.to_usize()]
    }

    pub fn previous_street_stacks(&self) -> &[u32] {
        match self.board.street.previous() {
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
            self.actions_in_street().iter().all(|action| {
                matches!(
                    action,
                    Action::Fold(_) | Action::Call(_, _) | Action::Post(_, _)
                )
            })
        } else {
            self.actions_in_street()
                .iter()
                .all(|action| matches!(action, Action::Check(_) | Action::Fold(_)))
        }
    }

    pub fn can_call(&self) -> Option<u32> {
        let player = self.current_player()?;
        let can_call = self.actions_in_street().iter().any(|action| {
            matches!(
                action,
                Action::Post(_, _) | Action::Bet(_, _) | Action::Raise { .. }
            )
        });
        if can_call {
            let amount = self.call_amount(player);
            if amount == 0 {
                assert_eq!(self.board.street, Street::PreFlop);
                assert_eq!(usize::from(self.current_player), self.big_blind_index());
                None
            } else {
                Some(min(self.current_street_stacks()[player], amount))
            }
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
            Some(min(self.current_street_stacks()[player], self.big_blind))
        } else {
            None
        }
    }

    pub fn can_raise(&self) -> Option<(u32, u32)> {
        let player = self.current_player()?;
        let actions = self.actions_in_street();
        let mut last_amount = 0;
        let mut last_to = 0;
        for action in actions.iter().copied() {
            let amount_to = match action {
                Action::Bet(_, amount) => Some((amount, amount)),
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
            match self.board.street {
                Street::PreFlop => {
                    last_amount = actions
                        .iter()
                        .copied()
                        .filter_map(|action| match action {
                            Action::Post(_, amount) => Some(amount),
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

    pub fn raise_in_street(&self) -> bool {
        self.actions_in_street()
            .iter()
            .any(|action| matches!(action, Action::Raise { .. }))
    }

    fn is_all_in(&self, player: usize) -> bool {
        self.current_street_stacks()[player] == 0
    }

    fn all_in_count(&self) -> usize {
        self.players_in_hand()
            .filter(|player| self.is_all_in(*player))
            .count()
    }

    pub fn action_ended(&self) -> bool {
        self.players_in_hand.count() == 1
            || (self.current_player().is_none() && self.board.street == Street::River)
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

    pub fn next_show_or_muck(&self) -> Option<usize> {
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
            .find(|action| matches!(action, Action::Bet(_, _) | Action::Raise { .. }))
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
        } else if let Some(player) = self.next_show_or_muck() {
            State::ShowOrMuck(player)
        } else if let Some(street) = self.can_next_street() {
            State::Street(street)
        } else if self.showdown_stacks.iter().copied().any(|stack| stack != 0) {
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
        let players_in_hand_not_all_in = self.players_in_hand_not_all_in();
        let check_count = actions
            .iter()
            .copied()
            .filter(|action| matches!(action, Action::Check(_)))
            .count();
        let all_check_or_fold = actions
            .iter()
            .copied()
            .all(|action| matches!(action, Action::Check(_) | Action::Fold(_)));
        if all_check_or_fold && check_count == players_in_hand_not_all_in {
            self.current_player = u8::MAX;
            return Ok(());
        }

        let last_bettor_callers = {
            let mut s = Bitset::<2>::EMPTY;
            for action in actions.iter().copied() {
                match action {
                    Action::Fold(_) | Action::Check(_) => (),
                    Action::Call(player, _) => {
                        if self.in_hand_not_all_in(usize::from(player)) {
                            s.set(usize::from(player))
                        }
                    }
                    Action::Post(player, _)
                    | Action::Bet(player, _)
                    | Action::Raise { player, .. } => {
                        s = Bitset::EMPTY;
                        if self.in_hand_not_all_in(usize::from(player)) {
                            s.set(usize::from(player));
                        }
                    }
                    _ => unreachable!(),
                }
            }
            s
        };
        if usize::try_from(last_bettor_callers.count()).unwrap() == players_in_hand_not_all_in {
            if self.board.street == Street::PreFlop
                && self.current_player().unwrap() == self.small_blind_index()
            {
                self.current_player = self.big_blind_index() as u8;
                let can_check = self.can_check();
                // TODO: Fails if button raised.
                assert!(can_check);
                return Ok(());
            }
            self.current_player = u8::MAX;
            return Ok(());
        }

        self.next_player_in_hand_not_all_in()
    }

    fn next_player_in_hand_not_all_in(&mut self) -> Result<()> {
        self.current_player_result()?;
        let current_player_start = self.current_player;
        loop {
            self.current_player = (self.current_player + 1) % self.player_count_u8();
            if self.current_player == current_player_start {
                self.current_player = u8::MAX;
                return Ok(());
            }
            if self.in_hand_not_all_in(usize::from(self.current_player)) {
                return Ok(());
            }
        }
    }

    fn players_in_hand_not_all_in(&self) -> usize {
        (0..self.player_count())
            .filter(|player| self.in_hand_not_all_in(*player))
            .count()
    }

    fn add_action(&mut self, action: Action) {
        assert_eq!(self.current_action_index, self.actions.len());
        self.actions.push(action);
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
        self.add_action(Action::Post(self.current_player, amount));
        self.next_player()
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
        let old_stack = self.previous_street_stacks()[player];
        if to > old_stack {
            return Err("player cannot afford raise".into());
        }
        self.current_street_stacks_mut()[player] = old_stack - to;

        self.add_action(Action::Raise {
            player: self.current_player,
            amount,
            to,
        });
        self.next_player()
    }

    pub fn uncalled_bet(&mut self, player: usize, amount: u32) -> Result<()> {
        // TODO: Workaround
        self.check_pre_update()?;
        // TODO: Check amount valid
        let Some(new_stack) = self.current_street_stacks()[player].checked_add(amount) else {
            return Err("uncalled bet: invalid amount".into());
        };
        self.current_street_stacks_mut()[player] = new_stack;
        Ok(())
    }

    pub fn can_next_street(&self) -> Option<Street> {
        let allowed = self.next_show_or_muck().is_none()
            && self.current_player().is_none()
            && (!self.actions_in_street().is_empty() || self.players_in_hand_not_all_in() <= 1)
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
        for card in self.board.cards().iter().copied() {
            if known_cards.has(card) {
                return Err(format!("duplicate card {} on board", card).into());
            }
            known_cards.add(card);
        }
        Ok(known_cards)
    }

    fn next_street_final(&mut self) -> Result<()> {
        self.check_cards()?;

        let (previous, next) = self
            .stacks_in_street
            .split_at_mut(self.board.street.to_usize());
        next.first_mut()
            .unwrap()
            .copy_from_slice(previous.last().unwrap().as_slice());

        if self.all_in_terminated_hand() {
            self.current_player = u8::MAX;
        } else {
            self.current_player = self.button_index;
            self.next_player_in_hand_not_all_in()?;
        }
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
        self.showdown_stacks
            .copy_from_slice(&self.stacks_in_street[self.board.street.to_usize()]);
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
        Ok(())
    }

    pub fn showdown_simple(&mut self) -> Result<()> {
        // TODO:
        // - Custom Rake
        // - Showdown order
        // - Some players muck

        self.check_pre_update()?;
        if self.state() != State::Showdown {
            return Err("showdown: not in showdown state".into());
        }
        self.showdown_stacks
            .copy_from_slice(&self.stacks_in_street[self.board.street.to_usize()]);
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
        Ok(())
    }

    fn showdown_winners_by_pot(&self) -> Result<Vec<(u32, Vec<u8>)>> {
        let mut scores_array = [Score::ZERO; Self::MAX_PLAYERS];
        let scores = &mut scores_array[..self.player_count()];
        let board = Cards::from_slice(self.board.cards()).unwrap();
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

    pub fn set_hand(&mut self, index: usize, hand: Hand) -> Result<()> {
        if index >= self.player_count() {
            return Err(format!("unknown player index {index}").into());
        }
        if self.hands[index] != Hand::UNDEFINED {
            return Err(format!("hand for player index {index} already set").into());
        }
        self.hands[index] = hand;
        self.check_cards()?;
        Ok(())
    }

    pub fn show_hand(&mut self, hand: Hand) -> Result<()> {
        self.check_pre_update()?;
        let State::ShowOrMuck(player) = self.state() else {
            return Err("cannot show hand in current state".into());
        };
        if self.hands[player] != Hand::UNDEFINED && hand != self.hands[player] {
            return Err("cannot show different hand than already set".into());
        }
        self.hands[player] = hand;
        self.hand_shown.set(player);
        self.check_cards()?;
        Ok(())
    }

    pub fn get_hand(&self, index: usize) -> Option<Hand> {
        if index >= self.player_count() || self.hands[index] == Hand::UNDEFINED {
            None
        } else {
            Some(self.hands[index])
        }
    }

    pub fn known_cards(&self) -> Cards {
        self.check_cards().unwrap()
    }
}
