use std::cmp::min;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::num::NonZeroU8;
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::sync::Arc;
use std::{array, fmt, usize};

use chrono::NaiveDateTime;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;

use crate::bitset::Bitset;
use crate::card::Card;
use crate::cards::{Cards, Score};
use crate::deck::Deck;
use crate::hand::Hand;
use crate::result::{Error, Result};

// TODO:
// - Bet/raise steps
// - Poison game on error or ensure every error is recoverable
// - Nicer handling of state method with multiple runouts
// - Add single mucks action if there is only one winner

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8")]
pub struct Player(u8);

impl Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<u8> for Player {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        if usize::from(value) >= Game::MAX_PLAYERS {
            return Err("player index too large".into());
        }
        Ok(Self(value))
    }
}

impl TryFrom<usize> for Player {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self> {
        if value >= Game::MAX_PLAYERS {
            return Err("player index too large".into());
        }
        Ok(Self(u8::try_from(value).unwrap()))
    }
}

impl From<Player> for u8 {
    fn from(player: Player) -> Self {
        player.0
    }
}

impl From<Player> for usize {
    fn from(player: Player) -> Self {
        usize::from(player.0)
    }
}

impl Player {
    pub const ZERO: Self = Self(0);

    const UNSET: Self = Self(u8::MAX);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8")]
pub struct Seat(u8);

impl TryFrom<u8> for Seat {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        if usize::from(value) >= Game::MAX_PLAYERS {
            return Err("seat index too large".into());
        }
        Ok(Self(value))
    }
}

impl TryFrom<usize> for Seat {
    type Error = Error;

    fn try_from(value: usize) -> Result<Self> {
        if value >= Game::MAX_PLAYERS {
            return Err("seat index too large".into());
        }
        Ok(Self(u8::try_from(value).unwrap()))
    }
}

impl From<Seat> for u8 {
    fn from(seat: Seat) -> Self {
        seat.0
    }
}

impl From<Seat> for usize {
    fn from(seat: Seat) -> Self {
        usize::from(seat.0)
    }
}

