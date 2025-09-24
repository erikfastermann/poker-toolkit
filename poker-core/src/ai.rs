use std::{
    cmp::{self, Ordering},
    error::Error,
    fmt::{self, Write},
    iter,
    sync::Arc,
};

use rand::{
    rngs::{SmallRng, StdRng},
    SeedableRng,
};

use crate::{
    bitset::Bitset,
    equity::EquityTable,
    game::{milli_big_blind_to_amount_rounded, Action, Game, Street},
    hand::Hand,
    range::{
        range_remove_cards, PreFlopAction, PreFlopRangeConfig, PreFlopRangeTable, RangeAction,
        RangeActionKind, RangeConfigEntry, RangeEntry, RangeTable, RangeTableWith, MAX_FREQUENCY,
    },
    rank::Rank,
    result::Result,
};

#[derive(Debug)]
pub struct ErrorRangeUnimplemented;

impl fmt::Display for ErrorRangeUnimplemented {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self, f)
    }
}

impl Error for ErrorRangeUnimplemented {}

// Post / Straddle excluded for now.
#[derive(Debug, Clone, Copy)]
pub enum AiAction {
    Fold,
    CheckFold,
    CheckCall,
    BetRaise(u32),
    AllIn,
}

impl AiAction {
    pub fn from_pre_flop(action: PreFlopAction, big_blind: u32) -> Result<Self> {
        match action {
            PreFlopAction::Post { .. } | PreFlopAction::Straddle { .. } => {
                Err("ai action from pre flop: straddle and post currently not supported".into())
            }
            PreFlopAction::Fold => Ok(AiAction::Fold),
            PreFlopAction::Check => Ok(AiAction::CheckFold),
            PreFlopAction::Call => Ok(AiAction::CheckCall),
            PreFlopAction::Raise(amount) => {
                if let Some(amount) = milli_big_blind_to_amount_rounded(amount, big_blind) {
                    Ok(AiAction::BetRaise(amount))
                } else {
                    Err("ai action from pre flop action: conversion of raise amount failed".into())
                }
            }
        }
    }

    pub fn from_range(action: RangeActionKind, big_blind: u32) -> Result<Self> {
        match action {
            RangeActionKind::Post { .. } | RangeActionKind::Straddle { .. } => {
                Err("ai action from range: straddle and post currently not supported".into())
            }
            RangeActionKind::Fold => Ok(AiAction::Fold),
            RangeActionKind::Check => Ok(AiAction::CheckFold),
            RangeActionKind::Call => Ok(AiAction::CheckCall),
            RangeActionKind::Bet(amount) | RangeActionKind::Raise(amount) => {
                if let Some(amount) = milli_big_blind_to_amount_rounded(amount, big_blind) {
                    Ok(AiAction::BetRaise(amount))
                } else {
                    Err("ai action from range: conversion of bet or raise amount failed".into())
                }
            }
        }
    }

    pub fn to_range(self, game: &Game) -> Result<RangeActionKind> {
        if game.current_player().is_none() {
            return Err("ai action to range: game is not a player decision point".into());
        }

        match self {
            AiAction::Fold => Ok(RangeActionKind::Fold),
            AiAction::CheckFold => {
                if game.can_check() {
                    Ok(RangeActionKind::Check)
                } else {
                    Ok(RangeActionKind::Fold)
                }
            }
            AiAction::CheckCall => {
                if game.can_check() {
                    Ok(RangeActionKind::Check)
                } else if game.can_call().is_some() {
                    Ok(RangeActionKind::Call)
                } else {
                    Err("ai action to range: check/call not possible".into())
                }
            }
            AiAction::BetRaise(amount) => {
                if game.can_bet().is_some() {
                    Ok(RangeActionKind::Bet(
                        game.amount_to_milli_big_blinds_rounded(amount),
                    ))
                } else if game.can_raise().is_some() {
                    Ok(RangeActionKind::Raise(
                        game.amount_to_milli_big_blinds_rounded(amount),
                    ))
                } else {
                    Err("ai action to range: bet/raise not possible".into())
                }
            }
            AiAction::AllIn => {
                if let Some(amount) = game.can_all_in() {
                    Self::BetRaise(amount).to_range(game)
                } else {
                    Err("ai action to range: all-in not possible".into())
                }
            }
        }
    }

