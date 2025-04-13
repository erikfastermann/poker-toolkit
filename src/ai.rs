use crate::{
    game::{Action, Game},
    gui::PlayerActionGenerator,
    result::Result,
};

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
