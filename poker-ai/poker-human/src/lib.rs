use core::fmt;
use std::{
    cmp,
    panic::{catch_unwind, AssertUnwindSafe, UnwindSafe},
    sync::Arc,
};

use poker_core::{
    ai::AiAction,
    bitset::Bitset,
    card::Card,
    cards::Cards,
    db::{HandData, DB},
    game::{milli_big_blind_to_amount_rounded, Action, Game, MilliBigBlind, State, Street},
    hand::Hand,
    init::init,
    range::RangeTableWith,
    rank::Rank,
    result::Result,
    suite::Suite,
};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::*,
};
use rand::{
    distributions::{WeightedError, WeightedIndex},
    prelude::Distribution,
    rngs::SmallRng,
    seq::SliceRandom,
    Rng, SeedableRng,
};

const DEBUG: bool = false;

trait ToPyResult<T> {
    fn py(self) -> PyResult<T>;
}

impl<T> ToPyResult<T> for Result<T> {
    fn py(self) -> PyResult<T> {
        self.map_err(|err| PyValueError::new_err(err.to_string()))
    }
}

#[pyclass]
struct Dataset {
    /// With sum of number of actions per game until this point,
    /// useful for binary search.
    games: Vec<(Game, usize)>,
    total_actions_of_interest: usize,

    /// Game index and sum of showdowns until this point.
    showdowns: Vec<(usize, usize)>,
    total_showdowns_of_interest: usize,

    rng: SmallRng,
}

#[pymethods]
impl Dataset {
    #[classattr]
    const ACTION_INPUT_LEN: usize = ACTION_INPUT_LEN;

    #[classattr]
    const SHOWDOWN_INPUT_LEN: usize = SHOWDOWN_INPUT_LEN;

    #[classattr]
    const ACTION_TARGET_LEN: usize = ACTION_TARGET_LEN;

    #[classattr]
    const SHOWDOWN_TARGET_LEN: usize = SHOWDOWN_TARGET_LEN;

    #[new]
    #[pyo3(signature = (db_path, limit=None))]
    fn new(py: Python<'_>, db_path: &str, limit: Option<usize>) -> PyResult<Self> {
        // SAFETY:
        // If this library is only used from Python,
        // it is not possible to read from another thread
        // while init runs.
        unsafe { init() };

        let db = DB::open(db_path).py()?;

        let mut showdowns = Vec::new();
        let mut total_showdown_count = 0;

        let mut games = Vec::new();
        let mut total_action_count = 0;

        let push_game = |hand_data: HandData| {
            py.check_signals()?;

            let mut game = Game::from_game_data(&hand_data.data)?;
            let current_action_count = Self::count_actions_of_interest(&mut game);

            if current_action_count == 0 {
                return Ok(true);
            }

            let current_showdown_count = Self::showdowns_of_interest(&mut game);
            if current_showdown_count != 0 {
                showdowns.push((games.len(), total_showdown_count));
                total_showdown_count += current_showdown_count;
            }

            games.push((game, total_action_count));
            total_action_count += current_action_count;

            Ok(true)
        };

        match limit {
            Some(limit) => db
                .hand_data_for_each("SELECT * FROM hands_data LIMIT ?", [limit], push_game)
                .py()?,
            None => db
                .hand_data_for_each("SELECT * FROM hands_data", [], push_game)
                .py()?,
        }

        Ok(Self {
            games,
            total_actions_of_interest: total_action_count,
            showdowns,
            total_showdowns_of_interest: total_showdown_count,
            rng: SmallRng::seed_from_u64(42), // deterministic
        })
    }

    fn total_actions_of_interest(&self) -> usize {
        self.total_actions_of_interest
    }

    fn total_showdowns_of_interest(&self) -> usize {
        self.total_showdowns_of_interest
    }

    fn get_action_item(&mut self, index: usize) -> PyResult<(Vec<f32>, Vec<i8>, Vec<f32>)> {
        catch_unwind_helper(AssertUnwindSafe(|| {
            let game = self.get_action_index_game(index);

            let target_index = Self::encode_action_target_index(game);

            assert!(game.previous());

            let x = encode_action_input(game);
            let legal_mask = encode_action_legal_mask(game);

            assert_eq!(legal_mask[target_index], 1);

            (x, legal_mask, Self::create_action_target(target_index))
        }))
    }

    fn action_info(&mut self, index: usize) -> (String, String) {
        let game = self.get_action_index_game(index);

        let hand_name = Arc::unwrap_or_clone(game.hand_name().unwrap_or_default());
        let info = format!("{}: {:?}", game.board().street(), game.actions().last());

        (hand_name, info)
    }

