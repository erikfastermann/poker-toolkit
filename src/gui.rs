use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Ui};

use crate::{card::Card, game::Game, rank::Rank, result::Result, suite::Suite};

pub fn gui() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_maximized(true),
        ..Default::default()
    };
    eframe::run_native(
        "Poker Toolkit",
        options,
        Box::new(|cc| {
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
            game: GameView::new(Game::new([100, 100, 100].into_iter(), 2, 5, 10)?),
        })
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| self.game.view(ui));
    }
}

struct GameView {
    game: Game,
}

impl GameView {
    fn new(game: Game) -> Self {
        Self { game }
    }

    fn view(&mut self, ui: &mut Ui) -> Result<()> {
        let card = Card::of(Rank::Ace, Suite::Diamonds);
        draw_card(ui.painter(), card, Pos2 { x: 200.0, y: 300.0 }, 200.0);
        Ok(())
    }
}

fn draw_card(painter: &Painter, card: Card, center: Pos2, width: f32) {
    let bounding_rect = Rect::from_two_pos(
        Pos2 {
            x: center.x - width / 2.0,
            y: center.y - width / 1.5,
        },
        Pos2 {
            x: center.x + width / 2.0,
            y: center.y + width / 1.5,
        },
    );
    let card_shape = Shape::rect_filled(bounding_rect, 10.0, suite_color(card.suite()));
    painter.add(card_shape);
    let rank_pos = Pos2 {
        x: bounding_rect.center_bottom().x,
        y: bounding_rect.center_bottom().y + width / 5.0,
    };
    painter.text(
        rank_pos,
        Align2::CENTER_BOTTOM,
        format!("{}", card.rank()),
        FontId::new(width * 1.3, egui::FontFamily::Proportional),
        Color32::WHITE,
    );
    let suite_pos = Pos2 {
        x: bounding_rect.right() - width / 6.0,
        y: bounding_rect.top() + width / 6.0,
    };
    painter.text(
        suite_pos,
        Align2::CENTER_CENTER,
        card.suite().to_symbol_str(),
        FontId::new(width / 3.0, egui::FontFamily::Proportional),
        Color32::WHITE,
    );
}

fn suite_color(suite: Suite) -> Color32 {
    match suite {
        Suite::Diamonds => Color32::BLUE,
        Suite::Spades => Color32::BLACK,
        Suite::Hearts => Color32::RED,
        Suite::Clubs => Color32::GREEN,
    }
}