    pub fn contains_fold(self) -> bool {
        matches!(self, AiAction::Fold | AiAction::CheckFold)
    }

    pub fn apply_to_game(self, game: &mut Game) -> Result<()> {
        match self {
            AiAction::Fold => game.fold(),
            AiAction::CheckFold => {
                if game.can_check() {
                    game.check()
                } else {
                    game.fold()
                }
            }
            AiAction::CheckCall => {
                if game.can_check() {
                    game.check()
                } else if game.can_call().is_some() {
                    game.call()
                } else {
                    Err("apply ai action: check/call not possible".into())
                }
            }
            AiAction::BetRaise(amount) => {
                if game.can_bet().is_some() {
                    game.bet(amount)
                } else if game.can_raise().is_some() {
                    game.raise(amount)
                } else {
                    Err("apply ai action: bet/raise not possible".into())
                }
            }
            AiAction::AllIn => game.all_in(),
        }
    }
}

pub trait PlayerActionGenerator {
    fn update_villain(&mut self, _game: &Game, log: &mut String) -> Result<()> {
        writeln!(log, "not implemented")?;
        Ok(())
    }

    fn update_hero(
        &mut self,
        game: &Game,
        log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )>;

    fn custom_show_or_muck(&self) -> bool {
        false
    }

    fn show_or_muck(&self, _game: &Game, _log: &mut String) -> Result<Option<Hand>> {
        Err("custom show or muck not implemented".into())
    }
}

pub struct AlwaysFold;

impl PlayerActionGenerator for AlwaysFold {
    fn update_hero(
        &mut self,
        _game: &Game,
        _log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        let total_range = RangeTable::FULL.to_frequencies(MAX_FREQUENCY);
        let config = RangeConfigEntry::distribute_action(total_range, RangeActionKind::Fold)?;
        Ok((AiAction::Fold, Some(config), None))
    }
}

pub struct AlwaysCheckCall;

impl PlayerActionGenerator for AlwaysCheckCall {
    fn update_hero(
        &mut self,
        game: &Game,
        _log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        let total_range = RangeTable::FULL.to_frequencies(MAX_FREQUENCY);
        let action = AiAction::CheckCall.to_range(game)?;
        let config = RangeConfigEntry::distribute_action(total_range, action)?;
        Ok((AiAction::CheckCall, Some(config), None))
    }
}

pub struct AlwaysAllIn;

impl PlayerActionGenerator for AlwaysAllIn {
    fn update_hero(
        &mut self,
        game: &Game,
        _log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        let total_range = RangeTable::FULL.to_frequencies(MAX_FREQUENCY);
        let action = AiAction::AllIn.to_range(game)?;
        let config = RangeConfigEntry::distribute_action(total_range, action)?;
        Ok((AiAction::AllIn, Some(config), None))
    }
}

pub struct SimpleStrategy {
    rng: StdRng,
    current_ranges: Vec<RangeTableWith<u16>>,
    pre_flop_ranges: Arc<PreFlopRangeConfig>,
    // 256 pre flop actions should be enough for anyone.
    pre_flop_fold_replace: Bitset<32>,
}

impl PlayerActionGenerator for SimpleStrategy {
    fn update_villain(&mut self, game: &Game, log: &mut String) -> Result<()> {
        // Using self range calculation for enemy.

        let action = game.actions().last().copied().unwrap();
        let action = RangeActionKind::from_game_action(game, action)?;

        let mut game = game.clone();
        game.undo()?;
        let player = game.current_player().unwrap();

        let range = self.player(&game, log)?;

        let range = if let Some(range) = range.action_range(action) {
            range
        } else if game.board().street() == Street::PreFlop {
            // Player action not in configured pre flop chart.
            self.villain_unexpected_pre_flop_action(game, action, &range, log)?
        } else {
            return Err("unexpected post flop action".into()); // TODO
        };

        self.current_ranges[player] = range;

        Ok(())
    }

