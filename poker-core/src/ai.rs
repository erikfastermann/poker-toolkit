use std::{
    error::Error,
    fmt::{self, Write},
    sync::Arc,
};

use rand::{rngs::StdRng, SeedableRng};

use crate::{
    bitset::Bitset,
    game::{milli_big_blind_to_amount_rounded, Action, Game, Street},
    range::{
        PreFlopAction, PreFlopRangeConfig, PreFlopRangeTable, RangeActionKind, RangeConfigEntry,
        RangeEntry, RangeTable, RangeTableWith, MAX_FREQUENCY,
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
    ) -> Result<(AiAction, RangeConfigEntry, Option<&[RangeTableWith<u16>]>)>;
}

pub struct AlwaysFold;

impl PlayerActionGenerator for AlwaysFold {
    fn update_hero(
        &mut self,
        _game: &Game,
        _log: &mut String,
    ) -> Result<(AiAction, RangeConfigEntry, Option<&[RangeTableWith<u16>]>)> {
        let total_range = RangeTable::FULL.to_frequencies(MAX_FREQUENCY);
        let config = RangeConfigEntry::distribute_action(total_range, RangeActionKind::Fold)?;
        Ok((AiAction::Fold, config, None))
    }
}

pub struct AlwaysCheckCall;

impl PlayerActionGenerator for AlwaysCheckCall {
    fn update_hero(
        &mut self,
        game: &Game,
        _log: &mut String,
    ) -> Result<(AiAction, RangeConfigEntry, Option<&[RangeTableWith<u16>]>)> {
        let total_range = RangeTable::FULL.to_frequencies(MAX_FREQUENCY);
        let action = AiAction::CheckCall.to_range(game)?;
        let config = RangeConfigEntry::distribute_action(total_range, action)?;
        Ok((AiAction::CheckCall, config, None))
    }
}

pub struct AlwaysAllIn;

impl PlayerActionGenerator for AlwaysAllIn {
    fn update_hero(
        &mut self,
        game: &Game,
        _log: &mut String,
    ) -> Result<(AiAction, RangeConfigEntry, Option<&[RangeTableWith<u16>]>)> {
        let total_range = RangeTable::FULL.to_frequencies(MAX_FREQUENCY);
        let action = AiAction::AllIn.to_range(game)?;
        let config = RangeConfigEntry::distribute_action(total_range, action)?;
        Ok((AiAction::AllIn, config, None))
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
    ) -> Result<(AiAction, RangeConfigEntry, Option<&[RangeTableWith<u16>]>)> {
        let range = self.player(game, log)?;
        let action = range.pick(&mut self.rng, game.current_hand().unwrap());
        self.current_ranges[game.current_player().unwrap()] = range.action_range(action).unwrap();

        let action = AiAction::from_range(action, game.big_blind())?;
        Ok((action, range, Some(&self.current_ranges)))
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
        // TODO
        writeln!(log, "post flop currently only check/call")?;

        let total_range = self.current_ranges[game.current_player().unwrap()].clone();
        let action = AiAction::CheckCall.to_range(game)?;
        let config = RangeConfigEntry::distribute_action(total_range, action)?;
        Ok(config)
    }
}
