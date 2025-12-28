use eframe::{
    egui::{CentralPanel, Context, Rect, Style, UiBuilder, Vec2, ViewportBuilder, Visuals},
    Frame,
};
use poker_core::{
    ai::{AiAction, EquityStrategy, PlayerActionGenerator},
    game::Game,
    hand::Hand,
    init::init,
    range::{RangeConfigEntry, RangeInfo, RangeTableWith},
    result::Result,
};
use poker_gui::game_view::{GameView, PlayerActionGeneratorEntry};
use poker_human::{
    equities_from_range, ActionHead, ActionProbabilities, ShowdownHead, ShowdownProbabilities,
};
use pyo3::{prelude::*, types::PyList};
use rand::thread_rng;
use std::{env, fmt::Write};

fn main() -> Result<()> {
    unsafe { init() };

    let args: Vec<_> = env::args().collect();

    if args.len() != 4 {
        return Err("invalid command".into());
    }

    let with_baseline = match args[1].as_str() {
        "model" => false,
        "baseline" => true,
        _ => return Err("invalid command".into()),
    };

    let action_model_path = args[2].as_str();
    let showdown_model_path = args[3].as_str();
    gui(with_baseline, action_model_path, showdown_model_path)
}

// TODO: Could unify the gui code more with poker-app.

fn gui(with_baseline: bool, action_model_path: &str, showdown_model_path: &str) -> Result<()> {
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
            Ok(Box::new(App::new(
                with_baseline,
                action_model_path,
                showdown_model_path,
            )?))
        }),
    )
    .map_err(|err| err.to_string())?;

    Ok(())
}

struct App {
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

impl App {
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
