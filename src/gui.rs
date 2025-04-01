use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Ui, Vec2};

use crate::{card::Card, game::Game, rank::Rank, result::Result, suite::Suite};

// TODO:
// - Portable text drawing fonts

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
            game: GameView::new(Game::new(
                vec![100, 100, 100, 100, 100, 100, 100, 100, 100],
                None,
                1,
                5,
                10,
            )?),
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
        self.draw_table(ui.painter());
        Ok(())
    }

    fn draw_table(&self, painter: &Painter) {
        let center = Pos2 { x: 500.0, y: 500.0 };
        let radius = Vec2 { x: 350.0, y: 200.0 };
        let table_fill = Shape::ellipse_filled(center, radius, Color32::from_rgb(0, 35, 0));
        painter.add(table_fill);
        let table_stroke = Shape::ellipse_stroke(
            center,
            radius,
            Stroke::new(radius.x / 50.0, Color32::DARK_GRAY),
        );
        painter.add(table_stroke);
        let player_size = Vec2 {
            x: radius.x / 2.5,
            y: radius.x / 3.0,
        };
        for (player, center) in self.table_player_points(center, radius).enumerate() {
            self.draw_player(painter, player, center, player_size);
        }
    }

    fn draw_player(&self, painter: &Painter, player: usize, center: Pos2, size: Vec2) {
        let bounding_rect = Rect::from_center_size(center, size);
        let name_stack_rect = bounding_rect.with_min_y(bounding_rect.bottom() - size.y / 3.0);
        let name_stack_shape = Shape::rect_filled(name_stack_rect, 5.0, Color32::BLACK);
        painter.add(name_stack_shape);
        let name_rect = name_stack_rect.with_max_y(name_stack_rect.center().y);
        painter.text(
            name_rect.center(),
            Align2::CENTER_CENTER,
            self.game.player_name(player),
            FontId::new(name_rect.height() * 0.9, egui::FontFamily::Proportional),
            Color32::WHITE,
        );
        let stack_rect = name_stack_rect.with_min_y(name_stack_rect.center().y);
        painter.text(
            stack_rect.center(),
            Align2::CENTER_CENTER,
            self.game.current_stacks()[player],
            FontId::new(stack_rect.height() * 0.9, egui::FontFamily::Proportional),
            Color32::WHITE,
        );

        let hand_bounding_rect = bounding_rect.with_max_y(name_stack_rect.top());
        let card_width = hand_bounding_rect.height() / 1.3;
        let card_a = Card::of(Rank::Ace, Suite::Diamonds);
        let card_b = Card::of(Rank::Ace, Suite::Spades);
        let card_a_rect = Rect::from_center_size(
            Pos2 {
                x: hand_bounding_rect.center().x - card_width / 2.0,
                y: hand_bounding_rect.center().y,
            },
            Vec2 {
                x: card_width,
                y: hand_bounding_rect.height(),
            },
        );
        let card_b_rect = card_a_rect.translate(Vec2 {
            x: card_width,
            y: 0.0,
        });
        draw_card(painter, card_a_rect, card_a);
        draw_card(painter, card_b_rect, card_b);
    }

    fn table_player_points(&self, center: Pos2, radius: Vec2) -> impl Iterator<Item = Pos2> {
        let n = self.game.player_count();
        let offset = if !self.game.is_heads_up_table() {
            4.5 * std::f32::consts::PI
        } else {
            0.0
        };
        (0..n).map(move |i| {
            let theta = offset + 2.0 * std::f32::consts::PI * i as f32 / n as f32;
            let x = center.x + radius.x * theta.cos();
            let y = center.y + radius.y * theta.sin();
            Pos2 { x, y }
        })
    }
}

fn draw_card(painter: &Painter, bounding_rect: Rect, card: Card) {
    let width = bounding_rect.width();
    let height = bounding_rect.height();
    assert!((width * 1.3 - height).abs() <= 0.1);
    let card_shape = Shape::rect_filled(bounding_rect, 10.0, suite_color(card.suite()));
    painter.add(card_shape);
    let rank_pos = Pos2 {
        x: bounding_rect.center_bottom().x,
        y: bounding_rect.center_bottom().y + width / 5.0,
    };
    painter.text(
        rank_pos,
        Align2::CENTER_BOTTOM,
        card.rank().to_string(),
        FontId::new(width * 1.3, egui::FontFamily::Proportional),
        Color32::WHITE,
    );
    let suite_pos = Pos2 {
        x: bounding_rect.right() - width / 8.0,
        y: bounding_rect.top() + width / 8.0,
    };
    painter.text(
        suite_pos,
        Align2::CENTER_CENTER,
        card.suite().to_symbol_str(),
        FontId::new(width / 4.0, egui::FontFamily::Proportional),
        Color32::WHITE,
    );
}

fn suite_color(suite: Suite) -> Color32 {
    match suite {
        Suite::Diamonds => Color32::BLUE,
        Suite::Spades => Color32::from_rgb(35, 35, 35),
        Suite::Hearts => Color32::RED,
        Suite::Clubs => Color32::DARK_GREEN,
    }
}
