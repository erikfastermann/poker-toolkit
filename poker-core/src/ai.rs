use crate::{
    game::{Action, Game, Street},
    result::Result,
};

pub trait PlayerActionGenerator {
    fn player_action(&mut self, game: &Game) -> Result<Action>;
}

pub struct AlwaysFold;

impl PlayerActionGenerator for AlwaysFold {
    fn player_action(&mut self, game: &Game) -> Result<Action> {
        let player = u8::try_from(game.current_player().unwrap()).unwrap();
        Ok(Action::Fold(player))
    }
}

pub struct AlwaysCheckCall;

impl PlayerActionGenerator for AlwaysCheckCall {
    fn player_action(&mut self, game: &Game) -> Result<Action> {
        let player = u8::try_from(game.current_player().unwrap()).unwrap();
        if game.can_check() {
            Ok(Action::Check(player))
        } else if let Some(amount) = game.can_call() {
            Ok(Action::Call { player, amount })
        } else {
            unreachable!()
        }
    }
}

pub struct SimpleStrategy;

impl PlayerActionGenerator for SimpleStrategy {
    fn player_action(&mut self, game: &Game) -> Result<Action> {
        let player = u8::try_from(game.current_player().unwrap()).unwrap();

        if game.board().street() == Street::PreFlop {
            return self.pre_flop(game);
        }

        if game.can_check() {
            Ok(Action::Check(player))
        } else {
            Ok(Action::Fold(player))
        }
    }
}

impl SimpleStrategy {
    fn pre_flop(&self, game: &Game) -> Result<Action> {
        let player = u8::try_from(game.current_player().unwrap()).unwrap();

        let actions = game.actions_in_street();
        todo!();

        if game.can_check() {
            Ok(Action::Check(player))
        } else {
            Ok(Action::Fold(player))
        }
    }
}
