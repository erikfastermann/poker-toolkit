use std::sync::Arc;

use rand::{rngs::StdRng, SeedableRng};

use crate::{
    game::{milli_big_blind_to_amount_rounded, Game, Street},
    range::{PreFlopAction, PreFlopRangeConfig, RangeEntry},
    rank::Rank,
    result::Result,
};

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
            PreFlopAction::Post { .. } | PreFlopAction::Straddle { .. } => todo!(),
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
    fn player_action(&mut self, game: &Game) -> Result<AiAction>;
}

pub struct AlwaysFold;

impl PlayerActionGenerator for AlwaysFold {
    fn player_action(&mut self, _game: &Game) -> Result<AiAction> {
        Ok(AiAction::Fold)
    }
}

pub struct AlwaysCheckCall;

impl PlayerActionGenerator for AlwaysCheckCall {
    fn player_action(&mut self, _game: &Game) -> Result<AiAction> {
        Ok(AiAction::CheckCall)
    }
}

pub struct AlwaysAllIn;

impl PlayerActionGenerator for AlwaysAllIn {
    fn player_action(&mut self, _game: &Game) -> Result<AiAction> {
        Ok(AiAction::AllIn)
    }
}

pub struct SimpleStrategy {
    rng: StdRng,
    pre_flop_ranges: Arc<PreFlopRangeConfig>,
}

impl PlayerActionGenerator for SimpleStrategy {
    fn player_action(&mut self, game: &Game) -> Result<AiAction> {
        if game.board().street() == Street::PreFlop {
            self.pre_flop(game)
        } else {
            Ok(AiAction::CheckCall) // TODO
        }
    }
}

impl SimpleStrategy {
    pub fn new(pre_flop_ranges: Arc<PreFlopRangeConfig>) -> Self {
        Self {
            rng: StdRng::from_entropy(),
            pre_flop_ranges,
        }
    }

    fn pre_flop(&mut self, game: &Game) -> Result<AiAction> {
        let action = self.pre_flop_inner(game)?;

        let range_entry = RangeEntry::from_hand(game.current_hand().unwrap());
        if action.contains_fold()
            && (range_entry == RangeEntry::paired(Rank::Ace)
                || range_entry == RangeEntry::paired(Rank::King))
        {
            dbg!(range_entry);
            if let Some((_, to)) = game.can_raise() {
                // TODO:
                // The totally not suspicious min raise.
                // Might not be the best choice,
                // should want to call after 3-betting often etc.
                Ok(AiAction::BetRaise(to))
            } else {
                Ok(AiAction::CheckCall)
            }
        } else {
            Ok(action)
        }
    }

    fn pre_flop_inner(&mut self, game: &Game) -> Result<AiAction> {
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
                    dbg!(err);
                    return Ok(AiAction::CheckFold);
                }
            };

        if diff_milli_big_blinds >= 15_000 {
            // Arbitrary choice, in reality this might be way too large in most situations.
            dbg!(diff_milli_big_blinds, game.actions());
            return Ok(AiAction::CheckFold);
        }

        let range_entry = RangeEntry::from_hand(game.current_hand().unwrap());
        let action = range.pick(&mut self.rng, range_entry);
        // TODO:
        // Adjust sizing to bets and handle smaller than min raise.
        Ok(AiAction::from_pre_flop(action, game.big_blind())?)
    }
}