impl Seat {
    pub const ZERO: Self = Seat(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Amount(u32);

impl Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for Amount {
    fn from(amount: u32) -> Self {
        Self(amount)
    }
}

impl From<Amount> for u32 {
    fn from(amount: Amount) -> Self {
        amount.0
    }
}

impl From<Amount> for f64 {
    fn from(amount: Amount) -> Self {
        f64::from(amount.0)
    }
}

impl Add for Amount {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl AddAssign for Amount {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl Sub for Amount {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl SubAssign for Amount {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Amount {
    pub const ZERO: Self = Self(0);

    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    pub fn to_milli_big_blinds_rounded(self, big_blind: Self) -> MilliBigBlind {
        let full_blinds = self.0 / big_blind.0;
        let remainder = self.0 % big_blind.0;

        let frac = ((1.0 / f64::from(big_blind.0)) * f64::from(remainder) * 1000.0).round();

        let mbb = i64::from(full_blinds) * 1000 + frac as i64;
        MilliBigBlind::new(mbb)
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.0.checked_add(rhs.0).map(Self::from)
    }

    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        self.0.checked_sub(rhs.0).map(Self::from)
    }

    pub fn checked_mul(self, rhs: u32) -> Option<Self> {
        self.0.checked_mul(rhs).map(Self::from)
    }

    pub fn saturating_add(self, rhs: Self) -> Self {
        self.0.saturating_add(rhs.0).into()
    }

    pub fn saturating_sub(self, rhs: Self) -> Self {
        self.0.saturating_sub(rhs.0).into()
    }

    pub fn saturating_mul(self, rhs: u32) -> Self {
        self.0.saturating_mul(rhs).into()
    }
}

// TODO: Check that after changing this from i32 to i64 no overflow issues occur.
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct MilliBigBlind(i64);

impl Display for MilliBigBlind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<f64> for MilliBigBlind {
    type Error = Error;

    fn try_from(n: f64) -> Result<Self> {
        let n = n * 1_000.0;
        if n.is_nan() || n.is_infinite() {
            return Err("milli big blind: value is not valid".into());
        }
        let n = n.round();

        if n < Self::MIN.0 as f64 || n > Self::MAX.0 as f64 {
            return Err("milli big blind: value too small or large".into());
        }

        Ok(MilliBigBlind(n as i64))
    }
}

impl From<MilliBigBlind> for i64 {
    fn from(n: MilliBigBlind) -> Self {
        n.0
    }
}

impl MilliBigBlind {
    pub const MIN: Self = Self(i64::MIN);
    pub const MAX: Self = Self(i64::MAX);

    pub const ZERO: Self = Self(0);

    pub const BIG_BLIND: Self = Self(1_000);

    pub const fn new(n: i64) -> Self {
        Self(n)
    }

    pub fn to_f64_approximate(self) -> f64 {
        // This might have some rounding issues.
        let full_blinds = (self.0 / 1000) as f64;
        let frac = (self.0 % 1000) as f64 / 1000.0;

        // This might have some rounding issues.
        full_blinds + frac
    }

    pub fn to_amount_rounded(self, big_blind: Amount) -> Option<Amount> {
        if self.0 < 0 {
            return None;
        }

        let full_blinds = self.0 / 1000;
        let frac = u32::try_from(self.0 % 1000).ok()?;

        let full_blinds = u32::try_from(full_blinds)
            .ok()
            .and_then(|n| n.checked_mul(big_blind.0))?;

        let frac = ((f64::from(frac) / 1000.0) * f64::from(big_blind.0)).round();

        full_blinds.checked_add(frac as u32).map(Amount::new)
    }

    pub fn abs_diff(self, other: Self) -> u64 {
        self.0.abs_diff(other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Post {
        player: Player,
        amount: Amount,
        dead: bool,
    },
    Straddle {
        player: Player,
        amount: Amount,
    },
    Fold(Player),
    Check(Player),
    Call {
        player: Player,
        amount: Amount,
    },
    Bet {
        player: Player,
        amount: Amount,
    },
    Raise {
        player: Player,
        old_stack: Amount,
        amount: Amount,
        to: Amount,
    },
    Flop([Card; 3]),
    Turn(Card),
    River(Card),
    UncalledBet {
        player: Player,
        amount: Amount,
    },
    Shows {
        player: Player,
        hand: Hand,
    },
    MucksOrUnknown(Player),
}

impl Action {
    pub fn kind_str(self) -> &'static str {
        match self {
            Action::Post { .. } => "Post",
            Action::Straddle { .. } => "Straddle",
            Action::Fold(_) => "Fold",
            Action::Check(_) => "Check",
            Action::Call { .. } => "Call",
            Action::Bet { .. } => "Bet",
            Action::Raise { .. } => "Raise",
            Action::Flop(_) => "Flop",
            Action::Turn(_) => "Turn",
            Action::River(_) => "River",
            Action::UncalledBet { .. } => "Uncalled bet",
            Action::Shows { .. } => "Shows",
            Action::MucksOrUnknown(_) => "Mucks",
        }
    }

    pub fn player_char(self) -> Option<char> {
        let ch = match self {
            Action::Post { .. } => 'p',
            Action::Straddle { .. } => 's',
            Action::Fold(_) => 'f',
            Action::Check(_) => 'x',
            Action::Call { .. } => 'c',
            Action::Bet { .. } => 'b',
            Action::Raise { .. } => 'r',
            _ => return None,
        };
        Some(ch)
    }

    pub fn is_street(self) -> bool {
        self.street().is_some()
    }

    pub fn street(self) -> Option<Street> {
        match self {
            Action::Flop(_) => Some(Street::Flop),
            Action::Turn(_) => Some(Street::Turn),
            Action::River(_) => Some(Street::River),
            _ => None,
        }
    }

    pub fn is_player(self) -> bool {
        self.player().is_some()
    }

    pub fn player(self) -> Option<Player> {
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
        Some(player)
    }

    pub fn player_all(self) -> Option<Player> {
        let player = match self {
            Action::Post { player, .. } => player,
            Action::Straddle { player, .. } => player,
            Action::Fold(player) => player,
            Action::Check(player) => player,
            Action::Call { player, .. } => player,
            Action::Bet { player, .. } => player,
            Action::Raise { player, .. } => player,
            Action::UncalledBet { player, .. } => player,
            Action::Shows { player, .. } => player,
            Action::MucksOrUnknown(player) => player,
            _ => return None,
        };
        Some(player)
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

    pub const fn previous(self) -> Option<Self> {
        match self {
            Street::PreFlop => None,
            Street::Flop => Some(Street::PreFlop),
            Street::Turn => Some(Street::Flop),
            Street::River => Some(Street::Turn),
        }
    }

    pub const fn next(self) -> Option<Self> {
        match self {
            Street::PreFlop => Some(Street::Flop),
            Street::Flop => Some(Street::Turn),
            Street::Turn => Some(Street::River),
            Street::River => None,
        }
    }

    pub const fn to_usize(self) -> usize {
        self as usize
    }

    pub const fn community_card_count(self) -> usize {
        match self {
            Street::PreFlop => 0,
            Street::Flop => 3,
            Street::Turn => 4,
            Street::River => 5,
        }
    }

    pub const fn new_community_card_count(self) -> usize {
        match self {
            Street::PreFlop => 0,
            Street::Flop => 3,
            Street::Turn => 1,
            Street::River => 1,
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
    pub const EMPTY: Self = Self {
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

    pub fn cards_set(&self) -> Cards {
        Cards::from_slice(self.cards()).unwrap()
    }

    pub fn street(&self) -> Street {
        self.street
    }

    pub fn flop(&self) -> Option<[Card; 3]> {
        if self.street >= Street::Flop {
            let cards = &self.cards()[..Street::Flop.community_card_count()];
            Some(cards.try_into().unwrap())
        } else {
            None
        }
    }

    pub fn turn(&self) -> Option<Card> {
        self.cards().get(3).copied()
    }

    pub fn river(&self) -> Option<Card> {
        self.cards().get(4).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Post,
    Player(Player),
    Street(Street),
    UncalledBet { player: Player, amount: Amount },
    ShowOrMuck(Player),
    ShowdownOrNextRunout,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    player_count: u8,
    button: Player,
    small_blind: Amount,
    big_blind: Amount,
    starting_stacks: [Amount; Self::MAX_PLAYERS],
    names: [Option<Arc<String>>; Self::MAX_PLAYERS],
    seats: [Seat; Self::MAX_PLAYERS],

    unit: Option<Arc<String>>,
    max_players: Option<NonZeroU8>,
    location: Option<Arc<String>>,
    date: Option<NaiveDateTime>,
    table_name: Option<Arc<String>>,
    hand_name: Option<Arc<String>>,
    /// Contains json data. Not using serde_json RawValue,
    /// because it does not implement Eq.
    /// Not using serde_json Value, because it is not compact
    /// enough here.
    additional_metadata: HashMap<Arc<str>, String>,
    /// Set to Player::UNSET if no hero is set.
    hero: Player,
    hands: [Hand; Self::MAX_PLAYERS],

    actions: Vec<Action>,
    boards: [Board; Self::MAX_RUNOUTS],
    current_board: u8,
    reference_stacks: [Amount; Self::MAX_PLAYERS],
    stacks_in_street: [[Amount; Self::MAX_PLAYERS]; Street::COUNT],
    showdown_stacks: [Amount; Self::MAX_PLAYERS],
    current_street_index: usize,
    current_action_index: usize,
    /// Set to u8::MAX if no current player is set.
    current_player: Player,
    not_folded: Bitset<2>,
    /// Using Hand::UNDEFINED if a hand is not known.
    hand_shown: Bitset<2>,
    hand_mucked: Bitset<2>,
    at_end: bool,
    in_next: bool,
}

impl Game {
    pub const MAX_RUNOUTS: usize = 4;

    pub const MIN_PLAYERS: usize = 2;
    // TODO: Check that nothing is broken with 10 players.
    pub const MAX_PLAYERS: usize = 10;

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
        &[
            ("BTN", "Button"),
            ("SB", "Small Blind"),
            ("BB", "Big Blind"),
            ("UTG", "Under the Gun"),
            ("UTG+1", "Under the Gun +1"),
            ("UTG+2", "Under the Gun +2"),
            ("UTG+3", "Under the Gun +3"),
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
        button: Player,
        player: Player,
    ) -> Option<(&'static str, &'static str)> {
        let names = Self::position_names(player_count)?;
        if usize::from(button) >= player_count || usize::from(player) >= player_count {
            None
        } else {
            let index = (player_count - usize::from(button) + usize::from(player)) % player_count;
            Some(names[index])
        }
    }

    pub fn player_to_button_offset(
        player_count: usize,
        button: Player,
        player: Player,
    ) -> Option<Player> {
        if player_count < Self::MIN_PLAYERS
            || player_count > Self::MAX_PLAYERS
            || usize::from(button) >= player_count
            || usize::from(player) >= player_count
        {
            None
        } else {
            let index = (player_count - usize::from(button) + usize::from(player)) % player_count;
            Some(Player::try_from(index).unwrap())
        }
    }

    pub fn new(
        players: &[PlayerData],
        button: Player,
        small_blind: Amount,
        big_blind: Amount,
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
        if usize::from(button) >= player_count {
            return Err("invalid button position".into());
        }

        let mut stacks = [Amount::ZERO; Self::MAX_PLAYERS];
        let mut names: [Option<Arc<String>>; Self::MAX_PLAYERS] =
            [const { None }; Self::MAX_PLAYERS];
        let mut hands = [Hand::UNDEFINED; Self::MAX_PLAYERS];
        for (index, player) in players.iter().enumerate() {
            stacks[index] = player.starting_stack;
            names[index] = player.name.clone();
            hands[index] = player.hand.unwrap_or(Hand::UNDEFINED)
        }

        if stacks
            .iter()
            .take(player_count)
            .any(|stack| *stack == Amount::ZERO)
        {
            return Err("empty stacks not allowed in hand".into());
        }
        let total_stacks = stacks
            .iter()
            .take(player_count)
            .copied()
            .fold(Some(0u32), |acc, n| {
                acc.and_then(|acc| acc.checked_add(n.into()))
            });
        if total_stacks.is_none() {
            return Err("total stacks overflows".into());
        }

        let unique_names = names
            .iter()
            .take(player_count)
            .enumerate()
            .map(|(player, name)| {
                name.as_ref().map(|name| name.as_str()).unwrap_or_else(|| {
                    Self::position_name(player_count, button, Player::try_from(player).unwrap())
                        .unwrap()
                        .0
                })
            })
            .collect::<HashSet<_>>()
            .len();
        if unique_names != player_count {
            return Err("duplicate player name".into());
        }
        if names
            .iter()
            .take(player_count)
            .any(|name| name.as_ref().is_some_and(|n| n.is_empty()))
        {
            return Err("empty player name".into());
        }

        let seats = {
            let mut seats = [Seat::ZERO; Self::MAX_PLAYERS];
            let mut seat_count = 0;
            for (index, player) in players.iter().enumerate() {
                if let Some(seat) = player.seat {
                    seats[index] = seat;
                    seat_count += 1;
                } else {
                    seats[index] = Seat::try_from(index).unwrap();
                }
            }

            if seat_count == 0 {
                seats
            } else if seat_count != player_count {
                return Err("all players need a seat config (or none)".into());
            } else if seats
                .iter()
                .take(player_count)
                .any(|seat| usize::from(*seat) >= Self::MAX_PLAYERS)
            {
                return Err(
                    "all player seat configs must be smaller than the max amount of players".into(),
                );
            } else if seats
                .iter()
                .take(player_count)
                .collect::<HashSet<_>>()
                .len()
                != player_count
            {
                return Err("duplicate player seat config".into());
            } else {
                seats
            }
        };

        let game = Self {
            location: None,
            date: None,
            table_name: None,
            hand_name: None,
            max_players: None,
            unit: None,
            additional_metadata: HashMap::new(),
            hero: Player::UNSET,
            names,
            seats,
            actions: Vec::new(),
            player_count: u8::try_from(player_count).unwrap(),
            starting_stacks: stacks.clone(),
            reference_stacks: stacks.clone(),
            stacks_in_street: array::from_fn(|_| stacks.clone()),
            showdown_stacks: [Amount::ZERO; Self::MAX_PLAYERS],
            boards: [Board::EMPTY; Self::MAX_RUNOUTS],
            current_board: 0,
            button,
            current_street_index: 0,
            current_action_index: 0,
            current_player: Player::UNSET,
            not_folded: Bitset::ones(player_count),
            small_blind,
            big_blind,
            hands,
            hand_shown: Bitset::EMPTY,
            hand_mucked: Bitset::EMPTY,
            at_end: false,
            in_next: false,
        };
        game.check_cards()?;
        Ok(game)
    }

    pub fn from_game_data(data: &GameData) -> Result<Game> {
        let mut game = Self::new(
            &data.players,
            data.button_index,
            data.small_blind,
            data.big_blind,
        )?;

        if let Some(unit) = data.unit.clone() {
            game.set_unit(unit);
        }
        if let Some(max_players) = data.max_players {
            game.set_max_players(usize::from(max_players))?;
        }

        if let Some(location) = data.location.clone() {
            game.set_location(location);
        }
        if let Some(date) = data.date {
            game.set_date(date);
        }
        if let Some(table_name) = data.table_name.clone() {
            game.set_table_name(table_name);
        }
        if let Some(hand_name) = data.hand_name.clone() {
            game.set_hand_name(hand_name);
        }
        game.additional_metadata = data
            .additional_metadata
            .iter()
            // Converting a serde_json::Value to String should never fail.
            .map(|(key, value)| (key.clone(), serde_json::to_string(value).unwrap()))
            .collect();
        if let Some(hero) = data.hero_index {
            game.set_hero(hero)?;
        }

        if !data.actions.is_empty() {
            game.post_small_and_big_blind()?;

            let mut current_action_index = 2;
            while current_action_index < data.actions.len() {
                let Action::Post {
                    player,
                    amount,
                    dead,
                } = data.actions[current_action_index]
                else {
                    break;
                };
                game.additional_post(player, amount, dead)?;
                current_action_index += 1;
            }

            for action in data.actions[current_action_index..].iter().copied() {
                game.apply_action(action)?;
            }
        }

        if &game.actions != &data.actions {
            let bad_actions = game
                .actions
                .iter()
                .zip(&data.actions)
                .filter(|(a, b)| a != b)
                .map(|(a, b)| format!("game: {a:?}\ndata: {b:?}\n"))
                .collect::<String>();
            return Err(format!("from game data: actions don't match\n\n{bad_actions}").into());
        }

        if let Some(showdown_stacks) = &data.showdown_stacks {
            game.showdown_stacks(&showdown_stacks)?;
        }
        Ok(game)
    }

    pub fn to_game_data(&self) -> GameData {
        let seats = self.seats.iter().take(self.player_count()).copied();

        let hands = self
            .hands
            .iter()
            .take(self.player_count())
            .copied()
            .map(|hand| {
                if hand == Hand::UNDEFINED {
                    None
                } else {
                    Some(hand)
                }
            });

        let stacks = self
            .starting_stacks
            .iter()
            .take(self.player_count())
            .copied();

        let players = self
            .names
            .iter()
            .take(self.player_count())
            .zip(seats)
            .zip(hands)
            .zip(stacks)
            .map(|(((name, seat), hand), stack)| PlayerData {
                name: name.clone(),
                seat: Some(seat),
                hand,
                starting_stack: stack,
            })
            .collect();

        let has_showdown_stacks = self
            .showdown_stacks
            .iter()
            .take(self.player_count())
            .copied()
            .any(|stack| stack != Amount::ZERO);
        let showdown_stacks: Option<Vec<_>> = if has_showdown_stacks {
            let showdown_stacks = self
                .showdown_stacks
                .iter()
                .copied()
                .take(self.player_count())
                .collect();
            Some(showdown_stacks)
        } else {
            None
        };

        let additional_metadata = self
            .additional_metadata
            .iter()
            // Should never fail, because we only allow storing valid json as a value.
            .map(|(key, value)| (key.clone(), serde_json::from_str(&value).unwrap()))
            .collect();

        GameData {
            unit: self.unit(),
            max_players: self.max_players().map(|n| u8::try_from(n).unwrap()),
            location: self.location(),
            table_name: self.table_name(),
            hand_name: self.hand_name(),
            additional_metadata,
            hero_index: self.hero(),
            date: self.date(),
            players,
            button_index: self.button,
            small_blind: self.small_blind,
            big_blind: self.big_blind,
            actions: self.actions.clone(),
            showdown_stacks,
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
        self.reference_stacks.copy_from_slice(&self.starting_stacks);
        for street_stacks in self.stacks_in_street.iter_mut() {
            street_stacks.copy_from_slice(&self.starting_stacks);
        }
        self.showdown_stacks
            .iter_mut()
            .for_each(|stack| *stack = Amount::ZERO);
        self.boards = [Board::EMPTY; Self::MAX_RUNOUTS];
        self.current_board = 0;
        self.current_street_index = 0;
        self.current_action_index = 0;
        self.current_player = Player::UNSET;
        self.not_folded = Bitset::ones(self.player_count());
        self.hand_shown = Bitset::EMPTY;
        self.hand_mucked = Bitset::EMPTY;
        self.at_end = false;
        self.in_next = false;
    }

    pub fn unit(&self) -> Option<Arc<String>> {
        self.unit.clone()
    }

    pub fn set_unit(&mut self, unit: Arc<String>) {
        self.unit = Some(unit);
    }

    pub fn clear_unit(&mut self) {
        self.unit = None;
    }

    pub fn max_players(&self) -> Option<usize> {
        self.max_players.map(|n| usize::from(n.get()))
    }

    pub fn set_max_players(&mut self, max_players: usize) -> Result<()> {
        if max_players < Self::MIN_PLAYERS
            || max_players > Self::MAX_PLAYERS
            || max_players < self.player_count()
        {
            Err("invalid max players".into())
        } else {
            let max_players = NonZeroU8::try_from(u8::try_from(max_players).unwrap()).unwrap();
            self.max_players = Some(max_players);
            Ok(())
        }
    }

    pub fn clear_max_players(&mut self) {
        self.max_players = None;
    }

    pub fn location(&self) -> Option<Arc<String>> {
        self.location.clone()
    }

    pub fn set_location(&mut self, location: Arc<String>) {
        self.location = Some(location);
    }

    pub fn clear_location(&mut self) {
        self.location = None;
    }

    pub fn date(&self) -> Option<NaiveDateTime> {
        self.date.clone()
    }

    pub fn set_date(&mut self, date: NaiveDateTime) {
        self.date = Some(date);
    }

    pub fn clear_date(&mut self) {
        self.date = None;
    }

    pub fn table_name(&self) -> Option<Arc<String>> {
        self.table_name.clone()
    }

    pub fn set_table_name(&mut self, table_name: Arc<String>) {
        self.table_name = Some(table_name);
    }

    pub fn clear_table_name(&mut self) {
        self.table_name = None;
    }

    pub fn hand_name(&self) -> Option<Arc<String>> {
        self.hand_name.clone()
    }

    pub fn set_hand_name(&mut self, hand_name: Arc<String>) {
        self.hand_name = Some(hand_name);
    }

    pub fn clear_hand_name(&mut self) {
        self.hand_name = None;
    }

    pub fn additional_metadata<'de, T: Deserialize<'de>>(&'de self, key: &str) -> Result<T> {
        let Some(value) = self.additional_metadata.get(key) else {
            return Err(format!("additional metadata: key {key} does not exist").into());
        };
        Ok(serde_json::from_str(&value)?)
    }

    pub fn set_additional_metadata<T: Serialize>(
        &mut self,
        key: Arc<str>,
        value: &T,
    ) -> Result<()> {
        self.additional_metadata
            .insert(key, serde_json::to_string(value)?);
        Ok(())
    }

    pub fn clear_additional_metadata<T: Serialize>(&mut self, key: &str) {
        self.additional_metadata.remove(key);
    }

    pub fn hero(&self) -> Option<Player> {
        if self.hero == Player::UNSET {
            None
        } else {
            Some(self.hero)
        }
    }

    pub fn set_hero(&mut self, hero: Player) -> Result<()> {
        if usize::from(hero) >= self.player_count() {
            Err("hero index greater than player count".into())
        } else {
            self.hero = hero;
            Ok(())
        }
    }

    pub fn clear_hero(&mut self) {
        self.hero = Player::UNSET;
    }

    pub fn player_name(&self, player: Player) -> &str {
        assert!(usize::from(player) < self.player_count());
        match &self.names[usize::from(player)] {
            Some(name) => &name,
            None => {
                Self::position_name(self.player_count(), self.button, player)
                    .unwrap()
                    .0
            }
        }
    }

    pub fn player_by_name(&self, name: &str) -> Option<Player> {
        self.players()
            .map(|player| self.player_name(player))
            .position(|n| n == name)
            .map(|player| Player::try_from(player).unwrap())
    }

    pub fn seat(&self, player: Player) -> Seat {
        assert!(usize::from(player) < self.player_count());
        self.seats[usize::from(player)]
    }

    pub fn players(&self) -> impl Iterator<Item = Player> {
        (0..self.player_count()).map(|index| Player(index as u8))
    }

    pub fn is_heads_up_table(&self) -> bool {
        self.player_count() == Self::MIN_PLAYERS
    }

    pub fn amount_to_milli_big_blinds_rounded(&self, amount: Amount) -> MilliBigBlind {
        amount.to_milli_big_blinds_rounded(self.big_blind)
    }

    pub fn small_blind(&self) -> Amount {
        self.small_blind
    }

    pub fn small_blind_player(&self) -> Player {
        let button_offset = if self.is_heads_up_table() { 0 } else { 1 };
        Player::try_from((usize::from(self.button) + button_offset) % self.player_count()).unwrap()
    }

    pub fn big_blind(&self) -> Amount {
        self.big_blind
    }

    pub fn big_blind_player(&self) -> Player {
        let button_offset = if self.is_heads_up_table() { 1 } else { 2 };
        Player::try_from((usize::from(self.button) + button_offset) % self.player_count()).unwrap()
    }

    fn first_to_act_post_flop(&self) -> Player {
        Player::try_from((usize::from(self.button) + 1) % self.player_count()).unwrap()
    }

    pub fn board(&self) -> Board {
        self.boards[usize::from(self.current_board)]
    }

    fn board_mut(&mut self) -> &mut Board {
        &mut self.boards[usize::from(self.current_board)]
    }

    pub fn starting_stacks(&self) -> &[Amount] {
        &self.starting_stacks[..self.player_count()]
    }

    pub fn current_street_stacks(&self) -> &[Amount] {
        &self.stacks_in_street[self.board().street().to_usize()][..self.player_count()]
    }

    fn current_street_stacks_mut(&mut self) -> &mut [Amount] {
        let player_count = self.player_count();
        &mut self.stacks_in_street[self.board().street().to_usize()][..player_count]
    }

    pub fn previous_street_stacks(&self) -> &[Amount] {
        match self.board().street().previous() {
            Some(street) => &self.stacks_in_street[street.to_usize()][..self.player_count()],
            None => &self.reference_stacks[..self.player_count()],
        }
    }

    pub fn previous_street_stack(&self) -> Option<Amount> {
        self.current_player()
            .map(|player| self.previous_street_stacks()[usize::from(player)])
    }

    pub fn total_pot(&self) -> Amount {
        self.total_invested_per_player()
            .map(u32::from)
            .sum::<u32>()
            .into()
    }

    fn total_invested_per_player(&self) -> impl Iterator<Item = Amount> + '_ {
        self.players().map(|player| self.total_invested(player))
    }

    fn invested_per_player(&self) -> impl Iterator<Item = Amount> + '_ {
        self.players().map(|player| self.invested(player))
    }

    pub fn total_invested(&self, player: Player) -> Amount {
        assert!(usize::from(player) < self.player_count());
        self.starting_stacks[usize::from(player)]
            - self.current_street_stacks()[usize::from(player)]
    }

    pub fn invested(&self, player: Player) -> Amount {
        assert!(usize::from(player) < self.player_count());
        self.reference_stacks[usize::from(player)]
            - self.current_street_stacks()[usize::from(player)]
    }

    pub fn invested_in_street(&self, player: Player) -> Amount {
        assert!(usize::from(player) < self.player_count());
        self.previous_street_stacks()[usize::from(player)]
            - self.current_street_stacks()[usize::from(player)]
    }

    pub fn folded(&self, player: Player) -> bool {
        assert!(usize::from(player) < self.player_count());
        !self.not_folded.has(player.into())
    }

    pub fn players_not_folded(&self) -> impl Iterator<Item = Player> + '_ {
        self.not_folded
            .iter(self.player_count())
            .map(|index| Player::try_from(index).unwrap())
    }

    pub fn in_hand_not_all_in(&self, player: Player) -> bool {
        assert!(usize::from(player) < self.player_count());
        self.not_folded.has(player.into()) && !self.is_all_in(player)
    }

    pub fn actions(&self) -> &[Action] {
        &self.actions[..self.current_action_index]
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

    pub fn can_straddle(&self, player: Player) -> Result<Amount> {
        if usize::from(player) >= self.player_count() {
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
        let min_straddle = min(
            self.reference_stacks[usize::from(player)],
            required_straddle,
        );
        Ok(min_straddle)
    }

    pub fn can_check(&self) -> bool {
        let Some(player) = self.current_player() else {
            return false;
        };
        self.call_amount(player) == Amount::ZERO
    }

    pub fn can_call(&self) -> Option<Amount> {
        let player = self.current_player()?;
        let amount = self.call_amount(player);
        if amount == Amount::ZERO {
            None
        } else {
            Some(min(
                self.current_street_stacks()[usize::from(player)],
                amount,
            ))
        }
    }

    fn call_amount(&self, player: Player) -> Amount {
        self.invested_per_player().max().unwrap() - self.invested(player)
    }

    pub fn can_bet(&self) -> Option<Amount> {
        let player = self.current_player()?;
        let can_bet = self
            .actions_in_street()
            .iter()
            .all(|action| matches!(action, Action::Check(_) | Action::Fold(_)));
        if can_bet {
            Some(min(
                self.current_street_stacks()[usize::from(player)],
                self.big_blind,
            ))
        } else {
            None
        }
    }

    pub fn can_raise(&self) -> Option<(Amount, Amount)> {
        // TODO: Should raise be allowed after all other players are all in?

        let player = self.current_player()?;
        let actions = self.actions_in_street();
        let mut last_amount = Amount::ZERO;
        let mut last_to = Amount::ZERO;
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

        if last_amount == Amount::ZERO {
            match self.board().street() {
                Street::PreFlop => {
                    last_amount = actions
                        .iter()
                        .copied()
                        .filter_map(|action| match action {
                            Action::Straddle { amount, .. } => Some(amount),
                            Action::Post { amount, dead, .. } if !dead => Some(amount),
                            _ => None,
                        })
                        .max()
                        .unwrap();
                    last_to = last_amount;
                }
                _ => return None,
            }
        }
        assert_ne!(last_to, Amount::ZERO);
        if last_amount < self.big_blind {
            last_amount = self.big_blind;
        }

        let call_amount = self.call_amount(player);
        let old_stack = self.previous_street_stacks()[usize::from(player)];
        let current_stack = self.current_street_stacks()[usize::from(player)];
        let to = last_to + last_amount;
        if call_amount >= current_stack {
            None
        } else if to > old_stack {
            let amount = old_stack.checked_sub(last_to).unwrap();
            Some((amount, old_stack))
        } else {
            Some((last_amount, to))
        }
    }

    pub fn can_all_in(&self) -> Option<Amount> {
        let player = self.current_player()?;

        if self.can_bet().is_some() || self.can_raise().is_some() {
            let max_amount = self.previous_street_stacks()[usize::from(player)];
            Some(max_amount)
        } else {
            None
        }
    }

    fn is_all_in(&self, player: Player) -> bool {
        self.current_street_stacks()[usize::from(player)] == Amount::ZERO
    }

    fn all_in_count(&self) -> usize {
        self.players_not_folded()
            .filter(|player| self.is_all_in(*player))
            .count()
    }

    fn action_ended(&self) -> bool {
        self.not_folded.count() == 1
            || (self.current_player().is_none() && self.board().street() == Street::River)
            || (self.current_player().is_none() && self.all_in_terminated_hand())
    }

    fn all_in_terminated_hand(&self) -> bool {
        self.not_folded.count() - 1 <= u32::try_from(self.all_in_count()).unwrap()
    }

    pub fn current_stack(&self) -> Option<Amount> {
        self.current_player()
            .map(|player| self.current_street_stacks()[usize::from(player)])
    }

    pub fn current_stacks(&self) -> &[Amount] {
        match self.state() {
            State::End => &self.showdown_stacks[..self.player_count()],
            _ => self.current_street_stacks(),
        }
    }

    pub fn button(&self) -> Player {
        self.button
    }

    pub fn player_count(&self) -> usize {
        usize::from(self.player_count)
    }

    fn can_uncalled_bet(&self) -> Option<(Player, Amount)> {
        if !self.action_ended() {
            return None;
        }
        let mut player_by_investment_array: [Player; Self::MAX_PLAYERS] =
            array::from_fn(|index| Player::try_from(index).unwrap());
        let player_by_investment = &mut player_by_investment_array[..self.player_count()];
        player_by_investment.sort_by_key(|player| self.invested(*player));
        let max_invested_player = player_by_investment[player_by_investment.len() - 1];
        let second_max_invested_player = player_by_investment[player_by_investment.len() - 2];
        let max_invested = self.invested(max_invested_player);
        let second_max_invested = self.invested(second_max_invested_player);
        if max_invested == second_max_invested {
            None
        } else {
            Some((max_invested_player, max_invested - second_max_invested))
        }
    }

    fn next_show_or_muck(&self) -> Option<Player> {
        let hands_shown_or_mucked = self.hand_shown.count() + self.hand_mucked.count();

        let not_allowed = !self.action_ended()
            || self.not_folded.count() == 1
            || hands_shown_or_mucked == self.not_folded.count();

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

        self.players()
            .skip(start_index.into())
            .chain(self.players().take(start_index.into()))
            .filter(|player| !self.folded(*player))
            .filter(|player| !self.hand_shown.has(usize::from(*player)))
            .filter(|player| !self.hand_mucked.has(usize::from(*player)))
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
            State::ShowdownOrNextRunout
        }
    }

    pub fn current_player(&self) -> Option<Player> {
        if self.current_player == Player::UNSET {
            None
        } else {
            Some(self.current_player)
        }
    }

    fn current_player_result(&self) -> Result<Player> {
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

        let not_folded_not_all_in = self
            .players_not_folded()
            .filter(|player| !self.is_all_in(*player))
            .fold(Bitset::<2>::EMPTY, |set, player| {
                set.with(usize::from(player))
            });

        let players_with_action = actions
            .iter()
            .filter(|action| !matches!(action, Action::Post { .. } | Action::Straddle { .. }))
            .filter_map(|action| action.player())
            .fold(Bitset::<2>::EMPTY, |set, player| {
                set.with(usize::from(player))
            });

        let invested = self.invested_per_player().max().unwrap();
        let all_equal_investments = not_folded_not_all_in
            .iter(self.player_count())
            .map(|player| self.invested(Player::try_from(player).unwrap()))
            .all(|n| n == invested);

        let can_skip = (not_folded_not_all_in.count() == 1
            || not_folded_not_all_in & players_with_action == not_folded_not_all_in)
            && all_equal_investments;
        if can_skip {
            self.current_player = Player::UNSET;
            return;
        }

        self.next_player_in_hand_not_all_in();
    }

    fn next_player_in_hand_not_all_in(&mut self) {
        assert!(self.current_player().is_some());
        let current_player_start = self.current_player;
        loop {
            self.current_player =
                Player::try_from((u8::from(self.current_player) + 1) % self.player_count).unwrap();
            if self.current_player == current_player_start {
                self.current_player = Player::UNSET;
                return;
            }
            if self.in_hand_not_all_in(self.current_player) {
                return;
            }
        }
    }

    fn players_not_folded_not_all_in(&self) -> usize {
        self.players()
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

    fn update_stack(&mut self, amount: Amount) -> Result<()> {
        let player = self.current_player_result()?;
        if amount > self.current_street_stacks()[usize::from(player)] {
            return Err("player cannot afford sizing".into());
        }
        self.current_street_stacks_mut()[usize::from(player)] -= amount;
        Ok(())
    }

    fn action_post_simple(&mut self, amount: Amount) -> Result<()> {
        let player = self.current_player_result()?;
        let amount = min(self.current_street_stacks()[usize::from(player)], amount);
        self.update_stack(amount)?;
        self.add_action(Action::Post {
            player: self.current_player,
            amount,
            dead: false,
        });
        self.next_player();
        Ok(())
    }

    pub fn post_small_and_big_blind(&mut self) -> Result<()> {
        self.check_pre_update()?;
        if !self.at_start() {
            return Err("can only post small and big blind before other actions".into());
        }
        self.current_player = self.button;
        if !self.is_heads_up_table() {
            self.current_player =
                Player::try_from((u8::from(self.current_player) + 1) % self.player_count).unwrap();
        }
        self.action_post_simple(self.small_blind)?;
        self.action_post_simple(self.big_blind)?;
        Ok(())
    }

    pub fn additional_post(&mut self, player: Player, amount: Amount, dead: bool) -> Result<()> {
        self.check_pre_update()?;
        if usize::from(player) >= self.player_count() {
            return Err("additional post: invalid player index".into());
        }
        if amount == Amount::ZERO {
            return Err("additional post: cannot post an amount of zero".into());
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

        let players = (usize::from(self.small_blind_player())..self.player_count())
            .chain(0..usize::from(self.small_blind_player()));
        for current_player in players.rev() {
            if current_player == usize::from(player) {
                break;
            }
            if poster.has(current_player) {
                return Err(
                    "additional post: all players posted already or post not in order".into(),
                );
            }
        }

        let last_action = self.actions[self.current_action_index.checked_sub(1).unwrap()];
        let Action::Post {
            player: last_player,
            amount: last_amount,
            dead: last_dead,
        } = last_action
        else {
            unreachable!();
        };

        if last_player == player {
            if dead && !last_dead {
                return Err(
                    "additional post: dead posts must appear before all other posts".into(),
                );
            }
            if last_dead == dead && last_amount > amount {
                return Err("additional post: amounts for single player must be ordered".into());
            }
        }

        let current_stack = self.stacks_in_street[Street::PreFlop.to_usize()][usize::from(player)];
        if amount > current_stack {
            return Err("additional post: player cannot afford post".into());
        }

        self.stacks_in_street[Street::PreFlop.to_usize()][usize::from(player)] -= amount;
        if dead {
            self.reference_stacks[usize::from(player)] -= amount;
        }

        self.add_action(Action::Post {
            player,
            amount,
            dead,
        });

        Ok(())
    }

    pub fn straddle(&mut self, player: Player, amount: Amount) -> Result<()> {
        self.check_pre_update()?;
        let required_straddle = self.can_straddle(player)?;
        if amount < required_straddle {
            return Err("straddle: amount too small".into());
        }
        if amount > self.reference_stacks[usize::from(player)] {
            return Err("straddle: player cannot afford amount".into());
        }

        self.current_street_stacks_mut()[usize::from(player)] =
            self.reference_stacks[usize::from(player)] - amount;
        self.add_action(Action::Straddle { player, amount });
        // Always start left of the last straddler.
        self.current_player = Player::try_from((u8::from(player) + 1) % self.player_count).unwrap();
        Ok(())
    }

    pub fn fold(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        assert!(self.not_folded.has(usize::from(player)));
        self.not_folded.remove(usize::from(player));
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

    pub fn bet(&mut self, amount: Amount) -> Result<()> {
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

    pub fn raise(&mut self, to: Amount) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        let Some((min_amount, min_to)) = self.can_raise() else {
            return Err("player is not allowed to raise".into());
        };

        if to < min_to {
            return Err("raise is smaller than the minimum".into());
        }

        let previous_street_stack = self.previous_street_stacks()[usize::from(player)];
        if to > previous_street_stack {
            return Err("player cannot afford raise".into());
        }

        let Some(amount) = min_amount.checked_add(to - min_to) else {
            return Err("calculating raise amount overflowed".into());
        };

        let old_stack = self.current_street_stacks()[usize::from(player)];
        self.current_street_stacks_mut()[usize::from(player)] = previous_street_stack - to;

        self.add_action(Action::Raise {
            player: self.current_player,
            old_stack,
            amount,
            to,
        });
        self.next_player();
        Ok(())
    }

    pub fn unsafe_raise_min_bet_unchecked(&mut self, to: Amount) -> Result<()> {
        self.check_pre_update()?;
        let player = self.current_player_result()?;
        let Some((min_amount, min_to)) = self.can_raise() else {
            return Err("player is not allowed to raise".into());
        };

        let previous_street_stack = self.previous_street_stacks()[usize::from(player)];
        if to > previous_street_stack {
            return Err("player cannot afford raise".into());
        }

        let Some(amount) = min_amount
            .checked_add(to)
            .and_then(|n| n.checked_sub(min_to))
        else {
            return Err("calculating raise amount overflowed".into());
        };

        let old_stack = self.current_street_stacks()[usize::from(player)];
        self.current_street_stacks_mut()[usize::from(player)] = previous_street_stack - to;

        self.add_action(Action::Raise {
            player: self.current_player,
            old_stack,
            amount,
            to,
        });
        self.next_player();
        Ok(())
    }

    pub fn all_in(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let Some(amount) = self.can_all_in() else {
            return Err("all in: no current player or player not allowed to bet or raise".into());
        };

        if self.can_bet().is_some() {
            self.bet(amount)
        } else if self.can_raise().is_some() {
            self.raise(amount)
        } else {
            unreachable!()
        }
    }

    pub fn uncalled_bet(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let State::UncalledBet { player, amount } = self.state() else {
            return Err("uncalled bet: cannot return uncalled bet in current state".into());
        };
        self.current_street_stacks_mut()[usize::from(player)] += amount;
        self.add_action(Action::UncalledBet { player, amount });
        Ok(())
    }

    fn can_next_street(&self) -> Option<Street> {
        let allowed = self.next_show_or_muck().is_none()
            && self.current_player().is_none()
            && (!self.actions_in_street().is_empty() || self.players_not_folded_not_all_in() <= 1)
            && self.not_folded.count() > 1
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
            State::ShowdownOrNextRunout => {
                if usize::from(self.current_board) >= Self::MAX_RUNOUTS - 1 {
                    None
                } else {
                    self.multiple_runouts_starting_street()
                        .map(|street| (street, true))
                }
            }
            _ => None,
        }
    }

    fn multiple_runouts_starting_street(&self) -> Option<Street> {
        assert_eq!(self.state(), State::ShowdownOrNextRunout);
        if !self.all_in_terminated_hand() {
            return None;
        }

        for (index, action) in self.actions.iter().copied().enumerate() {
            let Some(street) = action.street() else {
                continue;
            };

            let next_action = self.actions.get(index.checked_add(1).unwrap()).copied();
            match next_action {
                Some(next_action) if next_action.is_street() => return Some(street),
                None => {
                    assert_eq!(street, Street::River);
                    return Some(street);
                }
                _ => (),
            }
        }

        None
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
            .take(self.player_count())
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
            self.current_player = Player::UNSET;
        } else {
            self.current_player = self.button;
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

    pub fn runouts(&self) -> &[Board] {
        &self.boards[..usize::from(self.current_board + 1)]
    }

    pub fn showdown_custom(
        &mut self,
        total_rake: Amount,
        player_pot_share: impl Iterator<Item = (Player, Amount)>,
    ) -> Result<()> {
        self.check_pre_update()?;
        if self.state() != State::ShowdownOrNextRunout {
            return Err("showdown: not in showdown state".into());
        }

        let street = self.board().street();
        self.showdown_stacks
            .copy_from_slice(&self.stacks_in_street[street.to_usize()]);

        let mut total_pot = Amount::ZERO;
        for (player, pot_share) in player_pot_share {
            if usize::from(player) >= self.player_count() {
                return Err("showdown: invalid player index".into());
            }

            // We cannot really check if the winners are correct,
            // only if they have folded or not.
            if pot_share > Amount::ZERO && self.folded(player) {
                return Err("showdown: player who folded won part of the pot".into());
            }

            let Some(new_total_pot) = total_pot.checked_add(pot_share) else {
                return Err("showdown: amount won too large".into());
            };

            let Some(new_stack) = self.showdown_stacks[usize::from(player)].checked_add(pot_share)
            else {
                return Err("showdown: amount won too large".into());
            };
            total_pot = new_total_pot;
            self.showdown_stacks[usize::from(player)] = new_stack;
        }

        if total_pot.checked_add(total_rake) != Some(self.total_pot()) {
            return Err(format!(
                "{}: total_winnings={} total_rake={} total_pot={}",
                "showdown: total pot and supplied pot shares with rake don't match",
                total_pot,
                total_rake,
                self.total_pot(),
            )
            .into());
        }
        self.at_end = true;
        Ok(())
    }

    pub fn showdown_stacks(&mut self, stacks: &[Amount]) -> Result<()> {
        if stacks.len() != self.player_count() {
            return Err("showdown stacks: given stack count does not match player count".into());
        }

        let total = stacks
            .iter()
            .copied()
            .fold(Some(Amount::ZERO), |total, stack| {
                total.and_then(|total| total.checked_add(stack))
            });

        let Some(total) = total else {
            return Err("showdown stacks: stack sum overflows".into());
        };

        let expected_total = self
            .starting_stacks
            .iter()
            .take(self.player_count())
            .copied()
            .map(u32::from)
            .sum();

        if u32::from(total) > expected_total {
            return Err("showdown stacks: showdown stacks are larger than starting stacks".into());
        }

        let stacks_iter = stacks.iter().copied().zip(
            self.starting_stacks
                .iter()
                .copied()
                .take(self.player_count()),
        );

        for (player, (new_stack, starting_stack)) in self.players().zip(stacks_iter) {
            // We cannot really check if the winners are correct,
            // only if they have folded or not.
            if new_stack > starting_stack && self.folded(player) {
                return Err("showdown: player who folded won part of the pot".into());
            }
        }

        self.check_pre_update()?;
        if self.state() != State::ShowdownOrNextRunout {
            return Err("showdown stacks: not in showdown state".into());
        }
        let player_count = self.player_count();
        self.showdown_stacks[..player_count].copy_from_slice(stacks);
        self.at_end = true;
        Ok(())
    }

    pub fn showdown_simple(&mut self) -> Result<()> {
        // TODO: Custom rake.

        self.check_pre_update()?;
        if self.state() != State::ShowdownOrNextRunout {
            return Err("showdown: not in showdown state".into());
        }

        let street = self.board().street();
        self.showdown_stacks
            .copy_from_slice(&self.stacks_in_street[street.to_usize()]);

        for (pot, winners) in self.showdown_winners_by_pot()? {
            let winner_count = winners.count();

            let won_per_player = Amount::from(u32::from(pot) / winner_count);
            for player in winners.iter(self.player_count()) {
                assert!(player < self.player_count());
                self.showdown_stacks[player] += won_per_player;
            }

            let n = usize::try_from(u32::from(pot) % winner_count).unwrap();
            let extra_chip_players = (usize::from(self.small_blind_player())..self.player_count())
                .chain(0..usize::from(self.small_blind_player()))
                .filter(|player| winners.has(*player))
                .take(n);
            for player in extra_chip_players {
                assert!(player < self.player_count());
                self.showdown_stacks[player] += 1.into();
            }
        }

        self.at_end = true;
        Ok(())
    }

    pub fn showdown_winners_by_pot(&self) -> Result<Vec<(Amount, Bitset<2>)>> {
        if !matches!(
            self.state(),
            State::ShowOrMuck(_) | State::ShowdownOrNextRunout | State::End
        ) {
            return Err("showdown: not in show or muck, showdown, or end state".into());
        }

        if self.not_folded.count() == 1 {
            return Ok(vec![(self.total_pot(), self.not_folded)]);
        }

        if self.not_folded & self.hand_mucked == self.not_folded {
            return Err("showdown: all players mucked in multi-way showdown".into());
        }

        for player in self.players_not_folded() {
            if self.hand_mucked(player) || !self.hand_shown(player) {
                continue;
            }
            if self.get_hand(player).is_none() {
                return Err(format!("showdown: missing hand for player {player}").into());
            }
        }

        let mut investments_array = [Amount::ZERO; Self::MAX_PLAYERS];
        let investments = &mut investments_array[..self.player_count()];
        for player in self.players() {
            investments[usize::from(player)] = self.invested(player);
        }
        let pots = self.showdown_pots(investments);
        // We can only have max players minus one pots, so it's ok to unwrap.
        let pots_count = pots
            .iter()
            .copied()
            .position(|(pot, _)| pot == Amount::ZERO)
            .unwrap();
        assert_ne!(pots_count, 0);

        let mut scores_array = [Score::ZERO; Self::MAX_PLAYERS];
        let scores = &mut scores_array[..self.player_count()];
        let winners_by_pot = self.showdown_winners(&pots[..pots_count], scores);

        assert_eq!(
            winners_by_pot
                .iter()
                .map(|(pot, _)| u32::from(*pot))
                .sum::<u32>(),
            u32::from(self.total_pot())
        );
        Ok(winners_by_pot)
    }

    fn showdown_pots(
        &self,
        investments: &mut [Amount],
    ) -> [(Amount, Bitset<2>); Self::MAX_PLAYERS] {
        let mut dead_money: u32 = self
            .starting_stacks
            .iter()
            .copied()
            .zip(self.reference_stacks.iter().copied())
            .map(|(start, reference)| u32::from(start.checked_sub(reference).unwrap()))
            .sum();

        let mut out = [(Amount::ZERO, Bitset::EMPTY); Self::MAX_PLAYERS];
        for index in 0..Self::MAX_PLAYERS {
            let eligible_players = self
                .players()
                .filter(|player| !self.folded(*player))
                .filter(|player| investments[usize::from(*player)] > Amount::ZERO)
                .fold(Bitset::<2>::EMPTY, |s, p| s.with(p.into()));

            let min_investment = eligible_players
                .iter(self.player_count())
                .map(|player| investments[player])
                .min();
            let Some(min_investment) = min_investment else {
                return out;
            };

            let mut pot = Amount::ZERO;
            for investment in investments.iter_mut() {
                pot += min_investment - min_investment.saturating_sub(*investment);
                *investment = investment.saturating_sub(min_investment);
            }

            // Add the dead money to the main pot.
            pot += dead_money.into();
            dead_money = 0;

            out[index] = (pot, eligible_players);
        }

        unreachable!()
    }

    fn showdown_winners(
        &self,
        pots: &[(Amount, Bitset<2>)],
        scores: &mut [Score],
    ) -> Vec<(Amount, Bitset<2>)> {
        let runouts = self.runouts();
        let runout_count = u32::try_from(runouts.len()).unwrap();
        let mut winners_by_pot = Vec::new();

        let show_or_muck_player = if let State::ShowOrMuck(player) = self.state() {
            Some(player)
        } else {
            None
        };

        for (runout_index, board) in runouts.iter().enumerate() {
            let board = Cards::from_slice(board.cards()).unwrap();

            for player in self.players_not_folded() {
                if self.hand_mucked(player) {
                    continue;
                }
                if !self.hand_shown(player) && Some(player) != show_or_muck_player {
                    continue;
                }

                let hand = self.get_hand(player).unwrap();
                scores[usize::from(player)] = board.with(hand.high()).with(hand.low()).score_fast();
            }

            for (pot_per_investment, eligible_players) in pots.iter().copied() {
                let winners = self.showdown_winners_single(eligible_players, scores);
                let pot_per_runout = u32::from(pot_per_investment) / runout_count;
                let pot = if runout_index == 0 {
                    pot_per_runout + u32::from(pot_per_investment) % runout_count
                } else {
                    pot_per_runout
                };
                winners_by_pot.push((pot.into(), winners));
            }
        }

        winners_by_pot
    }

    fn showdown_winners_single(&self, eligible_players: Bitset<2>, scores: &[Score]) -> Bitset<2> {
        let max_score = eligible_players
            .iter(self.player_count())
            .map(|player| scores[usize::from(player)])
            .max()
            .unwrap();

        eligible_players
            .iter(self.player_count())
            .filter(|player| scores[usize::from(*player)] == max_score)
            .fold(Bitset::<2>::EMPTY, |set, player| set.with(player))
    }

    pub fn draw_unset_hands(&mut self, rng: &mut impl Rng) {
        let mut deck = Deck::from_cards(rng, self.known_cards());
        let player_count = self.player_count();
        for hand in self.hands.iter_mut().take(player_count) {
            if *hand != Hand::UNDEFINED {
                continue;
            }
            *hand = deck.hand(rng).unwrap();
        }
        self.check_cards().unwrap();
    }

    pub fn set_hand(&mut self, player: Player, hand: Hand) -> Result<()> {
        if usize::from(player) >= self.player_count() {
            return Err(format!("set hand: unknown player index {player}").into());
        }
        if self.hands[usize::from(player)] != Hand::UNDEFINED
            && self.hands[usize::from(player)] != hand
        {
            return Err(
                format!("set hand: cannot set different hand for player index {player}").into(),
            );
        }
        self.hands[usize::from(player)] = hand;
        self.check_cards()?;
        Ok(())
    }

    pub fn hand_shown(&self, player: Player) -> bool {
        assert!(usize::from(player) < self.player_count());
        self.hand_shown.has(player.into())
    }

    pub fn show_hand(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let State::ShowOrMuck(player) = self.state() else {
            return Err("show: cannot show hand in current state".into());
        };
        assert!(usize::from(player) < self.player_count());
        assert!(!self.hand_shown.has(usize::from(player)));
        assert!(!self.hand_mucked.has(usize::from(player)));
        if self.hands[usize::from(player)] == Hand::UNDEFINED {
            return Err(format!("show: hand for player index {player} not set").into());
        }
        self.hand_shown.set(usize::from(player));
        self.add_action(Action::Shows {
            player,
            hand: self.hands[usize::from(player)],
        });
        Ok(())
    }

    pub fn hand_mucked(&self, player: Player) -> bool {
        assert!(usize::from(player) < self.player_count());
        self.hand_mucked.has(player.into())
    }

    pub fn muck_hand(&mut self) -> Result<()> {
        self.check_pre_update()?;
        let State::ShowOrMuck(player) = self.state() else {
            return Err("muck: cannot muck hand in current state".into());
        };
        assert!(usize::from(player) < self.player_count());
        assert!(!self.hand_shown.has(usize::from(player)));
        assert!(!self.hand_mucked.has(usize::from(player)));
        self.hand_mucked.set(usize::from(player));
        self.add_action(Action::MucksOrUnknown(player));
        Ok(())
    }

    pub fn should_show(&self) -> Result<bool> {
        let State::ShowOrMuck(player) = self.state() else {
            return Err("should show: not in show or muck state".into());
        };

        if self.board().street() != Street::River {
            // Could muck when drawing dead, but keep it simple for now.
            return Ok(true);
        }

        let winners_by_pot = self.showdown_winners_by_pot()?;

        let should_show = winners_by_pot
            .iter()
            .any(|(_, winners)| winners.has(player.into()));

        Ok(should_show)
    }

    pub fn get_hand(&self, player: Player) -> Option<Hand> {
        if usize::from(player) >= self.player_count()
            || self.hands[usize::from(player)] == Hand::UNDEFINED
        {
            None
        } else {
            Some(self.hands[usize::from(player)])
        }
    }

    pub fn current_hand(&self) -> Option<Hand> {
        self.current_player()
            .and_then(|player| self.get_hand(player))
    }

    pub fn apply_action(&mut self, action: Action) -> Result<()> {
        self.check_pre_update()?;
        if !matches!(action, Action::Straddle { .. }) && action.player() != self.current_player() {
            return Err("apply action: current player and action don't match".into());
        }
        match action {
            Action::Post { .. } => Err("apply action: cannot apply post".into()),
            Action::Straddle { player, amount } => self.straddle(player, amount),
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
                let expected_state = State::UncalledBet { player, amount };
                if self.state() != expected_state {
                    return Err("apply action: uncalled bet not allowed or invalid".into());
                }
                self.uncalled_bet()
            }
            Action::Shows { player, hand } => {
                if self.state() != State::ShowOrMuck(player) || self.get_hand(player) != Some(hand)
                {
                    return Err("apply action: show not allowed or invalid".into());
                }
                self.show_hand()
            }
            Action::MucksOrUnknown(player) => {
                if self.state() != State::ShowOrMuck(player) {
                    return Err("apply action: muck or unknown hand not allowed or invalid".into());
                }
                self.muck_hand()
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
                self.reference_stacks.copy_from_slice(&self.starting_stacks);
                self.stacks_in_street[Street::PreFlop.to_usize()]
                    .copy_from_slice(&self.starting_stacks);
                self.current_player = Player::UNSET;
                self.current_action_index = 0;
            }
            Action::Fold(player) => {
                self.current_player = player;
                self.not_folded.set(usize::from(player));
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
                assert!(self.hand_shown.has(usize::from(player)));
                self.hand_shown.remove(usize::from(player));
            }
            Action::MucksOrUnknown(player) => {
                assert!(self.hand_mucked.has(usize::from(player)));
                self.hand_mucked.remove(usize::from(player));
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
        self.current_player = Player::UNSET;
    }

    pub fn undo(&mut self) -> Result<()> {
        self.check_pre_update()?;
        if !self.can_previous() {
            return Err("undo: cannot go to previous action".into());
        }

        match self.state() {
            State::Post => unreachable!(),
            State::End => {
                self.showdown_stacks
                    .iter_mut()
                    .for_each(|stack| *stack = Amount::ZERO);
            }
            _ => (),
        }

        assert!(self.previous());
        self.actions.truncate(self.current_action_index);
        Ok(())
    }

    pub fn can_next(&self) -> bool {
        let showdown_stacks_set = self
            .showdown_stacks
            .iter()
            .take(self.player_count())
            .copied()
            .any(|stack| stack != Amount::ZERO);
        let at_final_action = self.current_action_index == self.actions.len();
        match self.state() {
            State::ShowdownOrNextRunout if showdown_stacks_set => true,
            State::ShowdownOrNextRunout if !at_final_action => true,
            State::ShowdownOrNextRunout => false,
            _ if at_final_action => false,
            _ => true,
        }
    }

    pub fn forward(&mut self) {
        while self.next() {}
    }

    pub fn next(&mut self) -> bool {
        if !self.can_next() {
            return false;
        }
        match self.state() {
            State::ShowdownOrNextRunout if self.current_action_index == self.actions.len() => {
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
            Action::MucksOrUnknown(_) => self.muck_hand(),
        };
        self.in_next = false;
        result.unwrap();
        true
    }

    fn next_posts_straddles(&mut self) -> Result<()> {
        self.post_small_and_big_blind()?;

        while self.current_action_index < self.actions.len() {
            match self.actions[self.current_action_index] {
                Action::Post {
                    player,
                    amount,
                    dead,
                } => self.additional_post(player, amount, dead)?,
                Action::Straddle { player, amount } => {
                    self.straddle(player, amount)?;
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
            assert!(!self.folded(player));
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
                assert_eq!(self.call_amount(player), Amount::ZERO);
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
                Action::Post {
                    player,
                    amount,
                    dead,
                } => {
                    new_game.additional_post(player, amount, dead).unwrap();
                }
                Action::Straddle { player, amount } => {
                    new_game.straddle(player, amount).unwrap();
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
        new_game
            .showdown_stacks(&self.showdown_stacks[..self.player_count()])
            .unwrap();
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
        assert_eq!(expected.player_count, self.player_count);
        assert_eq!(expected.names, self.names);
        assert_eq!(expected.starting_stacks, self.starting_stacks);
        assert_eq!(expected.reference_stacks, self.reference_stacks);
        let current_street = expected.board().street();
        assert_eq!(
            &expected.stacks_in_street[..current_street.to_usize()],
            &self.stacks_in_street[..current_street.to_usize()],
        );
        assert_eq!(expected.button, self.button);
        assert_eq!(expected.current_street_index, self.current_street_index);
        assert_eq!(expected.current_action_index, self.current_action_index);
        assert_eq!(expected.current_player, self.current_player);
        assert_eq!(expected.not_folded, self.not_folded);
        assert_eq!(expected.small_blind, self.small_blind);
        assert_eq!(expected.big_blind, self.big_blind);
        assert_eq!(expected.hands, self.hands);
        assert_eq!(expected.hand_shown, self.hand_shown);
        assert_eq!(expected.hand_mucked, self.hand_mucked);

        assert_eq!(expected.actions_in_street(), self.actions_in_street());
        assert_eq!(expected.state(), self.state());
        assert_eq!(expected.can_check(), self.can_check());
        assert_eq!(expected.can_call(), self.can_call());
        assert_eq!(expected.can_bet(), self.can_bet());
        assert_eq!(expected.can_raise(), self.can_raise());
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerData {
    pub name: Option<Arc<String>>,
    pub seat: Option<Seat>,
    pub hand: Option<Hand>,
    pub starting_stack: Amount,
}

impl PlayerData {
    pub fn with_starting_stack(starting_stack: Amount) -> Self {
        Self {
            name: None,
            seat: None,
            hand: None,
            starting_stack,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameData {
    pub unit: Option<Arc<String>>,
    pub max_players: Option<u8>,
    pub location: Option<Arc<String>>,
    pub date: Option<NaiveDateTime>,
    pub table_name: Option<Arc<String>>,
    pub hand_name: Option<Arc<String>>,
    #[serde(default)]
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub additional_metadata: HashMap<Arc<str>, Value>,
    pub hero_index: Option<Player>,

    pub players: Vec<PlayerData>,
    pub button_index: Player,
    pub small_blind: Amount,
    pub big_blind: Amount,
    pub actions: Vec<Action>,
    pub showdown_stacks: Option<Vec<Amount>>,
}

impl Default for GameData {
    fn default() -> Self {
        Self {
            unit: None,
            max_players: None,
            location: None,
            table_name: None,
            hand_name: None,
            date: None,
            additional_metadata: HashMap::new(),
            hero_index: None,
            players: vec![PlayerData::with_starting_stack(1_000.into()); 6],
            button_index: Player::ZERO,
            small_blind: 5.into(),
            big_blind: 10.into(),
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
    pub call: Option<Amount>,
    pub bet: Option<Amount>,
    pub raise: Option<(Amount, Amount)>,
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    #[test]
    fn test_game_with_sample_hands() {
        let path = Path::new("src")
            .join("test_data")
            .join("game_validation_data.json");
        let validation_data_content = fs::read_to_string(path).unwrap();
        let validation_data: Vec<GameValidationData> =
            serde_json::from_str(&validation_data_content).unwrap();

        for game_entry in validation_data {
            let mut game = Game::from_game_data(&game_entry.game).unwrap();
            let start_game = game.clone();
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

            game.undo().unwrap();
            game.showdown_simple().unwrap();
            assert_eq!(game.showdown_stacks, start_game.showdown_stacks);
            assert_eq!(game, start_game);
        }
    }
}
