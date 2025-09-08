use eframe::{
    egui::{CentralPanel, Context, Rect, Style, UiBuilder, Vec2, ViewportBuilder, Visuals},
    Frame,
};
use poker_core::{
    ai::{AiAction, PlayerActionGenerator},
    game::Game,
    hand::Hand,
    init::init,
    range::{RangeConfigEntry, RangeTableWith},
    result::Result,
};
use poker_gui::game_view::{GameView, PlayerActionGeneratorEntry};
use poker_human::{ActionHead, ActionProbabilities, ShowdownHead, ShowdownProbabilities};
use pyo3::prelude::*;
use rand::thread_rng;
use std::fmt::Write;

const ACTION_MODEL_PATH: &str = "action.pt";

const SHOWDOWN_MODEL_PATH: &str = "showdown.pt";

fn main() -> Result<()> {
    unsafe { init() };

    gui()
}

// TODO: Could unify the gui code more with poker-app.

fn gui() -> Result<()> {
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
            Ok(Box::new(App::new()?))
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

impl App {
    fn new() -> Result<Self> {
        let action_head = ActionHead::new(ACTION_MODEL_PATH)?;
        let showdown_head = ShowdownHead::new(SHOWDOWN_MODEL_PATH)?;

        let action_generator = PlayerActionGeneratorEntry::new(
            "Human",
            Box::new(move || {
                Python::with_gil(|py| {
                    Box::new(HumanActionGenerator::new(
                        action_head.clone_ref(py),
                        showdown_head.clone_ref(py),
                    ))
                })
            }),
        );

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
