use std::cmp::Ordering;

use eframe::egui::{CentralPanel, Context, Rect, Style, UiBuilder, Vec2, ViewportBuilder, Visuals};
use eframe::Frame;
use poker_core::cards::Cards;
use poker_core::equity::{Equity, EquityTable};
use poker_core::range::RangeTable;
use poker_core::result::Result;
use poker_gui::game_view::GameView;

const INVALID_COMMAND_ERROR: &'static str = "Invalid command. See README for usage.";

fn main() -> Result<()> {
    unsafe { poker_core::init::init() };

    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("enumerate") => enumerate(&args[2..]),
        Some("simulate") => simulate(&args[2..]),
        Some("enumerate-table") => enumerate_table(&args[2..]),
        Some("simulate-table") => simulate_table(&args[2..]),
        Some("gui") => {
            if args.len() != 2 {
                return Err(INVALID_COMMAND_ERROR.into());
            }
            gui().map_err(|err| format!("{err}"))?;
            Ok(())
        }
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

fn gui() -> eframe::Result {
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
            Ok(Box::new(App::new()?))
        }),
    )
}

struct App {
    game: GameView,
}

impl App {
    fn new() -> Result<Self> {
        Ok(Self {
            game: GameView::new(),
        })
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