    fn update_hero(
        &mut self,
        game: &Game,
        log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        let range = self.player(game, log)?;
        let action = range.pick(&mut self.rng, game.current_hand().unwrap());
        self.current_ranges[game.current_player().unwrap()] = range.action_range(action).unwrap();

        let action = AiAction::from_range(action, game.big_blind())?;
        Ok((action, Some(range), Some(&self.current_ranges)))
    }
}

impl SimpleStrategy {
    pub fn new(pre_flop_ranges: Arc<PreFlopRangeConfig>) -> Self {
        Self {
            rng: StdRng::from_entropy(),
            pre_flop_ranges,
            current_ranges: vec![RangeTable::FULL.to_frequencies(MAX_FREQUENCY); Game::MAX_PLAYERS],
            pre_flop_fold_replace: Bitset::EMPTY,
        }
    }

    fn player(&self, game: &Game, log: &mut String) -> Result<RangeConfigEntry> {
        if game.board().street() == Street::PreFlop {
            self.pre_flop(game, log)
        } else {
            self.post_flop(game, log)
        }
    }

    fn pre_flop(&self, game: &Game, log: &mut String) -> Result<RangeConfigEntry> {
        let mut config = self.pre_flop_inner(game, log)?;

        let aces_kings = [
            RangeEntry::paired(Rank::Ace),
            RangeEntry::paired(Rank::King),
        ];

        for entry in aces_kings {
            if config.total_entry_frequency(entry) != 0.0
                && config.entry_frequency(RangeActionKind::Fold, entry) != 0.0
            {
                // TODO:
                // The totally not suspicious min raise.
                // Might not be the best choice,
                // should want to call after 3-betting often etc.

                let action = if let Some((_, to)) = game.can_raise() {
                    AiAction::BetRaise(to)
                } else {
                    AiAction::CheckCall
                };

                config.update_entry_only_action(entry, action.to_range(game)?)?;
                writeln!(log, "changed action for {entry}: {action:?}")?;
            }
        }

        Ok(config)
    }

    fn pre_flop_inner(&self, game: &Game, log: &mut String) -> Result<RangeConfigEntry> {
        // TODO: Handle unexpected sizings.

        if game.actions().len() > self.pre_flop_fold_replace.cap() {
            return Err("pre flop actions would overflow fold replace bitset".into());
        }

        // TODO: Handle posts / straddles.
        let best_fit_result = self
            .pre_flop_ranges
            .by_game_best_fit_raise_simple(game, |index| {
                let action = game.actions()[index];
                if self.pre_flop_fold_replace.has(index) {
                    // Replace unexpected calls with folds for limpers etc.,
                    // pretty crude but works for now.
                    let player = u8::try_from(action.player().unwrap()).unwrap();
                    Action::Fold(player)
                } else {
                    action
                }
            });

        let (range, diff_milli_big_blinds) = match best_fit_result {
            Ok(range) => range,
            Err(err) => {
                // Can also happen if all other players limped and we are in the big blind.
                //
                // TODO: Custom range in this case.

                // TODO: Totally breaks when calling 3-bets+.

                writeln!(
                    log,
                    "an error occurred while calculating range best fit: {err}"
                )?;

                // Just check/fold otherwise if the config does not match or another error occurred.
                let range = self.current_range_check_fold(game)?;
                return Ok(range);
            }
        };

        if diff_milli_big_blinds >= 10_000 {
            // TODO: Arbitrary choice, in reality this is way too large in most situations.
            writeln!(
                log,
                "diff milli big blinds too big: {diff_milli_big_blinds}"
            )?;
            return self.current_range_check_fold(game);
        }

        // TODO: Increase size when using fold replace.
        let mut range = range.to_full_range();

        if let Some((_, to)) = game.can_raise() {
            let to = game.amount_to_milli_big_blinds_rounded(to);
            range.update_min_raise(to)?;
        }

        Ok(range)
    }

