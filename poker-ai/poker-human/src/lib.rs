use std::sync::Arc;

use poker_core::{
    card::Card,
    cards::{Cards, Score},
    db::{HandData, DB},
    equity::{total_combos_upper_bound, EquityTable},
    game::{Action, Game, MilliBigBlind, Street},
    hand::Hand,
    init::init,
    range::RangeTable,
    result::Result,
    suite::Suite,
};
use pyo3::{exceptions::PyValueError, prelude::*};
use rand::{rngs::SmallRng, seq::SliceRandom, Rng, SeedableRng};

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
    const ACTION_INPUT_LEN: usize = 380;

    #[classattr]
    const SHOWDOWN_INPUT_LEN: usize = 381;

    #[classattr]
    const ACTION_TARGET_LEN: usize = 14;

    #[classattr]
    const SHOWDOWN_TARGET_LEN: usize = Hand::COUNT;

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

    fn get_action_item(&mut self, index: usize) -> (Vec<f32>, Vec<i8>, Vec<f32>) {
        let game = self.get_action_index_game(index);

        let target_index = Self::encode_action_target_index(game);

        assert!(game.previous());

        let x = Self::encode_game(game);
        let legal_mask = Self::encode_action_legal_mask(game);

        assert_eq!(legal_mask[target_index], 1);

        (x, legal_mask, Self::create_action_target(target_index))
    }

    fn get_showdown_item(&mut self, index: usize) -> (Vec<f32>, Vec<i8>, Vec<f32>) {
        // TODO: Could consider hands with revealed cards without showdown.

        // TODO:
        // Need to keep in mind that we unified the
        // community card suites, which is tricky here.
        // Maybe we cannot do that.

        let game_index = self.get_showdown_index_game(index);

        let game = &mut self.games[game_index].0;

        let x = Self::encode_showdown_input(game);
        let legal_mask = Self::encode_showdown_legal_mask(game);
        let target = self.encode_showdown_target(game_index);

        (x, legal_mask, target)
    }

    fn showdown_info(&mut self, index: usize) -> (String, String) {
        let game_index = self.get_showdown_index_game(index);
        let game = &self.games[game_index].0;

        let hand_name = Arc::unwrap_or_clone(game.hand_name().unwrap_or_default());
        let info = format!("{:?}", game.actions().last().unwrap());

        (hand_name, info)
    }
}

impl Dataset {
    const BOARD_FLOP_INDEX: usize = 0;
    const BOARD_TURN_INDEX: usize = 6;
    const BOARD_RIVER_INDEX: usize = 8;

    const ACTION_POST: u8 = 1;
    const ACTION_POST_DEAD: u8 = 2;
    const ACTION_STRADDLE: u8 = 3;
    const ACTION_FOLD: u8 = 4;
    const ACTION_CHECK_CALL: u8 = 5;
    const ACTION_BET_RAISE: u8 = 6;

    const ACTION_SIZE: usize = 3;
    const ACTION_KIND_OFFSET: usize = 0;
    const ACTION_PLAYER_OFFSET: usize = 1;
    const ACTION_AMOUNT_OFFSET: usize = 2;

    const ACTIONS_PER_STREET: usize = 30; // TODO: Probably too much.

    const PRE_FLOP_ACTIONS_INDEX: usize = 10;
    const FLOP_ACTIONS_INDEX: usize =
        Self::PRE_FLOP_ACTIONS_INDEX + Self::ACTIONS_PER_STREET * Self::ACTION_SIZE;
    const TURN_ACTIONS_INDEX: usize =
        Self::FLOP_ACTIONS_INDEX + Self::ACTIONS_PER_STREET * Self::ACTION_SIZE;
    const RIVER_ACTIONS_INDEX: usize =
        Self::TURN_ACTIONS_INDEX + Self::ACTIONS_PER_STREET * Self::ACTION_SIZE;

