use std::collections::HashMap;

use poker_core::{
    db::DB,
    game::{Action, Amount, Game, Street},
    result::Result,
};

const DB_PATH: &'static str = "../../poker-app/phh_full.db";

fn main() -> Result<()> {
    let db = DB::open(DB_PATH)?;

    let mut dist = [const { Vec::new() }; Street::COUNT];

    let mut count = 0u64;

    db.hand_data_for_each("SELECT * FROM hands_data", (), |hand_data| {
        if count % 10_000 == 0 {
            eprintln!("{count}");
        }

        let game = Game::from_game_data(&hand_data.data)?;
        process_game(game, &mut dist)?;

        count += 1;
        Ok(true)
    })?;

    println!("{}", serde_json::to_string(&dist)?);

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

    // No posters / straddlers appear in the handhq data.
    // This is weird, but ignore that for now.

    // Skip blinds/straddles.
    while game.can_next() {
        let pot = game.total_pot();
        let call_amount = game.can_call().unwrap_or(Amount::ZERO);

        assert!(game.next());

        let action = game.actions().last().copied().unwrap();

        let (player, amount) = match action {
            Action::Bet { player, amount, .. } => (player, amount),
            Action::Raise { player, amount, .. } => (player, amount),
            Action::Flop(_) | Action::Turn(_) | Action::River(_) => {
                street_bet_counter = 0;
                continue;
            }
            _ => continue,
        };

        let is_all_in = game.current_stacks()[usize::from(player)] == Amount::ZERO;

        let percent_pot = if is_all_in {
            u32::MAX
        } else {
            percent_pot(pot, call_amount, amount)
        };

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

fn percent_pot(pot: Amount, call_amount: Amount, amount: Amount) -> u32 {
    // We use the calculation for bets/raises, giving the percentage of the pot.
    // This is typically used to give the caller specific pot odds.
    // Often poker software implements them with configurable percent buttons,
    // although sometimes the calculation is different.
    // But I think this is the best solution to abstract the sizes.

    assert_ne!(pot, Amount::ZERO);
    let pot_with_call = pot.checked_add(call_amount).unwrap();
    let percent = f64::from(amount) / f64::from(pot_with_call);
    let percent = (percent * 100.0).round();
    assert!(percent >= 0.0 && percent <= f64::from(u32::MAX));
    let percent = percent as u32;

    percent
}
