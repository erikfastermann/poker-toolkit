use std::{
    error::Error,
    fmt::{self, Write},
    sync::Arc,
};

use rand::{rngs::StdRng, SeedableRng};

use crate::{
    game::{milli_big_blind_to_amount_rounded, Game, Street},
    range::{
        PreFlopAction, PreFlopRangeConfig, RangeActionKind, RangeConfigEntry, RangeEntry,
        RangeTable, RangeTableWith, MAX_FREQUENCY,
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
        writeln!(log, "Not implemented")?;
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
}

impl PlayerActionGenerator for SimpleStrategy {
    fn update_villain(&mut self, game: &Game, log: &mut String) -> Result<()> {
        // Using self range calculation for enemy.

        let action = game.actions().last().copied().unwrap();
        let action = RangeActionKind::from_game_action(game, action)?;

        let mut game = game.clone();
        assert!(game.previous());

        let range = self.player(&game, log)?;
        self.current_ranges[game.current_player().unwrap()] = range.action_range(action).unwrap();

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
                writeln!(log, "Pre Flop: Changed action for {entry}: {action:?}")?;
            }
        }

        Ok(config)
    }

    fn pre_flop_inner(&self, game: &Game, log: &mut String) -> Result<RangeConfigEntry> {
        // TODO:
        // Custom pre flop logic and adaptation for things like limping,
        // unexpected calls, crazy sizings.

        let (range, diff_milli_big_blinds) =
            match self.pre_flop_ranges.by_game_best_fit_raise_simple(game) {
                Ok(range_diff) => range_diff,
                // Just fold if the config does not match or another error occurred.
                // Might be confusing, if the errors are just eaten by this function
                // without any feedback.

                // TODO:
                // Ranges might have random holes that can totally happen in real life.
                // Also stuff like limping. Use custom logic here.
                Err(err) => {
                    writeln!(
                        log,
                        "Pre Flop: An error occurred while calculating range best fit: {err}"
                    )?;
                    return self.current_range_check_fold(game);
                }
            };

        if diff_milli_big_blinds >= 15_000 {
            // TODO: Arbitrary choice, in reality this might be way too large in most situations.
            writeln!(
                log,
                "Pre Flop: Diff milli big blinds too big: {diff_milli_big_blinds}"
            )?;
            return self.current_range_check_fold(game);
        }

        Ok(range.to_full_range())
    }

    fn current_range_check_fold(&self, game: &Game) -> Result<RangeConfigEntry> {
        RangeConfigEntry::distribute_action(
            self.current_ranges[game.current_player().unwrap()].clone(),
            AiAction::CheckFold.to_range(game)?,
        )
    }

    fn post_flop(&self, game: &Game, log: &mut String) -> Result<RangeConfigEntry> {
        // TODO
        writeln!(log, "Post Flop: Currently only check/call")?;

        let total_range = self.current_ranges[game.current_player().unwrap()].clone();
        let action = AiAction::CheckCall.to_range(game)?;
        let config = RangeConfigEntry::distribute_action(total_range, action)?;
        Ok(config)
    }
}