    fn current_range_check_fold(&self, game: &Game) -> Result<RangeConfigEntry> {
        RangeConfigEntry::distribute_action(
            self.current_ranges[game.current_player().unwrap()].clone(),
            AiAction::CheckFold.to_range(game)?,
        )
    }

    fn villain_unexpected_pre_flop_action(
        &mut self,
        game: Game,
        action: RangeActionKind,
        current_range: &RangeConfigEntry,
        log: &mut String,
    ) -> Result<RangeTableWith<u16>> {
        // Implementation of this function is really confusing when using weird
        // pre flop ranges.

        if let RangeActionKind::Raise(to) = action {
            let best_raise_to = current_range
                .action_kinds()
                .filter_map(|action| match action {
                    RangeActionKind::Raise(current_to) => Some(current_to),
                    _ => None,
                })
                .min_by_key(|current_to| current_to.abs_diff(to));

            let Some(best_raise_to) = best_raise_to else {
                return Err("villain unexpected pre flop action: raise action \
                    but current range does not contain raises"
                    .into());
            };

            let range = current_range
                .action_range(RangeActionKind::Raise(best_raise_to))
                .unwrap();
            return Ok(range);
        }

        let action_index = game.actions().len();
        let range = self.villain_unexpected_pre_flop_call(game, action, current_range, log)?;
        self.pre_flop_fold_replace.set(action_index);
        Ok(range)
    }

    fn villain_unexpected_pre_flop_call(
        &mut self,
        mut game: Game,
        action: RangeActionKind,
        current_range: &RangeConfigEntry,
        log: &mut String,
    ) -> Result<RangeTableWith<u16>> {
        if action != RangeActionKind::Call {
            return Err("villain unexpected pre flop action: only call supported".into());
        }

        let has_raise = game
            .actions()
            .iter()
            .any(|action| matches!(action, Action::Raise { .. }));
        if !has_raise {
            writeln!(log, "unexpected call: using limping range")?;

            // Default arbitrary limping range for all positions and previous limpers.
            const PRE_FLOP_LIMPING_RANGE: &str =
                "22+,A2s+,K2s+,Q2s+,J2s+,T2s+,92s+,82s+,72s+,62s+,52s+,42s+,32s+,\
                A2o+,K5o+,Q8o+,J8o+,T7o+,97o+,86o+,75o+,64o+,53o+,42o+,32o+";
            let range = PreFlopRangeTable::parse(PRE_FLOP_LIMPING_RANGE).unwrap();
            let range = RangeTable::from_range_table(&range).to_frequencies(MAX_FREQUENCY);
            return Ok(range);
        }

        game.fold()?;

        if game.current_player().is_none() {
            // Use the smallest raise of the last found range if this action terminates street.
            writeln!(
                log,
                "unexpected call: player terminates street, using raising range"
            )?;

            return Self::range_smallest_raise(current_range).map_err(|err| {
                format!("villain unexpected pre flop action: villain fold terminates street: {err}")
                    .into()
            });
        }

        // For other unexpected calls, we try call from the next position.
        let range = self.pre_flop(&game, log)?;
        if let Some(next_calling_range) = range.action_range(RangeActionKind::Call) {
            writeln!(log, "unexpected call: using next player calling range")?;

            Ok(next_calling_range)
        } else {
            // If that fails, we use the raising range of the next position.

            writeln!(log, "unexpected call: using next player raising range")?;

            return Self::range_smallest_raise(current_range)
                .map_err(|err| format!("villain unexpected pre flop action: {err}").into());
        }
    }

    fn range_smallest_raise(current_range: &RangeConfigEntry) -> Result<RangeTableWith<u16>> {
        let smallest_raise = current_range
            .actions()
            .iter()
            .filter(|action| matches!(action.action(), RangeActionKind::Raise(_)))
            .min_by_key(|action| {
                if let RangeActionKind::Raise(to) = action.action() {
                    to
                } else {
                    unreachable!()
                }
            });

        let Some(smallest_raise) = smallest_raise else {
            return Err("found range has no raises".into());
        };

        return Ok(smallest_raise.range().clone());
    }

