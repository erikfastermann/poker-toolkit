use eframe::{
    egui::{
        CentralPanel, Context, Id, Key, Rect, Style, Ui, UiBuilder, Vec2, ViewportBuilder, Visuals,
        Window,
    },
    Frame,
};
use poker_core::{
    ai::{AiAction, EquityStrategy, PlayerActionGenerator},
    card::Card,
    cards::Cards,
    game::Game,
    hand::Hand,
    range::{RangeConfigEntry, RangeInfo, RangeTable, RangeTableWith, MAX_FREQUENCY},
    result::Result,
};
use poker_gui::{
    game_view::{GameView, PlayerActionGeneratorEntry},
    range_viewer::{RangeValue, RangeViewer},
};
use poker_human::{
    equities_from_range, ActionHead, ActionProbabilities, ShowdownHead, ShowdownProbabilities,
};
use pyo3::{prelude::*, types::PyList};
use rand::thread_rng;
use serde::Deserialize;
use std::{
    collections::HashMap,
    env,
    fmt::Write,
    fs::File,
    io::{BufReader, BufWriter},
    str::FromStr,
    sync::{Arc, Mutex},
    thread,
};

fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();

    if args.len() < 2 {
        return Err("invalid command".into());
    }

    match args[1].as_str() {
        "model" => game_gui(false, &args[2..]),
        "baseline" => game_gui(true, &args[2..]),
        "ranges" => range_gui(&args[2..]),
        "showdown-equities" => showdown_equities(&args[2..]),
        _ => return Err("invalid command".into()),
    }
}

// TODO: Could unify the gui code more with poker-app.

fn game_gui(with_baseline: bool, args: &[String]) -> Result<()> {
    if args.len() != 2 {
        return Err("invalid command".into());
    }

    let action_model_path = args[0].as_str();
    let showdown_model_path = args[1].as_str();

    // TODO: Workaround
    Python::with_gil(|py| {
        let sys = py.import("sys")?;
        let path = sys.getattr("path")?;
        let path: &Bound<'_, PyList> = path.downcast().unwrap();
        path.insert(0, ".")
    })?;

    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "Poker Toolkit - Human",
        options,
        Box::new(|cc| {
            let style = Style {
                visuals: Visuals::dark(),
                ..Style::default()
            };
            cc.egui_ctx.set_style(style);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(GameApp::new(
                with_baseline,
                action_model_path,
                showdown_model_path,
            )?))
        }),
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

fn range_gui(args: &[String]) -> Result<()> {
    if args.len() != 2 {
        return Err("invalid command".into());
    }

    let range_data_path = args[0].as_str();
    let equity_data_path = args[1].as_str();

    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default().with_maximized(true),
        ..Default::default()
    };

    eframe::run_native(
        "Poker Toolkit - Ranges",
        options,
        Box::new(|cc| {
            let style = Style {
                visuals: Visuals::dark(),
                ..Style::default()
            };
            cc.egui_ctx.set_style(style);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(RangeApp::new(range_data_path, equity_data_path)?))
        }),
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

struct GameApp {
    game: GameView,
}

struct HumanActionGenerator {
    action_head: ActionHead,
    showdown_head: ShowdownHead,
}

impl HumanActionGenerator {
    fn new(action_head: ActionHead, showdown_head: ShowdownHead) -> Self {
        Self {
            action_head,
            showdown_head,
        }
    }
}

impl PlayerActionGenerator for HumanActionGenerator {
    fn update_villain(&mut self, _game: &Game, _log: &mut String) -> Result<()> {
        Ok(())
    }

    fn update_hero(
        &mut self,
        game: &Game,
        log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        let probs = ActionProbabilities::predict(&self.action_head, game)?;
        writeln!(log, "{probs}")?;

        let mut rng = &mut thread_rng(); // TODO: Could use deterministic rng.
        let (action, extra_info) = probs.choose(&mut rng);

        if !extra_info.is_empty() {
            writeln!(log, "Bet/raise size: {extra_info}")?;
        }

        Ok((action, None, None))
    }

    fn custom_show_or_muck(&self) -> bool {
        true
    }

    fn show_or_muck(&self, game: &Game, log: &mut String) -> Result<Option<Hand>> {
        let probs = ShowdownProbabilities::predict(&self.showdown_head, game)?;

        writeln!(log, "{probs:#?}")?;

        Ok(probs.choose(&mut thread_rng(), game.known_cards()))
    }
}

struct BaselineActionGenerator {
    baseline: EquityStrategy,
    action_head: ActionHead,
    showdown_head: ShowdownHead, // TODO
}

impl BaselineActionGenerator {
    fn new(action_head: ActionHead, showdown_head: ShowdownHead) -> Self {
        Self {
            action_head,
            showdown_head,
            baseline: EquityStrategy::new(),
        }
    }
}