    const STACK_SIZES_INDEX: usize =
        Self::RIVER_ACTIONS_INDEX + Self::ACTIONS_PER_STREET * Self::ACTION_SIZE;

    const FOLD_INDEX: usize = 0;
    const CHECK_CALL_INDEX: usize = 1;
    const BET_RAISE_INDEX: usize = 2;
    const ALL_IN_INDEX: usize = 13;
    const BET_RAISE_COUNT: usize = Self::ALL_IN_INDEX - Self::BET_RAISE_INDEX;

    /// Last value includes all values afterwards.
    const OPEN_SIZES: [MilliBigBlind; Self::BET_RAISE_COUNT] = [
        2000, 2200, 2500, 3000, 3500, 4000, 4500, 5000, 7500, 10_000, 20_000,
    ];

    /// Last value includes all values afterwards.
    const BET_RAISE_PERCENTAGES: [i64; Self::BET_RAISE_COUNT] =
        [10, 25, 33, 50, 67, 80, 100, 125, 150, 200, 300];

    fn next_action_of_interest(game: &mut Game) -> bool {
        // Assumes finalized game.

        if !game.can_previous() {
            // Skip initial posts / straddles.
            assert!(game.next());
            assert!(game.next());
            return true;
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

        assert!(count > 0);
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

            let action = game.actions().last().copied().unwrap();

            match action {
                Action::Shows { .. } | Action::MucksOrUnknown(_) => return game_index,
                _ => unreachable!(),
            }
        }

        unreachable!();
    }