    fn post_flop(&self, game: &Game, log: &mut String) -> Result<RangeConfigEntry> {
        let current_player = game.current_player().unwrap();
        let current_stack = game.current_stack().unwrap();
        let previous_street_stack = game.previous_street_stack().unwrap();
        let community_cards = game.board().cards_set();

        let (players, ranges): (Vec<_>, Vec<_>) = game
            .players_not_folded()
            .map(|player| {
                let mut range = self.current_ranges[player].clone();
                range_remove_cards(&mut range, community_cards);
                (player, range)
            })
            .unzip();

        let current_player_range_position =
            players.iter().position(|p| *p == current_player).unwrap();

        let simulate_rounds = 10_000 * u64::try_from(game.players_not_folded().count()).unwrap();

        // Use this rng to get deterministic results.
        let mut rng = SmallRng::seed_from_u64(42);

        let equities = EquityTable::simulate_frequencies_with(
            community_cards,
            &ranges,
            simulate_rounds,
            &mut rng,
        );

        let Some(equities) = equities else {
            writeln!(log, "equity simulation returned none")?;

            return self.current_range_check_fold(game);
        };

        for (player, equity) in players.iter().copied().zip(equities.iter()) {
            writeln!(
                log,
                "player {}: {}",
                game.player_name(player),
                equity.total_equity()
            )?;
        }

        let current_range = &ranges[current_player_range_position];
        let current_equities = &equities[current_player_range_position];

        let mut hands: Vec<_> = RangeTable::from_frequencies_not_zero(current_range)
            .into_iter()
            // Exclude blocked hands.
            .filter(|hand| !community_cards.has(hand.high()) && !community_cards.has(hand.low()))
            .collect();

        hands.sort_by(|a, b| {
            let a = current_equities.equity(*a);
            let b = current_equities.equity(*b);

            a.equity_percent()
                .partial_cmp(&b.equity_percent())
                .unwrap_or(Ordering::Less)
                .then_with(|| {
                    a.win_percent()
                        .partial_cmp(&b.win_percent())
                        .unwrap_or(Ordering::Less)
                })
        });

        // TODO: Round if all-in size is close.

        // TODO: Bet/raise draws more as a bluff.

        // TODO: Mixed strategy

        // TODO: Villain modelling

        if let Some(call_amount) = game.can_call() {
            if let Some((_, min_raise_to)) = game.can_raise() {
                let raise_size = (f64::from(game.total_pot()) * 0.7) as u32;
                let raise_size =
                    cmp::min(cmp::max(raise_size, min_raise_to), previous_street_stack);

                // Super simple equity based calculation.
                // TODO: Not correct with to.
                let raise_pot_odds =
                    f64::from(raise_size) / f64::from(raise_size + game.total_pot());

                let (range_bottom, range_middle, range_top) =
                    Self::range_bottom_middle_top(&hands, current_equities, current_range, 0.85);

                writeln!(
                    log,
                    "raise: size={raise_size} pot_odds={raise_pot_odds} bottom={} middle={} top={}",
                    range_bottom.len(),
                    range_middle.len(),
                    range_top.len()
                )?;

                let raise_size = game.amount_to_milli_big_blinds_rounded(raise_size);

                let raising_hands = range_top
                    .iter()
                    .copied()
                    .chain(range_bottom.iter().copied());
                let raising_range =
                    RangeTable::from_hands(raising_hands)?.to_frequencies(MAX_FREQUENCY);
                let raising_action = RangeAction::new(
                    RangeActionKind::Raise(raise_size),
                    current_range,
                    raising_range,
                );

                // Does not consider multi-way spots or future actions.
                let pot_odds = f64::from(call_amount) / f64::from(call_amount + game.total_pot());

                let equity_cutoff = range_middle
                    .iter()
                    .position(|hand| current_equities.equity_percent(*hand) >= pot_odds)
                    .unwrap_or(range_middle.len());

                writeln!(
                    log,
                    "raise: call: pot_odds={pot_odds} ratio={}/{}",
                    range_middle.len() - equity_cutoff,
                    range_middle.len()
                )?;

                let calling_range =
                    RangeTable::from_hands(range_middle[equity_cutoff..].iter().copied())?
                        .to_frequencies(MAX_FREQUENCY);
                let calling_action =
                    RangeAction::new(RangeActionKind::Call, current_range, calling_range);

                RangeConfigEntry::new(current_range.clone(), vec![calling_action, raising_action])
            } else {
                // Terminates hand for us, only use pot odds.

                // Does not consider multi-way spots.
                let pot_odds = f64::from(call_amount) / f64::from(call_amount + game.total_pot());

                let equity_cutoff = hands
                    .iter()
                    .position(|hand| current_equities.equity_percent(*hand) >= pot_odds)
                    .unwrap_or(hands.len());

                writeln!(
                    log,
                    "raise: call only: pot_odds={pot_odds} ratio={}/{}",
                    hands.len() - equity_cutoff,
                    hands.len()
                )?;

                let calling_range = RangeTable::from_hands(hands[equity_cutoff..].iter().copied())?
                    .to_frequencies(MAX_FREQUENCY);
                let calling_action =
                    RangeAction::new(RangeActionKind::Call, current_range, calling_range);

                RangeConfigEntry::new(current_range.clone(), vec![calling_action])
            }
        } else if game.can_check() {
            if game.can_bet().is_none() {
                return Err("ai: post flop: invalid game state: cannot check and bet".into());
            }

            let (bet_size_percent, required_equity) = match game.board().street() {
                Street::PreFlop => unreachable!(),
                Street::Flop => (0.3, 0.5),
                Street::Turn => (0.7, 0.8),
                Street::River => (0.5, 0.6),
            };

            let bet_size = (f64::from(game.total_pot()) * bet_size_percent) as u32;
            let bet_size = cmp::min(cmp::max(bet_size, game.big_blind()), current_stack);

            // Super simple equity based calculation.
            let pot_odds = f64::from(bet_size) / f64::from(bet_size + game.total_pot());

            let (range_bottom, range_middle, range_top) = Self::range_bottom_middle_top(
                &hands,
                current_equities,
                current_range,
                required_equity,
            );

            writeln!(
                log,
                "bet: size={bet_size} pot_odds={pot_odds} bottom={} middle={} top={}",
                range_bottom.len(),
                range_middle.len(),
                range_top.len()
            )?;

            let bet_size = game.amount_to_milli_big_blinds_rounded(bet_size);

            let betting_hands = range_top
                .iter()
                .copied()
                .chain(range_bottom.iter().copied());
            let betting_range =
                RangeTable::from_hands(betting_hands)?.to_frequencies(MAX_FREQUENCY);
            let betting_action =
                RangeAction::new(RangeActionKind::Bet(bet_size), current_range, betting_range);

            let checking_range =
                RangeTable::from_hands(range_middle.iter().copied())?.to_frequencies(MAX_FREQUENCY);
            let checking_action =
                RangeAction::new(RangeActionKind::Check, current_range, checking_range);

            RangeConfigEntry::new(current_range.clone(), vec![checking_action, betting_action])
        } else {
            return Err("ai: post flop: invalid game state".into());
        }
    }

