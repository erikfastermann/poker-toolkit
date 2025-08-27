use std::cmp::Ordering;
use std::fmt::Write;
use std::fs::read_to_string;
use std::io::{self, BufWriter};
use std::path::Path;
use std::time::Instant;

use eframe::egui::{CentralPanel, Context, Rect, Style, UiBuilder, Vec2, ViewportBuilder, Visuals};
use eframe::Frame;
use poker_core::cards::Cards;
use poker_core::db::{self, DB};
use poker_core::equity::{Equity, EquityTable};
use poker_core::game::Game;
use poker_core::parser::GGHandHistoryParser;
use poker_core::phh_parser::parse_phhs_str;
use poker_core::range::RangeTable;
use poker_core::result::Result;
use poker_gui::game_view::GameView;
use poker_gui::history_viewer::HistoryView;
use rusqlite::types::Value;
use walkdir::WalkDir;

const INVALID_COMMAND_ERROR: &'static str = "Invalid command. See README for usage.";

fn main() -> Result<()> {
    unsafe { poker_core::init::init() };

    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("enumerate") => enumerate(&args[2..]),
        Some("simulate") => simulate(&args[2..]),
        Some("enumerate-table") => enumerate_table(&args[2..]),
        Some("simulate-table") => simulate_table(&args[2..]),
        Some("parse-gg") => parse_gg(&args[2..]),
        Some("parse-phhs") => parse_phhs(&args[2..]),
        Some("query") => query(&args[2..]),
        Some("gui") => gui(&args[2..]),
        Some("history-gui") => history_gui(&args[2..]),
        _ => Err(INVALID_COMMAND_ERROR.into()),
    }
}

