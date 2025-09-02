use poker_core::{
    card::Card,
    cards::Cards,
    db::{HandData, DB},
    game::{Action, Game, MilliBigBlind, Street},
    result::Result,
    suite::Suite,
};
use pyo3::{exceptions::PyValueError, prelude::*};

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
}

#[pymethods]
impl Dataset {
    #[classattr]
    const INPUT_LEN: usize = 380;

    #[classattr]
    const TARGET_LEN: usize = 14;

    #[new]
    #[pyo3(signature = (db_path, limit=None))]
    fn new(db_path: &str, limit: Option<usize>) -> PyResult<Self> {
        let db = DB::open(db_path).py()?;

        let mut games = Vec::new();
        let mut total_count = 0;

        let push_game = |hand_data: HandData| {
            let mut game = Game::from_game_data(&hand_data.data)?;
            let current_count = Self::count_actions_of_interest(&mut game);

            games.push((game, total_count));

            total_count += current_count;

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
            total_actions_of_interest: total_count,
        })
    }

    fn total_actions_of_interest(&self) -> usize {
        self.total_actions_of_interest
    }

    fn get_item(&mut self, index: usize) -> (Vec<f32>, Vec<i8>, Vec<f32>) {
        let game = self.get_index_game(index);

        let (legal_mask, target) = Self::encode_legal_mask_target(game);
        (Self::encode_game(game), legal_mask, target)
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

    fn get_index_game(&mut self, index: usize) -> &mut Game {
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

    fn encode_legal_mask_target(game: &mut Game) -> (Vec<i8>, Vec<f32>) {
        assert!(game.small_blind() <= game.big_blind());

        let current_action = game.actions().last().copied().unwrap();
        let player = current_action.player().unwrap();

        assert!(game.previous());
        assert!(game.current_player().is_some());

        let old_stack = game.current_stacks()[player];

        let can_check = game.can_check();
        let can_call = game.can_call();
        assert!(can_check || can_call.is_some());

        let can_bet = game.can_bet();
        let can_raise = game.can_raise();
        let min_amount = can_bet.or_else(|| can_raise.map(|(amount, _)| amount));

        let pot = game.total_pot();
        let call_amount = can_call.unwrap_or(0);

        assert!(game.next());

        let previous_actions = &game.actions()[..game.actions().len().checked_sub(1).unwrap()];

        let is_open = game.board().street() == Street::PreFlop
            && matches!(current_action, Action::Raise { .. })
            && previous_actions
                .iter()
                .all(|action| !matches!(action, Action::Raise { .. }));

        if DEBUG {
            dbg!(
                game.big_blind(),
                game.actions(),
                pot,
                is_open,
                min_amount,
                old_stack
            );
        }

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
                } else if is_open {
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

        let legal_mask = {
            // Fold, Check/Call is always allowed.
            let mut legal_mask = vec![1; Self::TARGET_LEN];

            if min_amount.is_none() {
                if DEBUG {
                    eprintln!("legal: all-in not allowed");
                }

                legal_mask[Self::ALL_IN_INDEX] = 0;
            }

            if is_open {
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
                let max_percent_pot = Self::percent_pot(pot, call_amount, old_stack); // TODO: Probably not correct.

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
        };

        assert_eq!(legal_mask[target_action_index], 1);

        let target = {
            let mut target = vec![0.0; Self::TARGET_LEN];
            target[target_action_index] = 1.0;
            target
        };

        (legal_mask, target)
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

    fn encode_game(game: &mut Game) -> Vec<f32> {
        assert_eq!(game.runouts().len(), 1);

        let mut out = vec![0.0; Self::INPUT_LEN];

        assert!(game.previous());

        let stacks = game.current_stacks();

        let players = (game.button_index()..game.player_count()).chain(0..game.button_index());

        for (index, player) in players.enumerate() {
            // We accept the potential loss of precision here.
            out[Self::STACK_SIZES_INDEX + index] = stacks[player] as f32 / game.big_blind() as f32;
        }

        assert!(game.next());

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

        let previous_actions = &game.actions()[..game.actions().len().checked_sub(1).unwrap()];

        for action in previous_actions.iter().copied() {
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
}

#[pymodule]
mod poker_human {
    #[pymodule_export]
    use super::Dataset;
}