    fn range_bottom_middle_top<'a>(
        hands_sorted_by_equity: &'a [Hand],
        equities: &EquityTable,
        range: &RangeTableWith<u16>,
        required_equity: f64,
    ) -> (&'a [Hand], &'a [Hand], &'a [Hand]) {
        assert!(required_equity >= 0.0 && required_equity <= 1.0);

        let mut range_top_end_sum = 0u32;
        let mut range_top_end = 0usize;

        for (index, hand) in hands_sorted_by_equity.iter().copied().enumerate().rev() {
            if equities.equity_percent(hand) < required_equity {
                range_top_end = index + 1;
                break;
            }

            range_top_end_sum += u32::from(range[hand]);
        }

        let mut current_range_sum = 0u32;
        let mut range_bottom_end = hands_sorted_by_equity.len();

        for (index, hand) in hands_sorted_by_equity.iter().copied().enumerate() {
            current_range_sum += u32::from(range[hand]);

            if current_range_sum >= range_top_end_sum {
                range_bottom_end = index;
                break;
            }
        }

        if range_bottom_end > range_top_end {
            range_bottom_end = range_top_end;
        }

        let range_bottom = &hands_sorted_by_equity[..range_bottom_end];
        let range_middle = &hands_sorted_by_equity[range_bottom_end..range_top_end];
        let range_top = &hands_sorted_by_equity[range_top_end..];

        (range_bottom, range_middle, range_top)
    }
}