impl PlayerActionGenerator for BaselineActionGenerator {
    fn update_villain(&mut self, game: &Game, log: &mut String) -> Result<()> {
        self.baseline.update_villain(game, log)
    }

    fn update_hero(
        &mut self,
        game: &Game,
        log: &mut String,
    ) -> Result<(
        AiAction,
        Option<RangeConfigEntry>,
        Option<&[RangeTableWith<u16>]>,
    )> {
        let probs = ActionProbabilities::predict(&self.action_head, game)?;
        let (action, range, villains) = self.baseline.update_hero(game, log)?;

        if let Some(range) = range.as_ref() {
            let baseline_probs = ActionProbabilities::from_range(game, range)?;

            writeln!(
                log,
                "Probabilities (Actual | Predicted):\n{}",
                baseline_probs.comparison_string(&probs),
            )?;
        } else {
            writeln!(log, "Predicted probabilities:\n{probs}")?;
        }

        Ok((action, range, villains))
    }

    fn showdown_info(&self, game: &Game) -> Result<Option<RangeInfo>> {
        let baseline_range = self.baseline.current_range().clone();
        let baseline_equities = equities_from_range(game.board().cards_set(), &baseline_range);

        let probs = ShowdownProbabilities::predict(&self.showdown_head, game)?;
        let model_range = probs.range();
        let model_equities = equities_from_range(game.board().cards_set(), &model_range);

        let ranges = vec![
            ("Actual".to_owned(), baseline_range),
            ("Predicted".to_owned(), model_range),
            ("Actual equities".to_owned(), baseline_equities),
            ("Predicted equities".to_owned(), model_equities),
        ];

        Ok(Some(RangeInfo::Ranges(ranges)))
    }
}

impl GameApp {
    fn new(
        with_baseline: bool,
        action_model_path: &str,
        showdown_model_path: &str,
    ) -> Result<Self> {
        let action_head = ActionHead::new(action_model_path)?;
        let showdown_head = ShowdownHead::new(showdown_model_path)?;

        let action_generator = if with_baseline {
            PlayerActionGeneratorEntry::new(
                "Baseline",
                Box::new(move || {
                    Python::with_gil(|py| {
                        Box::new(BaselineActionGenerator::new(
                            action_head.clone_ref(py),
                            showdown_head.clone_ref(py),
                        ))
                    })
                }),
            )
        } else {
            PlayerActionGeneratorEntry::new(
                "Human",
                Box::new(move || {
                    Python::with_gil(|py| {
                        Box::new(HumanActionGenerator::new(
                            action_head.clone_ref(py),
                            showdown_head.clone_ref(py),
                        ))
                    })
                }),
            )
        };

        let game = GameView::new_with_action_generators([action_generator], Some(0))?;

        Ok(Self { game })
    }
}

impl eframe::App for GameApp {
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

struct RangeApp {
    expected: RangeViewer,
    got: RangeViewer,
    uniform: RangeViewer,

    range_data: Vec<RangeEntry>,
    equity_data: HashMap<u64, [RangeTableWith<u16>; 3]>,
    current_entry: usize,
}

impl RangeApp {
    fn new(range_data_path: &str, equity_data_path: &str) -> Result<Self> {
        let range_data: Vec<RangeEntry> =
            serde_json::from_reader(BufReader::new(File::open(range_data_path)?))?;

        if range_data.is_empty() {
            return Err("empty dataset".into());
        }

        let equity_data: HashMap<u64, [RangeTableWith<u16>; 3]> =
            serde_json::from_reader(BufReader::new(File::open(equity_data_path)?))?;

        if range_data.len() != equity_data.len() {
            return Err("range and equity data lengths don't match".into());
        }

        let mut app = Self {
            expected: RangeViewer::new(),
            got: RangeViewer::new(),
            uniform: RangeViewer::new(),
            range_data,
            equity_data,
            current_entry: 0,
        };

        app.update_ranges()?;
        Ok(app)
    }

    fn view(&mut self, ctx: &Context) {
        // Quick and dirty ui.

        // TODO: Handle errors properly.

        let old_entry = self.current_entry;

        self.expected
            .window(ctx, Id::new("expected"), "Expected".to_owned());

        self.got.window(ctx, Id::new("got"), "Got".to_owned());

        self.uniform
            .window(ctx, Id::new("uniform"), "Uniform".to_owned());

        Window::new("Navigate").show(ctx, |ui| self.navigation(ui));

        if self.current_entry != old_entry {
            self.update_ranges().unwrap()
        }
    }