    fn get_showdown_item(&mut self, index: usize) -> PyResult<(Vec<f32>, Vec<i8>, Vec<f32>)> {
        // TODO: Could consider hands with revealed cards without showdown.

        catch_unwind_helper(AssertUnwindSafe(|| {
            let game_index = self.get_showdown_index_game(index);

            let game = &mut self.games[game_index].0;

            let x = encode_showdown_input(game);
            let legal_mask = encode_showdown_legal_mask(game);
            let target = self.encode_showdown_target(game_index);

            (x, legal_mask, target)
        }))
    }

    fn showdown_info(&mut self, index: usize) -> (String, String) {
        let game_index = self.get_showdown_index_game(index);
        let game = &self.games[game_index].0;

        let hand_name = Arc::unwrap_or_clone(game.hand_name().unwrap_or_default());
        let info = format!("{:?}", game.state());

        (hand_name, info)
    }
}

impl Dataset {
    fn next_action_of_interest(game: &mut Game) -> bool {
        // Assumes finalized game.

        if !game.can_previous() {
            // Skip initial posts / straddles.
            assert!(game.next());
        }

        let mut last_action = game.actions().last().copied().unwrap();

        while game.next() {
            let current_action = game.actions().last().copied().unwrap();

            if current_action == last_action {
                return false;
            }

            last_action = current_action;

            match current_action {
                Action::Post { .. } | Action::Straddle { .. } => unreachable!(),
                Action::Fold(_)
                | Action::Check(_)
                | Action::Call { .. }
                | Action::Bet { .. }
                | Action::Raise { .. } => return true,
                Action::Flop(_)
                | Action::Turn(_)
                | Action::River(_)
                | Action::UncalledBet { .. }
                | Action::Shows { .. }
                | Action::MucksOrUnknown(_) => continue,
            }
        }

        false
    }

    fn count_actions_of_interest(game: &mut Game) -> usize {
        game.rewind();

        let mut count = 0usize;

        while Self::next_action_of_interest(game) {
            count += 1;
        }

        count
    }

    fn showdowns_of_interest(game: &mut Game) -> usize {
        game.forward();

        let shows = game
            .actions()
            .iter()
            .filter(|action| matches!(action, Action::Shows { .. }))
            .count();

        if shows == 0 {
            return 0;
        }

        game.actions()
            .iter()
            .filter(|action| matches!(action, Action::Shows { .. } | Action::MucksOrUnknown(_)))
            .count()
    }

    fn get_showdown_index_game(&mut self, index: usize) -> usize {
        assert!(index < self.total_showdowns_of_interest);

        let search_result = self
            .showdowns
            .binary_search_by_key(&index, |(_, count)| *count);

        let showdown_index = match search_result {
            Ok(index) => index,
            Err(index) => index.checked_sub(1).unwrap(),
        };

        let (game_index, showdowns) = self.showdowns[showdown_index];

        let (game, _) = &mut self.games[game_index];

        Self::first_shows_mucks(game);

        for i in 0..Game::MAX_PLAYERS {
            if showdowns + i != index {
                assert!(game.next());
                continue;
            }

            assert!(matches!(game.state(), State::ShowOrMuck(_)));
            return game_index;
        }

        unreachable!();
    }

    fn first_shows_mucks(game: &mut Game) {
        game.rewind();

        while game.next() {
            if matches!(game.state(), State::ShowOrMuck(_)) {
                return;
            }
        }

        panic!("game has no show or muck action");
    }

    fn get_action_index_game(&mut self, index: usize) -> &mut Game {
        assert!(index < self.total_actions_of_interest);

        let search_result = self.games.binary_search_by_key(&index, |(_, count)| *count);

        let game_index = match search_result {
            Ok(index) => index,
            Err(index) => index.checked_sub(1).unwrap(),
        };

        let (game, count) = &mut self.games[game_index];

        game.rewind();

        for i in 0..10_000 {
            assert!(Self::next_action_of_interest(game));

            if *count + i != index {
                continue;
            }

            return game;
        }

        panic!("too many actions in single game");
    }