pub struct EquityStrategy {
    current_range: RangeTableWith<u16>,
}

impl EquityStrategy {
    pub fn new() -> Self {
        Self {
            current_range: RangeTable::FULL.to_frequencies(MAX_FREQUENCY),
        }
    }
}

impl PlayerActionGenerator for EquityStrategy {
    fn update_villain(&mut self, _game: &Game, _log: &mut String) -> Result<()> {
        Ok(())
    }

    fn update_hero(
        &mut self,
        game: &Game,
        _log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        // Possible optimizations:
        // - Cache pre flop frequencies
        // - Pre flop with ranges where possible
        // - No limping pre flop

        const SIZE_1_PERCENT: f64 = 1.0 / 3.0;
        const SIZE_2_PERCENT: f64 = 0.8;

        let previous_bet_raise_count: u32 = game
            .actions()
            .iter()
            .filter(|action| matches!(action, Action::Bet { .. } | Action::Raise { .. }))
            .count()
            .try_into()
            .unwrap();

        let equity_scale_factor =
            previous_bet_raise_count + game.total_pot() / game.big_blind() / 10 + 1;

        let player = game.current_player().unwrap();

        let not_folded = game.players_not_folded().count();
        assert!(not_folded >= 2);

        let ranges: Vec<_> = iter::once(self.current_range.clone())
            .chain(
                iter::repeat(RangeTable::FULL.to_frequencies(MAX_FREQUENCY)).take(not_folded - 1),
            )
            .collect();

        let board = game.board().cards_set();
        let equity = EquityTable::simulate_frequencies_with(
            board,
            &ranges,
            100_000,
            &mut SmallRng::seed_from_u64(42), // deterministic
        )
        .unwrap();

        let equity = &equity[0];

        let mut check_fold = RangeTableWith::default();
        let mut check_call = RangeTableWith::default();
        let mut bet_raise_1 = RangeTableWith::default();
        let mut bet_raise_2 = RangeTableWith::default();

        let mut bet_raise_1_count = 0usize;
        let mut bet_raise_2_count = 0usize;
        let mut would_fold = Vec::new();

        for hand in Hand::all() {
            if hand.to_cards().overlaps(board) {
                self.current_range[hand] = 0;
            }

            let scaled_equity = equity
                .equity_percent(hand)
                .powf(f64::from(equity_scale_factor));

            if scaled_equity > 0.75 {
                bet_raise_2[hand] = MAX_FREQUENCY;
                bet_raise_2_count += 1;
            } else if scaled_equity > 0.5 {
                bet_raise_1[hand] = MAX_FREQUENCY;
                bet_raise_1_count += 1;
            } else if scaled_equity > 0.25 {
                check_call[hand] = MAX_FREQUENCY;
            } else {
                check_fold[hand] = MAX_FREQUENCY;
                would_fold.push(hand);
            }
        }

        would_fold.sort_by_key(|hand| (equity.equity_percent(*hand) * 10_000.0).round() as u16);

        // Add some bluffs, specific number has no significance.

        for hand in would_fold.iter().copied().rev().take(bet_raise_2_count / 3) {
            bet_raise_2[hand] = MAX_FREQUENCY;
            check_fold[hand] = 0;
        }

        for hand in would_fold
            .iter()
            .copied()
            .rev()
            .skip(bet_raise_2_count / 3)
            .take(bet_raise_1_count / 3)
        {
            bet_raise_1[hand] = MAX_FREQUENCY;
            check_fold[hand] = 0;
        }

        let call_amount = game.can_call().unwrap_or(0);
        let pot_with_call = game.total_pot().checked_add(call_amount).unwrap();

        let raise_offset = game
            .invested_in_street(player)
            .checked_add(call_amount)
            .unwrap();

        // TODO: Remove with AiAction raise amount.
        let min_bet_raise = game
            .can_bet()
            .or_else(|| game.can_raise().map(|(_, to)| to))
            .unwrap_or(0);

        // TODO: Use amount here with AiAction raise amount.
        let max_bet_raise = game.previous_street_stack().unwrap();

        // Should not overflow a u32, the percentages are smaller than one.
        // TODO: Use amount here with AiAction raise amount, can remove raise_offset.

        let size_1 = (f64::from(pot_with_call) * SIZE_1_PERCENT).round() as u32;
        let size_1 = size_1.checked_add(raise_offset).unwrap();
        let size_1 = cmp::max(cmp::min(size_1, max_bet_raise), min_bet_raise);

        let size_2 = (f64::from(pot_with_call) * SIZE_2_PERCENT).round() as u32;
        let size_2 = size_2.checked_add(raise_offset).unwrap();
        let size_2 = cmp::max(cmp::min(size_2, max_bet_raise), min_bet_raise);

        let bet_raise_actions = if game.can_bet().is_none() && game.can_raise().is_none() {
            for hand in Hand::all() {
                check_call[hand] = check_call[hand].checked_add(bet_raise_1[hand]).unwrap();
                check_call[hand] = check_call[hand].checked_add(bet_raise_2[hand]).unwrap();

                bet_raise_1[hand] = 0;
                bet_raise_2[hand] = 0;
            }

            Vec::new()
        } else {
            if size_1 == size_2 {
                for hand in Hand::all() {
                    bet_raise_1[hand] = bet_raise_1[hand].checked_add(bet_raise_2[hand]).unwrap();
                    bet_raise_2[hand] = 0;
                }
            }

            let bet_raise_1_action = AiAction::BetRaise(size_1);
            let bet_raise_1 = RangeAction::new(
                bet_raise_1_action.to_range(game).unwrap(),
                &self.current_range,
                bet_raise_1,
            );

            let bet_raise_2_action = AiAction::BetRaise(size_2);
            let bet_raise_2 = RangeAction::new(
                bet_raise_2_action.to_range(game).unwrap(),
                &self.current_range,
                bet_raise_2,
            );

            if size_1 == size_2 {
                vec![bet_raise_1]
            } else {
                vec![bet_raise_1, bet_raise_2]
            }
        };

        let check_actions = if game.can_check() {
            for hand in Hand::all() {
                check_fold[hand] = check_fold[hand].checked_add(check_call[hand]).unwrap();
                check_call[hand] = 0;
            }

            let check = RangeAction::new(RangeActionKind::Check, &self.current_range, check_fold);
            vec![check]
        } else {
            let fold = RangeAction::new(RangeActionKind::Fold, &self.current_range, check_fold);
            let call = RangeAction::new(RangeActionKind::Call, &self.current_range, check_call);
            vec![fold, call]
        };

        let actions = check_actions.into_iter().chain(bet_raise_actions).collect();
        let config = RangeConfigEntry::new(self.current_range.clone(), actions)?;

        let action = config.pick(&mut rand::thread_rng(), game.current_hand().unwrap());
        self.current_range = config.action_range(action).unwrap();

        let action = AiAction::from_range(action, game.big_blind()).unwrap();

        let action = if let AiAction::BetRaise(amount) = action {
            // Avoid rounding issues.
            let size = [size_1, size_2]
                .into_iter()
                .min_by_key(|size| size.abs_diff(amount))
                .unwrap();

            AiAction::BetRaise(size)
        } else {
            action
        };

        Ok((action, Some(config), None))
    }
}