    fn navigation(&mut self, ui: &mut Ui) {
        let previous_button = ui
            .add_enabled_ui(self.current_entry != 0, |ui| ui.button("<"))
            .inner;

        if previous_button.clicked() || ui.ctx().input(|input| input.key_pressed(Key::ArrowLeft)) {
            self.current_entry -= 1;
        }

        let next_button = ui
            .add_enabled_ui(self.current_entry != self.range_data.len() - 1, |ui| {
                ui.button(">")
            })
            .inner;

        if next_button.clicked() || ui.ctx().input(|input| input.key_pressed(Key::ArrowRight)) {
            self.current_entry += 1;
        }
    }

    fn update_ranges(&mut self) -> Result<()> {
        let entry = &self.range_data[self.current_entry];

        let Some([expected, got, uniform]) = self.equity_data.get(&entry.idx).cloned() else {
            return Err("missing index in range data".into());
        };

        let mae = mean_absolute_error(&expected, &got);
        let mae_uniform = mean_absolute_error(&expected, &uniform);

        self.expected
            .replace_ranges(vec![RangeValue::Simple(expected)]);

        self.got.replace_ranges(vec![RangeValue::Simple(got)]);

        self.uniform
            .replace_ranges(vec![RangeValue::Simple(uniform)]);

        let details = format!(
            "{}\n{}\n{:?}\nMAE: {}\nMAE (uniform): {}",
            entry.idx, entry.prob, entry.board, mae, mae_uniform
        );
        self.expected.set_details(details);

        Ok(())
    }
}

impl eframe::App for RangeApp {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        self.view(ctx)
    }
}

fn mean_absolute_error(expected: &RangeTableWith<u16>, got: &RangeTableWith<u16>) -> f64 {
    let absolute_error: u32 = expected
        .iter()
        .map(|(hand, equity)| u32::from(equity.abs_diff(got[hand])))
        .sum();

    (f64::from(absolute_error) / f64::from(MAX_FREQUENCY)) / Hand::COUNT as f64
}

#[derive(Deserialize)]
struct RangeEntry {
    idx: u64,
    expected: Vec<f32>,
    got: Vec<f32>,
    prob: f64,
    board: Vec<String>,
}

impl RangeEntry {
    fn board(&self) -> Result<Cards> {
        let board = self
            .board
            .iter()
            .map(|card| Card::from_str(card))
            .collect::<Result<Vec<Card>>>()?;

        let Some(board) = Cards::from_slice(&board) else {
            return Err("duplicate card on board".into());
        };

        Ok(board)
    }
}

const WORKER_THREADS: usize = 140;
const REPORT_INTERVAL: usize = 500;

fn showdown_equities(args: &[String]) -> Result<()> {
    if args.len() != 2 {
        return Err("invalid command".into());
    }

    let data_path = args[0].as_str();
    let out_path = args[1].as_str();

    let data: Vec<RangeEntry> = serde_json::from_reader(BufReader::new(File::open(data_path)?))?;
    let out = spawn_workers(data)?;
    serde_json::to_writer(BufWriter::new(File::create(out_path)?), &out)?;

    Ok(())
}

fn spawn_workers(data: Vec<RangeEntry>) -> Result<HashMap<u64, [RangeTableWith<u16>; 3]>> {
    println!("Processing {} entries...", data.len());

    let queue = Arc::new(Mutex::new(data));
    let out = Arc::new(Mutex::new(HashMap::new()));

    let mut handles = Vec::new();

    for _ in 0..WORKER_THREADS {
        let queue = Arc::clone(&queue);
        let out = Arc::clone(&out);

        handles.push(thread::spawn(move || worker_loop(queue, out)));
    }

    for handle in handles {
        handle.join().unwrap()?;
    }

    Ok(Arc::try_unwrap(out).unwrap().into_inner().unwrap())
}

fn worker_loop(
    queue: Arc<Mutex<Vec<RangeEntry>>>,
    out: Arc<Mutex<HashMap<u64, [RangeTableWith<u16>; 3]>>>,
) -> Result<()> {
    loop {
        let entry = {
            let mut queue = queue.lock().unwrap();

            if !queue.is_empty() && queue.len() % REPORT_INTERVAL == 0 {
                println!("{} remaining", queue.len());
            }

            queue.pop()
        };

        let Some(entry) = entry else {
            return Ok(());
        };

        let board = entry.board()?;

        let expected = ShowdownProbabilities::from_probabilities(&entry.expected, None)?;
        let got = ShowdownProbabilities::from_probabilities(&entry.got, None)?;

        let expected_equities = equities_from_range(board, &expected.range());
        let got_equities = equities_from_range(board, &got.range());
        let uniform_equities =
            equities_from_range(board, &RangeTable::FULL.to_frequencies(MAX_FREQUENCY));

        {
            let mut out = out.lock().unwrap();

            let old_entry = out.insert(
                entry.idx,
                [expected_equities, got_equities, uniform_equities],
            );

            if old_entry.is_some() {
                return Err("duplicate index".into());
            }
        }
    }
}