    fn encode_action_target_index(game: &mut Game) -> usize {
        assert!(game.small_blind() <= game.big_blind());

        let current_action = game.actions().last().copied().unwrap();
        let player = current_action.player().unwrap();

        assert!(game.previous());
        assert_eq!(game.current_player(), Some(player));

        let can_call = game.can_call();
        assert!(game.can_check() || can_call.is_some());

        let pot = game.total_pot();
        let call_amount = can_call.unwrap_or(0);

        let can_open = can_open(game);

        assert!(game.next());

        let target_action_index = match current_action {
            Action::Fold(_) => TARGET_FOLD_INDEX,
            Action::Check(_) | Action::Call { .. } => TARGET_CHECK_CALL_INDEX,
            Action::Bet { amount, .. } | Action::Raise { amount, .. } => {
                let is_all_in = game.current_stacks()[usize::from(player)] == 0;

                if is_all_in {
                    if DEBUG {
                        eprintln!("target: all-in");
                    }

                    TARGET_ALL_IN_INDEX
                } else if can_open {
                    let to = match current_action {
                        Action::Raise { to, .. } => to,
                        _ => unreachable!(),
                    };

                    let class_index =
                        class_index(&OPEN_SIZES, game.amount_to_milli_big_blinds_rounded(to));

                    if DEBUG {
                        eprintln!("target: open raise to {}", OPEN_SIZES[class_index]);
                    }

                    TARGET_BET_RAISE_INDEX + class_index
                } else {
                    let percent_pot = percent_pot(pot, call_amount, amount);

                    let class_index = class_index(&BET_RAISE_PERCENTAGES, i64::from(percent_pot));

                    if DEBUG {
                        eprintln!(
                            "target: bet/raise to {}",
                            BET_RAISE_PERCENTAGES[class_index]
                        );
                    }

                    TARGET_BET_RAISE_INDEX + class_index
                }
            }
            _ => unreachable!(),
        };

        target_action_index
    }

    fn create_action_target(index: usize) -> Vec<f32> {
        let mut target = vec![0.0; Self::ACTION_TARGET_LEN];
        target[index] = 1.0;
        target
    }

    fn encode_showdown_target(&mut self, game_index: usize) -> Vec<f32> {
        let game = &mut self.games[game_index].0;

        let hero_player = match game.state() {
            State::ShowOrMuck(player) => player,
            _ => unreachable!(),
        };

        let current_board = game.board();

        // Show / muck always happens at the point,
        // where no player can act anymore.
        // If it's not on the river,
        // we have cards to come, so forward.
        game.forward();
        assert_eq!(game.runouts().len(), 1);

        let final_board = game.board();
        let final_board_cards = final_board.cards_set();

        let hero_hand = match game.get_hand(usize::from(hero_player)) {
            Some(hand) if game.hand_shown(hero_player) => hand,
            _ => {
                // Get the worst score of the showdown winners.
                // Using final board, because we don't know
                // how the data source handles show / muck.
                // Also easier to implement.
                //
                // TODO: Use the actual showdown order.
                let worst_score = game
                    .showdown_winners_by_pot()
                    .unwrap()
                    .iter()
                    .fold(Bitset::EMPTY, |acc, (_, players)| acc | *players)
                    .iter(game.player_count())
                    .filter_map(|player| game.get_hand(player))
                    .map(|hand| (final_board_cards | hand.to_cards()).score_fast())
                    .min()
                    .unwrap(); // One shows is required in the dataset construction.

                let known_cards = game.known_cards();

                let worse_hands: Vec<_> = Hand::all()
                    .filter(|hand| !hand.to_cards().overlaps(known_cards))
                    .filter(|hand| (hand.to_cards() | final_board_cards).score_fast() < worst_score)
                    .collect();

                // Using a random worse hand than the worst known hand that won something.
                // This is misleading, but I currently don't see another option
                // to still use most showdown data.
                // This should always exist, but it is not guaranteed by the game implementation.
                // Does not always exist in the phh handhq dataset,
                // probably happens in cases where the hand is unknown.
                worse_hands.choose(&mut self.rng).copied().unwrap()
            }
        };

        if DEBUG {
            eprintln!(
                "current_board: {:?}, final_board: {:?}, hero: {}, hand: {}",
                current_board.cards(),
                final_board.cards(),
                hero_player,
                hero_hand,
            );
        }

        let mut target = vec![0.0f32; Self::SHOWDOWN_TARGET_LEN];
        target[hero_hand.to_index()] = 1.0;
        target
    }
}

const ACTION_INPUT_LEN: usize = CURRENT_STACKS_INDEX + STACKS_LEN;

const SHOWDOWN_INPUT_LEN: usize = ACTION_INPUT_LEN + Game::MAX_PLAYERS;

const ACTION_TARGET_LEN: usize = 14;

const SHOWDOWN_TARGET_LEN: usize = Hand::COUNT;

const CARD_LEN: usize = Rank::COUNT + Suite::COUNT;

