use std::collections::HashMap;

use poker_core::{
    db::DB,
    game::{Action, Game, Street},
    result::Result,
};

const DB_PATH: &'static str = "../../poker-app/phh.db";

fn main() -> Result<()> {
    let db = DB::open(DB_PATH)?;

    let mut dist = [const { Vec::new() }; Street::COUNT];

    let mut count = 0u64;

    db.hand_data_for_each("SELECT * FROM hands_data", (), |hand_data| {
        if count % 10_000 == 0 {
            println!("{count}");
        }

        let game = Game::from_game_data(&hand_data.data)?;
        process_game(game, &mut dist)?;

        count += 1;
        Ok(true)
    })?;

    println!("{dist:#?}");

    Ok(())
}

type Count = u32;

type Percent = u32;

type Dist = [Vec<(Count, HashMap<Percent, Count>)>; Street::COUNT];

fn process_game(mut game: Game, dist: &mut Dist) -> Result<()> {
    assert_eq!(game.runouts().len(), 1);
    assert!(!matches!(
        game.actions().last(),
        Some(Action::Bet { .. } | Action::Raise { .. })
    ));

    // Start with index one pre flop, because only raising is allowed.
    let mut street_bet_counter = 1usize;

    game.rewind();

    // TODO: Check how many posters / straddlers appear in the handhq data.

    // Skip blinds/straddles.
    while game.next() {
        let action = game.actions().last().unwrap();

        let amount = match action {
            Action::Bet { amount, .. } => *amount,
            Action::Raise { amount, .. } => *amount,
            Action::Flop(_) | Action::Turn(_) | Action::River(_) => {
                street_bet_counter = 0;
                continue;
            }
            _ => continue,
        };

        let pot = game.total_pot();
        let percent_pot = percent_pot(pot, amount);

        let dist_street = &mut dist[game.board().street().to_usize()];

        while dist_street.len() <= street_bet_counter {
            dist_street.push((0, HashMap::new()));
        }

        dist_street[street_bet_counter].0 += 1;
        *dist_street[street_bet_counter]
            .1
            .entry(percent_pot)
            .or_insert(0) += 1;

        street_bet_counter += 1;
    }

    Ok(())
}

fn percent_pot(pot: u32, amount: u32) -> u32 {
    // TODO:
    // Is this calculation sensible?
    // Raises are typically calculated differently.

    assert_ne!(pot, 0);
    let percent = f64::from(amount) / f64::from(pot);
    let percent = (percent * 100.0).round();
    assert!(percent >= 0.0 && percent <= f64::from(u32::MAX));
    let percent = percent as u32;

    percent
}