fn enumerate(args: &[String]) -> Result<()> {
    let [community_cards_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let community_cards = Cards::from_str(community_cards_raw)?;
    let ranges = args[1..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Box::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) = Equity::enumerate(community_cards, &ranges) else {
        return Err("enumerate failed: invalid input or expected sample to large".into());
    };
    print_equities(&equities);
    Ok(())
}

fn simulate(args: &[String]) -> Result<()> {
    let [rounds_raw, community_cards_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let rounds: u64 = rounds_raw.parse()?;
    let community_cards = Cards::from_str(community_cards_raw)?;
    let ranges = args[2..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Box::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) = Equity::simulate(community_cards, &ranges, rounds) else {
        return Err("simulate failed: invalid input".into());
    };
    print_equities(&equities);
    Ok(())
}

fn print_equities(equities: &[Equity]) {
    assert!(equities.len() >= 2);
    for (i, equity) in equities.iter().enumerate() {
        println!("player {}: {}", i + 1, equity);
    }
}

fn enumerate_table(args: &[String]) -> Result<()> {
    let [community_cards_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let community_cards = Cards::from_str(community_cards_raw)?;
    let ranges = args[1..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Box::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) = EquityTable::enumerate(community_cards, &ranges) else {
        return Err("enumerate-table failed: invalid input or expected sample to large".into());
    };
    print_equity_tables(&ranges, &equities);
    Ok(())
}

fn simulate_table(args: &[String]) -> Result<()> {
    let [rounds_raw, community_cards_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let rounds: u64 = rounds_raw.parse()?;
    let community_cards = Cards::from_str(community_cards_raw)?;
    let ranges = args[2..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Box::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equity_tables) = EquityTable::simulate(community_cards, &ranges, rounds) else {
        return Err("simulate-table failed: invalid input".into());
    };
    print_equity_tables(&ranges, &equity_tables);
    Ok(())
}

fn print_equity_tables(ranges: &[impl AsRef<RangeTable>], equity_tables: &[EquityTable]) {
    assert!(equity_tables.len() >= 2);
    for (i, equity_table) in equity_tables.iter().enumerate() {
        println!("player {}: {}", i + 1, equity_table.total_equity());

        let range = ranges[i].as_ref();
        let mut hands: Vec<_> = range.into_iter().collect();
        hands.sort_by(|a, b| {
            let a = equity_table.equity(*a);
            let b = equity_table.equity(*b);
            a.equity_percent()
                .partial_cmp(&b.equity_percent())
                .unwrap_or(Ordering::Less)
                .then_with(|| {
                    a.win_percent()
                        .partial_cmp(&b.win_percent())
                        .unwrap_or(Ordering::Less)
                })
                .reverse()
        });

        for hand in hands {
            let no_data = if equity_table.has_data(hand) {
                ""
            } else {
                " (no data)"
            };
            println!("  - {}: {}{}", hand, equity_table.equity(hand), no_data);
        }
        println!()
    }
}

fn parse_gg(args: &[String]) -> Result<()> {
    let [db_path, hand_history_path] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };

    let mut db = DB::open_and_create(&db_path)?;

    let read_time = Instant::now();
    let content = read_to_string(hand_history_path)?;
    eprintln!(
        "--- took {:?} to read the hand history file ---",
        read_time.elapsed(),
    );

    let parse_time = Instant::now();
    let games = GGHandHistoryParser::new(true).parse_str_full(&content);
    eprintln!(
        "--- took {:?} to parse the hand history file ---",
        parse_time.elapsed(),
    );

    let assert_print_errors_time = Instant::now();
    let mut game_data = Vec::new();
    let mut error_count = 0usize;
    for game in &games {
        match game {
            Ok(game) => {
                game.internal_asserts_full(); // TODO
                game_data.push(game.to_game_data());
            }
            Err(err) => {
                eprintln!("{err}\n");
                error_count += 1;
            }
        }
    }
    eprintln!(
        "--- took {:?} to assert the internal game states and print errors ---",
        assert_print_errors_time.elapsed(),
    );
    eprintln!("--- found {} hands ---", game_data.len());

    let write_json_time = Instant::now();
    serde_json::to_writer_pretty(BufWriter::new(io::stdout().lock()), &game_data)?;
    println!();
    eprintln!(
        "--- took {:?} to write the json output ---",
        write_json_time.elapsed(),
    );

    let write_db_time = Instant::now();
    let new_hands_count = db.add_games(games.iter().filter_map(|game| game.as_ref().ok()))?;
    eprintln!(
        "--- took {:?} to write {new_hands_count} new hand(s) to the database ---",
        write_db_time.elapsed(),
    );

    if error_count > 0 {
        let message =
            format!("parse-gg: {error_count} error(s) occurred while parsing the hand history");
        Err(message.into())
    } else {
        Ok(())
    }
}

fn parse_phhs(args: &[String]) -> Result<()> {
    let [db_path, hand_history_path] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };

    let start_time = Instant::now();

    let mut db = DB::open_and_create(&db_path)?;

    let hand_history_path = Path::new(hand_history_path);

    let (new_hands_count, error_count) = if hand_history_path.is_dir() {
        let mut new_hands_count = 0u64;
        let mut error_count = 0u64;

        let entries = WalkDir::new(&hand_history_path)
            .into_iter()
            .filter_map(std::result::Result::ok);

        for entry in entries {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "phhs") {
                let (current_new_hands, current_errors) = parse_phhs_file(path, &mut db);

                new_hands_count = new_hands_count.saturating_add(current_new_hands);
                error_count = error_count.saturating_add(current_errors);
            }
        }

        (new_hands_count, error_count)
    } else {
        parse_phhs_file(hand_history_path, &mut db)
    };

    eprintln!(
        "--- took {:?} to parse and write {new_hands_count} new hand(s) to the database ---",
        start_time.elapsed(),
    );

    if error_count > 0 {
        let message =
            format!("parse-phhs: {error_count} error(s) occurred while parsing the hand history");
        Err(message.into())
    } else {
        Ok(())
    }
}

fn parse_phhs_file(path: &Path, db: &mut DB) -> (u64, u64) {
    let content = match read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("error reading the phhs file from path {path:?}: {err}");
            return (0, 1);
        }
    };

    let entries = match parse_phhs_str(&content, true) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("parsing of phhs file from path {path:?} failed: {err}");
            return (0, 1);
        }
    };

    let mut error_count = 0u64;
    let mut games = Vec::new();

    for entry in entries {
        match entry {
            Ok(game) => {
                game.internal_asserts_full(); // TODO
                games.push(game);
            }
            Err(err) => {
                eprintln!("error parsing phh entry from path {path:?}:\n{err}\n");
                error_count += 1;
            }
        }
    }

    match db.add_games(games.iter()) {
        Ok(new_hands_count) => (new_hands_count, error_count),
        Err(err) => {
            eprintln!("error writing phh hands from path {path:?} to the db: {err}");
            (0, 1)
        }
    }
}