const BOARD_INDEX: usize = 0;
const BOARD_LEN: usize = CARD_LEN * Street::River.community_card_count();

const ACTION_POST_INDEX: usize = 0;
const ACTION_POST_DEAD_INDEX: usize = 1;
const ACTION_STRADDLE_INDEX: usize = 2;
const ACTION_FOLD_INDEX: usize = 3;
const ACTION_CHECK_CALL_INDEX: usize = 4;
const ACTION_BET_RAISE_INDEX: usize = 5;
const ACTION_KIND_LEN: usize = 6;

const ACTION_KIND_OFFSET: usize = 0;
const ACTION_PLAYER_OFFSET: usize = ACTION_KIND_LEN;
const ACTION_PLAYER_LEN: usize = Game::MAX_PLAYERS;
const ACTION_AMOUNT_OFFSET: usize = ACTION_PLAYER_OFFSET + ACTION_PLAYER_LEN;
const ACTION_LEN: usize = ACTION_AMOUNT_OFFSET + 1;

const ACTIONS_PER_STREET: usize = 30; // TODO: Probably too much.

const ACTIONS_INDEX: usize = BOARD_INDEX + BOARD_LEN;
const ACTIONS_LEN: usize = ACTIONS_PER_STREET * ACTION_LEN * Street::COUNT;

const STACKS_LEN: usize = Game::MAX_PLAYERS;
const STARTING_STACKS_INDEX: usize = ACTIONS_INDEX + ACTIONS_LEN;
const CURRENT_STACKS_INDEX: usize = STARTING_STACKS_INDEX + STACKS_LEN;

const TARGET_FOLD_INDEX: usize = 0;
const TARGET_CHECK_CALL_INDEX: usize = 1;
const TARGET_BET_RAISE_INDEX: usize = 2;
const TARGET_ALL_IN_INDEX: usize = 13;
const TARGET_BET_RAISE_COUNT: usize = TARGET_ALL_IN_INDEX - TARGET_BET_RAISE_INDEX;

/// Last value includes all values afterwards.
const OPEN_SIZES: [MilliBigBlind; TARGET_BET_RAISE_COUNT] = [
    2000, 2200, 2500, 3000, 3500, 4000, 4500, 5000, 7500, 10_000, 20_000,
];

/// Last value includes all values afterwards.
const BET_RAISE_PERCENTAGES: [i64; TARGET_BET_RAISE_COUNT] =
    [10, 25, 33, 50, 67, 80, 100, 125, 150, 200, 300];

pub fn encode_action_input(game: &Game) -> Vec<f32> {
    assert_eq!(game.runouts().len(), 1);

    let mut out = vec![0.0; ACTION_INPUT_LEN];

    let players = (game.button_index()..game.player_count()).chain(0..game.button_index());

    let starting_stacks = game.starting_stacks();

    for (index, player) in players.clone().enumerate() {
        // We accept the potential loss of precision here.
        out[STARTING_STACKS_INDEX + index] =
            starting_stacks[player] as f32 / game.big_blind() as f32;
    }

    let current_stacks = game.current_stacks();

    for (index, player) in players.enumerate() {
        // We accept the potential loss of precision here.
        out[CURRENT_STACKS_INDEX + index] = current_stacks[player] as f32 / game.big_blind() as f32;
    }

    let board = game.board();

    if DEBUG {
        eprintln!("board: {:?}", board.cards());
    }

    let mut cards = Vec::from(board.cards());
    if board.street() >= Street::Flop {
        cards[..Street::Flop.community_card_count()].sort_by(|a, b| a.cmp_by_rank(*b));
    }

    for (index, card) in cards.iter().copied().enumerate() {
        let offset = BOARD_INDEX + index * CARD_LEN;
        encode_card(card, &mut out[offset..offset + CARD_LEN]);
    }

    encode_actions(game, &mut out[ACTIONS_INDEX..ACTIONS_INDEX + ACTIONS_LEN]);

    out
}