    fn first_shows_mucks(game: &mut Game) {
        game.rewind();

        while game.next() {
            let action = game.actions().last().copied().unwrap();

            if matches!(action, Action::Shows { .. } | Action::MucksOrUnknown(_)) {
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

    fn encode_action_legal_mask(game: &Game) -> Vec<i8> {
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

        let can_open = Self::can_open(game);

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
        let mut legal_mask = vec![1; Self::ACTION_TARGET_LEN];

        if min_amount.is_none() {
            if DEBUG {
                eprintln!("legal: all-in not allowed");
            }

            legal_mask[Self::ALL_IN_INDEX] = 0;
        }

        if can_open {
            let previous_street_stack = game.previous_street_stacks()[player];
            let previous_street_stack_bb =
                game.amount_to_milli_big_blinds_rounded(previous_street_stack);

            let open_class = Self::class_index(&Self::OPEN_SIZES, previous_street_stack_bb);

            if DEBUG {
                eprintln!("legal: max open is {}", Self::OPEN_SIZES[open_class]);
            }

            for index in (open_class + 1)..Self::BET_RAISE_COUNT {
                legal_mask[Self::BET_RAISE_INDEX + index] = 0;
            }

            if min_amount.is_none() {
                legal_mask[Self::BET_RAISE_INDEX] = 0;
            }
        } else if let Some(min_amount) = min_amount {
            // This allows all-in and a specific bet size to overlap.

            let min_percent_pot = Self::percent_pot(pot, call_amount, min_amount);
            let max_amount = stack.checked_sub(call_amount).unwrap();
            let max_percent_pot = Self::percent_pot(pot, call_amount, max_amount);

            let min_class =
                Self::class_index(&Self::BET_RAISE_PERCENTAGES, i64::from(min_percent_pot));
            let max_class =
                Self::class_index(&Self::BET_RAISE_PERCENTAGES, i64::from(max_percent_pot));

            if DEBUG {
                eprintln!(
                    "legal: bet/raise: min={}% max={}%",
                    Self::BET_RAISE_PERCENTAGES[min_class],
                    Self::BET_RAISE_PERCENTAGES[max_class]
                );
            }

            for index in 0..min_class {
                legal_mask[Self::BET_RAISE_INDEX + index] = 0;
            }

            for index in (max_class + 1)..Self::BET_RAISE_COUNT {
                legal_mask[Self::BET_RAISE_INDEX + index] = 0;
            }
        } else {
            if DEBUG {
                eprintln!("legal: no bet/raise allowed");
            }

            for index in 0..Self::BET_RAISE_COUNT {
                legal_mask[Self::BET_RAISE_INDEX + index] = 0;
            }
        }

        if min_amount.is_none() {
            assert!(legal_mask
                [Self::BET_RAISE_INDEX..Self::BET_RAISE_INDEX + Self::BET_RAISE_COUNT]
                .iter()
                .copied()
                .all(|flag| flag == 0))
        }

        legal_mask
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

        let can_open = Self::can_open(game);

        assert!(game.next());

        let target_action_index = match current_action {
            Action::Fold(_) => Self::FOLD_INDEX,
            Action::Check(_) | Action::Call { .. } => Self::CHECK_CALL_INDEX,
            Action::Bet { amount, .. } | Action::Raise { amount, .. } => {
                let is_all_in = game.current_stacks()[usize::from(player)] == 0;

                if is_all_in {
                    if DEBUG {
                        eprintln!("target: all-in");
                    }

                    Self::ALL_IN_INDEX
                } else if can_open {
                    let to = match current_action {
                        Action::Raise { to, .. } => to,
                        _ => unreachable!(),
                    };

                    let class_index = Self::class_index(
                        &Self::OPEN_SIZES,
                        game.amount_to_milli_big_blinds_rounded(to),
                    );

                    if DEBUG {
                        eprintln!("target: open raise to {}", Self::OPEN_SIZES[class_index]);
                    }

                    Self::BET_RAISE_INDEX + class_index
                } else {
                    let percent_pot = Self::percent_pot(pot, call_amount, amount);

                    let class_index =
                        Self::class_index(&Self::BET_RAISE_PERCENTAGES, i64::from(percent_pot));

                    if DEBUG {
                        eprintln!(
                            "target: bet/raise to {}",
                            Self::BET_RAISE_PERCENTAGES[class_index]
                        );
                    }

                    Self::BET_RAISE_INDEX + class_index
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

    fn encode_game(game: &Game) -> Vec<f32> {
        assert_eq!(game.runouts().len(), 1);

        let mut out = vec![0.0; Self::ACTION_INPUT_LEN];

        let stacks = game.current_stacks();

        let players = (game.button_index()..game.player_count()).chain(0..game.button_index());

        for (index, player) in players.enumerate() {
            // We accept the potential loss of precision here.
            out[Self::STACK_SIZES_INDEX + index] = stacks[player] as f32 / game.big_blind() as f32;
        }

        let board = game.board();

        if DEBUG {
            eprintln!("board: {:?}", board.cards());
        }

        // Can only map suites from flop,
        // although in some cases more flexibility is possible.
        let suite_mapping = if let Some(flop) = board.flop() {
            Cards::from_slice(&flop).unwrap().unify_suites_mapping()
        } else {
            Suite::SUITES
        };

        if let Some(mut flop) = board.flop() {
            for card in &mut flop {
                *card = Card::of(card.rank(), suite_mapping[card.suite().to_usize()]);
            }

            flop.sort_by(|a, b| a.cmp_by_rank(*b).reverse());

            for (index, card) in flop.iter().copied().enumerate() {
                out[Self::BOARD_FLOP_INDEX + index * 2] = Self::encode_card(card).0;
                out[Self::BOARD_FLOP_INDEX + index * 2 + 1] = Self::encode_card(card).1;
            }

            if DEBUG {
                eprintln!("flop converted: {:?}", &flop);
            }
        }

        if let Some(turn) = board.turn() {
            let turn = Card::of(turn.rank(), suite_mapping[turn.suite().to_usize()]);
            out[Self::BOARD_TURN_INDEX] = Self::encode_card(turn).0;
            out[Self::BOARD_TURN_INDEX + 1] = Self::encode_card(turn).1;

            if DEBUG {
                eprintln!("turn converted: {:?}", turn);
            }
        }

        if let Some(river) = board.river() {
            let river = Card::of(river.rank(), suite_mapping[river.suite().to_usize()]);
            out[Self::BOARD_RIVER_INDEX] = Self::encode_card(river).0;
            out[Self::BOARD_RIVER_INDEX + 1] = Self::encode_card(river).1;

            if DEBUG {
                eprintln!("river converted: {:?}", river);
            }
        }

        let mut street = Street::PreFlop;
        let mut street_index = 0usize;

        for action in game.actions().iter().copied() {
            let (action_kind, player, amount) = match action {
                Action::Post {
                    player,
                    amount,
                    dead,
                } if dead => (Self::ACTION_POST_DEAD, player, amount),
                Action::Post { player, amount, .. } => (Self::ACTION_POST, player, amount),
                Action::Straddle { player, amount } => (Self::ACTION_STRADDLE, player, amount),
                Action::Fold(player) => (Self::ACTION_FOLD, player, 0),
                Action::Check(player) => (Self::ACTION_CHECK_CALL, player, 0),
                Action::Call { player, amount } => (Self::ACTION_CHECK_CALL, player, amount),
                Action::Bet { player, amount } => (Self::ACTION_BET_RAISE, player, amount),
                Action::Raise { player, amount, .. } => (Self::ACTION_BET_RAISE, player, amount),
                Action::Flop(_) => {
                    street = Street::Flop;
                    street_index = 0;
                    continue;
                }
                Action::Turn(_) => {
                    street = Street::Turn;
                    street_index = 0;
                    continue;
                }
                Action::River(_) => {
                    street = Street::River;
                    street_index = 0;
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
            let player = u8::try_from(player).unwrap();

            let actions_index = match street {
                Street::PreFlop => Self::PRE_FLOP_ACTIONS_INDEX,
                Street::Flop => Self::FLOP_ACTIONS_INDEX,
                Street::Turn => Self::TURN_ACTIONS_INDEX,
                Street::River => Self::RIVER_ACTIONS_INDEX,
            };

            assert!(street_index < Self::ACTIONS_PER_STREET);
            let index = actions_index + street_index * Self::ACTION_SIZE;

            out[index + Self::ACTION_KIND_OFFSET] = f32::from(action_kind);
            out[index + Self::ACTION_PLAYER_OFFSET] = f32::from(player + 1);
            // We accept the potential loss of precision here.
            out[index + Self::ACTION_AMOUNT_OFFSET] = amount as f32 / game.big_blind() as f32;

            street_index += 1;
        }

        out
    }

    fn encode_card(card: Card) -> (f32, f32) {
        let rank = f32::from(card.rank().to_i8() + 1);
        let suite = f32::from(card.suite().to_i8() + 1);
        (rank, suite)
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

    fn encode_showdown_input(game: &Game) -> Vec<f32> {
        let action = game.actions().last().copied().unwrap();

        let hero_player = match action {
            Action::Shows { player, .. } | Action::MucksOrUnknown(player) => player,
            _ => unreachable!(),
        };

        let player_button_offset = Game::player_to_button_offset(
            game.player_count(),
            game.button_index(),
            usize::from(hero_player),
        )
        .unwrap();
        let player_button_offset = u8::try_from(player_button_offset).unwrap();

        let mut x = Self::encode_game(game);
        x.push(f32::from(player_button_offset) + 1.0);
        assert_eq!(x.len(), Self::SHOWDOWN_INPUT_LEN);

        x
    }

    fn encode_showdown_legal_mask(game: &Game) -> Vec<i8> {
        let community_cards = game.board().cards_set();

        let mut legal_mask = vec![0i8; Self::SHOWDOWN_TARGET_LEN];

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

    fn encode_showdown_target(&mut self, game_index: usize) -> Vec<f32> {
        let game = &mut self.games[game_index].0;

        let action = game.actions().last().copied().unwrap();

        let hero_player = match action {
            Action::Shows { player, .. } | Action::MucksOrUnknown(player) => player,
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

        if DEBUG {
            eprintln!(
                "current_board: {:?}, final_board: {:?}, hero: {}",
                current_board.cards(),
                final_board.cards(),
                hero_player,
            )
        }

        let showdown_players = (0..game.player_count())
            .filter(|player| game.hand_shown(*player) || game.hand_mucked(*player));

        let mucked_hands = (0..game.player_count())
            .filter(|player| game.hand_mucked(*player))
            .count();

        let mut worse_hands = if mucked_hands != 0 {
            let mut worst_score = Score::MAX;

            for player in showdown_players.clone() {
                let Some(hand) = game.get_hand(player) else {
                    continue;
                };

                // Need to be conservative. Using final board,
                // because we don't know how the data source handles
                // show / muck. Also easier to implement.
                let player_cards = final_board_cards | hand.to_cards();
                let score = player_cards.score_fast();

                if score <= worst_score {
                    worst_score = score;
                }
            }

            // One shows is required in the dataset construction.
            assert_ne!(worst_score, Score::MAX);

            let known_cards = game.known_cards();

            let worse_hands: Vec<_> = Hand::all()
                .filter(|hand| !final_board_cards.overlaps(hand.to_cards()))
                .filter(|hand| (hand.to_cards() | final_board_cards).score_fast() < worst_score)
                .filter(|hand| !hand.to_cards().overlaps(known_cards))
                .collect();

            worse_hands
        } else {
            Vec::new()
        };

        let ranges: Vec<_> = showdown_players
            .clone()
            .map(|player| {
                if player == usize::from(hero_player) {
                    Box::new(RangeTable::FULL)
                } else {
                    let hand = game.get_hand(player).unwrap_or_else(|| {
                        // Using a random worse hand for every player than the one we found.
                        // This is misleading, but currently I don't see another option
                        // to still use most showdown data.
                        // TODO: Can optimize distribution.

                        // For every player who mucked,
                        // we should have one hand that is worse.
                        // This is not guaranteed.
                        let player_hand = worse_hands.choose(&mut self.rng).copied().unwrap();
                        worse_hands
                            .retain(|hand| !hand.to_cards().overlaps(player_hand.to_cards()));
                        player_hand
                    });

                    if DEBUG {
                        eprintln!(
                            "player {} hand: {} (known: {})",
                            player,
                            hand,
                            game.get_hand(player).is_some()
                        );
                    }

                    Box::new(RangeTable::from_hands([hand]).unwrap())
                }
            })
            .collect();

        let community_cards = current_board.cards_set();

        // Should always succeed, non hero ranges have size of one,
        // so the total is small.
        let equity = if total_combos_upper_bound(community_cards, &ranges) <= 100_000 {
            EquityTable::enumerate(community_cards, &ranges).unwrap()
        } else {
            EquityTable::simulate(community_cards, &ranges, 300_000).unwrap()
        };

        let hero_range_index = showdown_players
            .clone()
            .position(|player| player == usize::from(hero_player))
            .unwrap();

        let hand_cards = ranges
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != hero_range_index)
            .flat_map(|(_, range)| range.into_iter())
            .fold(Cards::EMPTY, |acc, hand| acc | hand.to_cards());

        let equity = &equity[hero_range_index];

        let mut target = vec![0.0f32; Self::SHOWDOWN_TARGET_LEN];

        for hand in Hand::all() {
            let index = hand.to_index();

            if community_cards.overlaps(hand.to_cards()) {
                target[index] = 0.0;
            } else if hand_cards.overlaps(hand.to_cards()) {
                // Another players hand overlaps.
                // Also using a random value here,
                // because what else to do?

                target[index] = self.rng.gen_range(0.0..=1.0);
            } else {
                target[index] = equity.equity(hand).equity_percent() as f32;
            }
        }

        target
    }
}

#[pymodule]
mod poker_human {
    #[pymodule_export]
    use super::Dataset;
}