fn query(args: &[String]) -> Result<()> {
    let [db_path, query] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let db = DB::open(db_path)?;

    let mut formatted = String::new();
    db.query_for_each(query, (), |row| {
        formatted.truncate(0);

        for (index, value) in row.iter().enumerate() {
            match value {
                Value::Null => write!(&mut formatted, "Null")?,
                Value::Integer(n) => write!(&mut formatted, "{n}")?,
                Value::Real(n) => write!(&mut formatted, "{n}")?,
                Value::Text(s) => write!(&mut formatted, "{s:?}")?,
                Value::Blob(b) => write!(&mut formatted, "{b:?}")?,
            }

            if index != row.len() - 1 {
                write!(&mut formatted, ", ")?;
            }
        }

        println!("{formatted}");
        Ok(true)
    })?;
    Ok(())
}

fn history_gui(args: &[String]) -> Result<()> {
    const DEFAULT_QUERY: &str =
        "SELECT * FROM hands LEFT JOIN hands_players ON id = hand_id AND hero_index = player";

    let (db_path, query) = match args {
        [db_path] => (db_path.as_str(), DEFAULT_QUERY),
        [db_path, query] => (db_path.as_str(), query.as_str()),
        _ => return Err(INVALID_COMMAND_ERROR.into()),
    };

    let db = DB::open(db_path)?;
    let hands = db.load_hands_from_query(query, ())?;

    let game_getter = move |hand_id| {
        db.get_game_data(hand_id)
            .and_then(|data| Game::from_game_data(&data))
    };

    // TODO: Deduplicate.
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "Poker Toolkit",
        options,
        Box::new(|cc| {
            let style = Style {
                visuals: Visuals::dark(),
                ..Style::default()
            };
            cc.egui_ctx.set_style(style);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(HandHistory::new(hands, game_getter)?))
        }),
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn gui(args: &[String]) -> Result<()> {
    let pre_flop_ranges_config_path = match args {
        [] => None,
        [pre_flop_ranges_config_path] => Some(pre_flop_ranges_config_path.as_str()),
        _ => return Err(INVALID_COMMAND_ERROR.into()),
    };

    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "Poker Toolkit",
        options,
        Box::new(|cc| {
            let style = Style {
                visuals: Visuals::dark(),
                ..Style::default()
            };
            cc.egui_ctx.set_style(style);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(App::new(pre_flop_ranges_config_path)?))
        }),
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

struct App {
    game: GameView,
}

impl App {
    fn new(pre_flop_ranges_config_path: Option<&str>) -> Result<Self> {
        let game = if let Some(path) = pre_flop_ranges_config_path {
            GameView::new_with_simple_strategy(path)?
        } else {
            GameView::new()
        };
        Ok(Self { game })
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        CentralPanel::default().show(ctx, |ui| {
            let table_height = ui.clip_rect().height() * 0.9;
            let bounding_rect = Rect::from_center_size(
                ui.clip_rect().center(),
                Vec2 {
                    x: table_height * 4.0 / 3.0,
                    y: table_height,
                },
            );
            ui.allocate_new_ui(UiBuilder::new().max_rect(bounding_rect), |ui| {
                self.game.view(ui).unwrap()
            });
        });
    }
}

struct HandHistory {
    history: HistoryView,
}

impl HandHistory {
    fn new(
        entries: Vec<(db::Hand, Option<db::HandPlayer>)>,
        game_getter: impl FnMut(u64) -> Result<Game> + 'static,
    ) -> Result<Self> {
        Ok(Self {
            history: HistoryView::new(entries, Box::new(game_getter)),
        })
    }
}

impl eframe::App for HandHistory {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.history.view(ctx);
    }
}