fn encode_actions(game: &Game, out: &mut [f32]) {
    assert_eq!(out.len(), ACTIONS_LEN);

    let mut street = Street::PreFlop;
    let mut per_street_index = 0usize;

    for action in game.actions().iter().copied() {
        let (action_kind, player, amount) = match action {
            Action::Post {
                player,
                amount,
                dead,
            } if dead => (ACTION_POST_DEAD_INDEX, player, amount),
            Action::Post { player, amount, .. } => (ACTION_POST_INDEX, player, amount),
            Action::Straddle { player, amount } => (ACTION_STRADDLE_INDEX, player, amount),
            Action::Fold(player) => (ACTION_FOLD_INDEX, player, 0),
            Action::Check(player) => (ACTION_CHECK_CALL_INDEX, player, 0),
            Action::Call { player, amount } => (ACTION_CHECK_CALL_INDEX, player, amount),
            Action::Bet { player, amount } => (ACTION_BET_RAISE_INDEX, player, amount),
            Action::Raise { player, amount, .. } => (ACTION_BET_RAISE_INDEX, player, amount),
            Action::Flop(_) => {
                street = Street::Flop;
                per_street_index = 0;
                continue;
            }
            Action::Turn(_) => {
                street = Street::Turn;
                per_street_index = 0;
                continue;
            }
            Action::River(_) => {
                street = Street::River;
                per_street_index = 0;
                continue;
            }
            _ => continue,
        };

        let player = Game::player_to_button_offset(
            game.player_count(),
            game.button_index(),
            usize::from(player),
        )
        .unwrap();

        let street_index = street.to_usize() * ACTION_LEN * ACTIONS_PER_STREET;

        assert!(per_street_index < ACTIONS_PER_STREET);
        let index = street_index + per_street_index * ACTION_LEN;

        let offset = index + ACTION_KIND_OFFSET;
        one_hot(action_kind, &mut out[offset..offset + ACTION_KIND_LEN]);

        let offset = index + ACTION_PLAYER_OFFSET;
        one_hot(player, &mut out[offset..offset + ACTION_PLAYER_LEN]);

        // We accept the potential loss of precision here.
        out[index + ACTION_AMOUNT_OFFSET] = amount as f32 / game.big_blind() as f32;

        per_street_index += 1;
    }
}

pub fn encode_action_legal_mask(game: &Game) -> Vec<i8> {
    assert!(game.small_blind() <= game.big_blind());

    let player = game.current_player().unwrap();

    let stack = game.current_stacks()[player];

    let can_call = game.can_call();
    assert!(game.can_check() || can_call.is_some());

    let min_amount = game
        .can_bet()
        .or_else(|| game.can_raise().map(|(amount, _)| amount));

    let pot = game.total_pot();
    let call_amount = can_call.unwrap_or(0);

    let can_open = can_open(game);

    if DEBUG {
        dbg!(
            game.big_blind(),
            game.actions(),
            pot,
            can_open,
            min_amount,
            stack
        );
    }

    // Fold, Check/Call is always allowed.
    let mut legal_mask = vec![1; ACTION_TARGET_LEN];

    if min_amount.is_none() {
        if DEBUG {
            eprintln!("legal: all-in not allowed");
        }

        legal_mask[TARGET_ALL_IN_INDEX] = 0;
    }

    if can_open {
        let previous_street_stack = game.previous_street_stacks()[player];
        let previous_street_stack_bb =
            game.amount_to_milli_big_blinds_rounded(previous_street_stack);

        let open_class = class_index(&OPEN_SIZES, previous_street_stack_bb);

        if DEBUG {
            eprintln!("legal: max open is {}", OPEN_SIZES[open_class]);
        }

        for index in (open_class + 1)..TARGET_BET_RAISE_COUNT {
            legal_mask[TARGET_BET_RAISE_INDEX + index] = 0;
        }

        if min_amount.is_none() {
            legal_mask[TARGET_BET_RAISE_INDEX] = 0;
        }
    } else if let Some(min_amount) = min_amount {
        // This allows all-in and a specific bet size to overlap.

        let min_percent_pot = percent_pot(pot, call_amount, min_amount);
        let max_amount = stack.checked_sub(call_amount).unwrap();
        let max_percent_pot = percent_pot(pot, call_amount, max_amount);

        let min_class = class_index(&BET_RAISE_PERCENTAGES, i64::from(min_percent_pot));
        let max_class = class_index(&BET_RAISE_PERCENTAGES, i64::from(max_percent_pot));

        if DEBUG {
            eprintln!(
                "legal: bet/raise: min={}% max={}%",
                BET_RAISE_PERCENTAGES[min_class], BET_RAISE_PERCENTAGES[max_class]
            );
        }

        for index in 0..min_class {
            legal_mask[TARGET_BET_RAISE_INDEX + index] = 0;
        }

        for index in (max_class + 1)..TARGET_BET_RAISE_COUNT {
            legal_mask[TARGET_BET_RAISE_INDEX + index] = 0;
        }
    } else {
        if DEBUG {
            eprintln!("legal: no bet/raise allowed");
        }

        for index in 0..TARGET_BET_RAISE_COUNT {
            legal_mask[TARGET_BET_RAISE_INDEX + index] = 0;
        }
    }

    if min_amount.is_none() {
        assert!(
            legal_mask[TARGET_BET_RAISE_INDEX..TARGET_BET_RAISE_INDEX + TARGET_BET_RAISE_COUNT]
                .iter()
                .copied()
                .all(|flag| flag == 0)
        )
    }

    legal_mask
}

