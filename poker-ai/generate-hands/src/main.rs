use std::{
    collections::BTreeMap,
    iter,
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread,
    time::Instant,
};

use poker_core::{
    ai::{EquityStrategy, PlayerActionGenerator},
    db::DB,
    game::{Game, Player, State},
    range::RangeInfo,
    result::Result,
};

const DB_PATH: &str = "equity.db";
const WORKER_THREADS: usize = 10;
const TOTAL_HANDS: usize = 100;
const REPORT_INTERVAL: usize = 10;
const WRITE_TO_DB_INTERVAL: usize = 100;

const PLAYER_COUNT: usize = 6;
const STARTING_STACK: u32 = 1000;
const SMALL_BLIND: u32 = 5;
const BIG_BLIND: u32 = 10;

fn main() -> Result<()> {
    spawn_workers()
}

fn spawn_workers() -> Result<()> {
    let start = Instant::now();

    let mut db = DB::open_and_create(DB_PATH)?;

    let (tx, rx) = mpsc::channel::<Game>();
    let counter = Arc::new(Mutex::new(0));

    for _ in 0..WORKER_THREADS {
        let tx = tx.clone();
        let counter = Arc::clone(&counter);

        thread::spawn(move || worker_loop(tx, counter, start));
    }

    let mut games = Vec::new();

    for game in rx.into_iter().take(TOTAL_HANDS) {
        games.push(game);

        if games.len() >= WRITE_TO_DB_INTERVAL {
            db.add_games(games.iter())?;
            games.clear();
        }
    }

    db.add_games(games.iter())?;

    Ok(())
}

fn worker_loop(tx: Sender<Game>, counter: Arc<Mutex<usize>>, start: Instant) {
    loop {
        let mut count = counter.lock().unwrap();

        if *count >= TOTAL_HANDS {
            break;
        }

        if *count % REPORT_INTERVAL == 0 {
            eprintln!("{count}/{TOTAL_HANDS} (elapsed: {:?})", start.elapsed());
        }

        *count += 1;
        drop(count);

        let game = produce_hand().unwrap();

        if tx.send(game).is_err() {
            break;
        }
    }
}

fn produce_hand() -> Result<Game> {
    let mut rng = rand::thread_rng();

    let players: Vec<_> = iter::repeat(Player::with_starting_stack(STARTING_STACK))
        .take(PLAYER_COUNT)
        .collect();

    let mut game = Game::new(&players, 0, SMALL_BLIND, BIG_BLIND)?;
    game.draw_unset_hands(&mut rng);
    game.post_small_and_big_blind()?;

    let mut player_strategies: Vec<_> = iter::repeat(EquityStrategy::new())
        .take(PLAYER_COUNT)
        .collect();

    assert!(player_strategies.iter().all(|s| !s.custom_show_or_muck()));

    let mut range_info = BTreeMap::new();

    for _ in 0..100_000 {
        match game.state() {
            State::Post => unreachable!(),
            State::Player(player) => {
                let mut log = String::new();
                let (action, range, _) = player_strategies[player].update_hero(&game, &mut log)?;

                if log.len() != 0 {
                    // TODO: Support log.
                    return Err(
                        format!("game log is not empty: {:?}: {}", game.actions(), log).into(),
                    );
                }

                action.apply_to_game(&mut game)?;

                if let Some(range) = range {
                    let actions: Vec<_> = range
                        .action_kinds()
                        .map(|action| (action, range.frequency(action)))
                        .collect();

                    range_info.insert(game.actions().len(), RangeInfo::Frequencies(actions));
                }

                for (villain, strat) in player_strategies.iter_mut().enumerate() {
                    if villain == player {
                        continue;
                    }

                    strat.update_villain(&game, &mut log)?;

                    if log.len() != 0 {
                        // TODO: Support log.
                        return Err(format!(
                            "game log is not empty for villain {}: {:?}: {}",
                            villain,
                            game.actions(),
                            log
                        )
                        .into());
                    }
                }
            }
            State::Street(_) => game.draw_next_street(&mut rng)?,
            State::UncalledBet { .. } => game.uncalled_bet()?,
            State::ShowOrMuck(player) => {
                if game.should_show()? {
                    game.show_hand()?
                } else {
                    game.muck_hand()?
                }

                let range = player_strategies[player].current_range();
                range_info.insert(game.actions().len(), RangeInfo::Range(range.clone()));
            }
            State::ShowdownOrNextRunout => game.showdown_simple()?,
            State::End => {
                game.set_additional_metadata(Arc::from("range_info"), &range_info)?;
                return Ok(game);
            }
        }
    }

    panic!("maximum number of actions in hand reached");
}