pub fn encode_showdown_input(game: &Game) -> Vec<f32> {
    let hero_player = match game.state() {
        State::ShowOrMuck(player) => player,
        _ => unreachable!(),
    };

    let player_button_offset = Game::player_to_button_offset(
        game.player_count(),
        game.button_index(),
        usize::from(hero_player),
    )
    .unwrap();

    let mut x = encode_action_input(game);

    let mut player_encoded = [0.0; Game::MAX_PLAYERS];
    one_hot(player_button_offset, &mut player_encoded);

    x.extend(player_encoded);
    assert_eq!(x.len(), SHOWDOWN_INPUT_LEN);

    x
}

pub fn encode_showdown_legal_mask(game: &Game) -> Vec<i8> {
    assert!(matches!(game.state(), State::ShowOrMuck(_)));

    let community_cards = game.board().cards_set();

    let mut legal_mask = vec![0i8; SHOWDOWN_TARGET_LEN];

    for hand in Hand::all() {
        let index = hand.to_index();

        if community_cards.overlaps(hand.to_cards()) {
            legal_mask[index] = 0;
        } else {
            legal_mask[index] = 1;
        }
    }

    legal_mask
}

fn encode_card(card: Card, out: &mut [f32]) {
    assert_eq!(out.len(), CARD_LEN);

    one_hot(card.rank().to_usize(), &mut out[..Rank::COUNT]);
    one_hot(card.suite().to_usize(), &mut out[Rank::COUNT..]);
}

fn one_hot(index: usize, out: &mut [f32]) {
    assert!(index < out.len());

    for (i, v) in out.iter_mut().enumerate() {
        *v = if i == index { 1.0 } else { 0.0 };
    }
}

fn can_open(game: &Game) -> bool {
    game.board().street() == Street::PreFlop
        && game
            .actions()
            .iter()
            .all(|action| !matches!(action, Action::Raise { .. }))
}

fn class_index(classes: &[i64], value: i64) -> usize {
    assert!(!classes.is_empty());
    debug_assert!(classes.is_sorted());

    match classes.binary_search(&value) {
        Ok(index) => index,
        Err(index) => {
            if index == 0 {
                index
            } else if index == classes.len() {
                index - 1
            } else {
                let bottom = classes[index - 1];
                let top = classes[index];

                let mid = bottom + ((top - bottom) / 2);

                if value <= mid {
                    index - 1
                } else {
                    index
                }
            }
        }
    }
}

fn percent_pot(pot: u32, call_amount: u32, amount: u32) -> u32 {
    // We use the calculation for bets/raises, giving the percentage of the pot.
    // This is typically used to give the caller specific pot odds.
    // Often poker software implements them with configurable percent buttons,
    // although sometimes the calculation is different.
    // But I think this is the best solution to abstract the sizes.

    assert_ne!(pot, 0);
    let pot_with_call = pot.checked_add(call_amount).unwrap();
    let percent = f64::from(amount) / f64::from(pot_with_call);
    let percent = (percent * 100.0).round();
    assert!(percent >= 0.0 && percent <= f64::from(u32::MAX));
    let percent = percent as u32;

    percent
}

fn percent_pot_to_amount(pot_with_call: MilliBigBlind, percent: i64) -> MilliBigBlind {
    assert!(percent >= 0);

    // We accept the precision loss here.
    let amount = ((percent as f64) / 100.0) * pot_with_call as f64;
    amount.round() as MilliBigBlind
}

pub struct ActionHead(Py<PyAny>);

impl ActionHead {
    pub fn new(model_path: &str) -> Result<Self> {
        Python::with_gil(|py| {
            // TODO: This is circular and somewhat ugly.
            let poker_human = PyModule::import(py, "python.poker_human_user")?;

            let action_head = poker_human
                .getattr("ActionHead")?
                .call_method1("for_predict", (model_path,))?
                .unbind();

            Ok(Self(action_head))
        })
    }

    pub fn clone_ref(&self, py: Python<'_>) -> Self {
        Self(self.0.clone_ref(py))
    }
}

pub struct ActionProbabilities {
    fold: f32,
    check_call: f32,
    bet_raise: [f32; TARGET_BET_RAISE_COUNT],
    all_in: f32,
    pot_with_call_for_bet_raise: Option<MilliBigBlind>,
    raise_offset: u32,
    big_blind: u32,
    min_amount: u32,
    max_amount: u32,
}

impl fmt::Display for ActionProbabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fold = {}", self.fold)?;
        writeln!(f, "check/call = {}", self.check_call)?;

        for index in 0..TARGET_BET_RAISE_COUNT {
            writeln!(
                f,
                "bet/raise {} ({}BB) = {}",
                self.bet_raise_size_string(index),
                self.bet_raise_size(index) as f64 / 1000.0,
                self.bet_raise(index)
            )?;
        }

        writeln!(f, "all_in = {}", self.all_in)
    }
}

impl ActionProbabilities {
    pub fn predict(action_head: &ActionHead, game: &Game) -> Result<Self> {
        let action_input = encode_action_input(game);
        let legal_mask = encode_action_legal_mask(game);

        let probs: Vec<f32> = Python::with_gil(|py| {
            action_head
                .0
                .call_method1(py, "predict", (action_input, &legal_mask))?
                .extract(py)
        })?;

        if probs.len() != ACTION_TARGET_LEN {
            return Err("model actions output has bad len".into());
        }

        let between_zero_and_one = probs.iter().all(|p| *p >= 0.0 && *p <= 1.0);

        let probs_sum: f32 = probs.iter().sum();
        let sum_to_one = (1.0 - probs_sum).abs() < 0.02;

        if !between_zero_and_one || !sum_to_one {
            return Err("model actions not a valid probability distribution".into());
        }

        for (index, b) in legal_mask.iter().copied().enumerate() {
            if b != 0 && b != 1 {
                return Err("action model legal mask entry is not zero or one".into());
            }

            if b == 0 && probs[index] != 0.0 {
                return Err("action model has illegal action with non zero probability".into());
            }
        }

        let bet_raise_data =
            &probs[TARGET_BET_RAISE_INDEX..TARGET_BET_RAISE_INDEX + TARGET_BET_RAISE_COUNT];

        let mut bet_raise = [0.0f32; TARGET_BET_RAISE_COUNT];
        bet_raise.copy_from_slice(bet_raise_data);

        let call_amount = game.can_call().unwrap_or(0);

        let pot_with_call = game.total_pot().checked_add(call_amount).unwrap();
        let pot_with_call = game.amount_to_milli_big_blinds_rounded(pot_with_call);

        // TODO: Use amount here with AiAction raise amount.
        let min_amount = game
            .can_bet()
            .or_else(|| game.can_raise().map(|(_, to)| to))
            .unwrap_or(0);

        let player = game.current_player().unwrap();
        let stack = game.current_stacks()[player];

        // TODO: Use amount here with AiAction raise amount.
        let max_amount = if game.can_raise().is_some() {
            game.previous_street_stack().unwrap()
        } else {
            stack
        };

        // TODO: Probably should use raise amount in AiAction.
        let raise_offset = if game.can_raise().is_some() && !can_open(game) {
            game.invested_in_street(player)
                .checked_add(call_amount)
                .unwrap()
        } else {
            0
        };

        Ok(Self {
            fold: probs[TARGET_FOLD_INDEX],
            check_call: probs[TARGET_CHECK_CALL_INDEX],
            bet_raise,
            all_in: probs[TARGET_ALL_IN_INDEX],
            pot_with_call_for_bet_raise: if can_open(game) {
                None
            } else {
                Some(pot_with_call)
            },
            raise_offset,
            big_blind: game.big_blind(),
            min_amount,
            max_amount,
        })
    }

    pub fn fold(&self) -> f32 {
        self.fold
    }

    pub fn check_call(&self) -> f32 {
        self.check_call
    }

    pub fn can_open(&self) -> bool {
        self.pot_with_call_for_bet_raise.is_none()
    }

    pub fn bet_raise(&self, index: usize) -> f32 {
        assert!(index < TARGET_BET_RAISE_COUNT);
        self.bet_raise[index]
    }

    pub fn bet_raise_size(&self, index: usize) -> MilliBigBlind {
        assert!(index < TARGET_BET_RAISE_COUNT);

        if let Some(pot_with_call) = self.pot_with_call_for_bet_raise {
            let percent = BET_RAISE_PERCENTAGES[index];
            percent_pot_to_amount(pot_with_call, percent)
        } else {
            OPEN_SIZES[index]
        }
    }

    pub fn bet_raise_size_string(&self, index: usize) -> String {
        if self.can_open() {
            format!("{}BB", OPEN_SIZES[index] as f64 / 1000.0)
        } else {
            format!("{}%", BET_RAISE_PERCENTAGES[index])
        }
    }

    pub fn all_in(&self) -> f32 {
        self.all_in
    }

    pub fn choose(&self, rng: &mut impl Rng) -> (AiAction, String) {
        let weights = [self.fold, self.check_call, self.all_in]
            .into_iter()
            .chain(self.bet_raise);

        let weights = WeightedIndex::new(weights).unwrap();
        let index = weights.sample(rng);

        const BET_RAISE_END: usize = 3 + TARGET_BET_RAISE_COUNT;

        let mut extra_info = String::new();

        let action = match index {
            0 => AiAction::Fold,
            1 => AiAction::CheckCall,
            2 => AiAction::AllIn,
            3..BET_RAISE_END => {
                let index = index - 3;
                let size = self.bet_raise_size(index);

                let amount = milli_big_blind_to_amount_rounded(size, self.big_blind).unwrap();
                let amount = amount.checked_add(self.raise_offset).unwrap();
                let amount = cmp::max(amount, self.min_amount);
                let amount = cmp::min(amount, self.max_amount);

                extra_info = self.bet_raise_size_string(index);

                AiAction::BetRaise(amount)
            }
            _ => unreachable!(),
        };

        (action, extra_info)
    }
}

pub struct ShowdownHead(Py<PyAny>);

impl ShowdownHead {
    pub fn new(model_path: &str) -> Result<Self> {
        Python::with_gil(|py| {
            // TODO: This is circular and somewhat ugly.
            let poker_human = PyModule::import(py, "python.poker_human_user")?;

            let showdown_head = poker_human
                .getattr("ShowdownHead")?
                .call_method1("for_predict", (model_path,))?
                .unbind();

            Ok(Self(showdown_head))
        })
    }

    pub fn clone_ref(&self, py: Python<'_>) -> Self {
        Self(self.0.clone_ref(py))
    }
}

#[derive(Debug)]
pub struct ShowdownProbabilities {
    range: RangeTableWith<f32>,
}

impl ShowdownProbabilities {
    pub fn predict(showdown_head: &ShowdownHead, game: &Game) -> Result<Self> {
        let showdown_input = encode_showdown_input(game);
        let legal_mask = encode_showdown_legal_mask(game);

        let probs: Vec<f32> = Python::with_gil(|py| {
            showdown_head
                .0
                .call_method1(py, "predict", (showdown_input, &legal_mask))?
                .extract(py)
        })?;

        if probs.len() != SHOWDOWN_TARGET_LEN {
            return Err("showdown model output has bad len".into());
        }

        let between_zero_and_one = probs.iter().all(|p| *p >= 0.0 && *p <= 1.0);

        let probs_sum: f32 = probs.iter().sum();
        let sum_to_one = (1.0 - probs_sum).abs() < 0.02;

        if !between_zero_and_one || !sum_to_one {
            return Err("showdown model output is not a valid probability distribution".into());
        }

        for (index, b) in legal_mask.iter().copied().enumerate() {
            if b != 0 && b != 1 {
                return Err("showdown model legal mask entry is not zero or one".into());
            }

            if b == 0 && probs[index] != 0.0 {
                return Err("showdown model has illegal hand with non zero probability".into());
            }
        }

        Ok(Self {
            range: RangeTableWith::from_iter(probs)?,
        })
    }

    pub fn get(&self, hand: Hand) -> f32 {
        self.range[hand]
    }

    pub fn choose(&self, rng: &mut impl Rng, know_cards: Cards) -> Option<Hand> {
        let weights = self.range.iter().map(|(hand, p)| {
            if hand.to_cards().overlaps(know_cards) {
                0.0
            } else {
                *p
            }
        });

        let weights = match WeightedIndex::new(weights) {
            Ok(weights) => weights,
            Err(WeightedError::AllWeightsZero) => return None,
            Err(err) => panic!("{err}"),
        };

        Some(Hand::from_index(weights.sample(rng)))
    }
}

fn catch_unwind_helper<F: FnOnce() -> R + UnwindSafe, R>(f: F) -> PyResult<R> {
    catch_unwind(f).map_err(|err| {
        let message = if let Some(s) = err.downcast_ref::<&str>() {
            format!("panic: {s}")
        } else if let Some(s) = err.downcast_ref::<String>() {
            format!("panic: {s}")
        } else {
            "panic: unknown reason".to_owned()
        };

        PyRuntimeError::new_err(message)
    })
}

#[pymodule]
mod poker_human {
    #[pymodule_export]
    use super::Dataset;
}
