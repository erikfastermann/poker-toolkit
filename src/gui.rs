use eframe::egui::{
    self, Align, Align2, Button, Color32, FontFamily, FontId, Layout, Painter, Pos2, Rect, Rgba,
    Shape, Stroke, StrokeKind, TextStyle, Ui, UiBuilder, Vec2,
};

use crate::{card::Card, game::Game, hand::Hand, rank::Rank, result::Result, suite::Suite};

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
        let mut game = Game::new(
            vec![100, 100, 100, 100, 100, 100, 100, 100, 100],
            None,
            1,
            5,
            10,
        )?;
        game.set_hand(
            1,
            Hand::of_two_cards(
                Card::of(Rank::Ace, Suite::Diamonds),
                Card::of(Rank::King, Suite::Spades),
            )
            .unwrap(),
        )?;
        game.post_small_and_big_blind()?;
        Ok(Self {
            game: GameView::new(game),
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
        let table_height = ui.clip_rect().height() / 2.0;
        let bounding_rect = Rect::from_center_size(
            ui.clip_rect().center(),
            Vec2 {
                x: table_height * 4.0 / 3.0,
                y: table_height,
            },
        );
        assert!(((bounding_rect.width() / 4.0 * 3.0) - bounding_rect.height()).abs() <= 0.1);

        let table_bounding_rect =
            bounding_rect.with_max_y(bounding_rect.bottom() - bounding_rect.height() / 6.0);
        self.draw_table(ui.painter(), table_bounding_rect);

        let action_bar_bounding_rect =
            bounding_rect.with_min_y(table_bounding_rect.bottom() + bounding_rect.height() / 100.0);
        ui.allocate_new_ui(
            UiBuilder::new()
                .max_rect(action_bar_bounding_rect)
                .layout(Layout::right_to_left(Align::Center)),
            |ui| {
                ui.set_clip_rect(action_bar_bounding_rect);
                self.action_bar(ui)
            },
        )
        .inner
    }

    fn draw_table(&self, painter: &Painter, bounding_rect: Rect) {
        let player_size = Vec2 {
            x: bounding_rect.width() / 5.0,
            y: bounding_rect.height() / 4.5,
        };
        let radius = Vec2 {
            x: (bounding_rect.width() - player_size.x) / 2.0,
            y: (bounding_rect.height() - player_size.y) / 2.0,
        };
        let table_fill =
            Shape::ellipse_filled(bounding_rect.center(), radius, Color32::from_rgb(0, 35, 0));
        painter.add(table_fill);

        let table_stroke = Shape::ellipse_stroke(
            bounding_rect.center(),
            radius * 0.99,
            Stroke::new(radius.x / 50.0, Color32::DARK_GRAY),
        );
        painter.add(table_stroke);

        for (player, center) in self
            .table_player_points(bounding_rect.center(), radius)
            .enumerate()
        {
            self.draw_player(painter, player, center, player_size);
        }
    }

    fn action_bar(&mut self, ui: &mut Ui) -> Result<()> {
        let bounding_rect = ui.clip_rect();
        let action_bar = Shape::rect_filled(
            bounding_rect,
            bounding_rect.width() / 100.0,
            Rgba::from_black_alpha(0.5),
        );
        ui.painter().add(action_bar);
        if self.game.current_player().is_none() {
            return Ok(());
        }

        // TODO: Buttons shift funny on really small window sizes.
        let button_width = bounding_rect.width() / 6.0;
        let button_height = bounding_rect.height() * 0.8;
        let text_styles = ui.ctx().style().text_styles.clone();
        ui.style_mut().text_styles.insert(
            TextStyle::Button,
            FontId::new(button_height / 4.0, FontFamily::Proportional),
        );
        if let Some((_, to)) = self.game.can_raise() {
            if ui
                .add_sized(
                    [button_width, button_height],
                    Button::new(format!("Raise\n{to}")),
                )
                .clicked()
            {
                self.game.raise(to)?;
            }
        }
        if let Some(amount) = self.game.can_bet() {
            if ui
                .add_sized(
                    [button_width, button_height],
                    Button::new(format!("Bet\n{amount}")),
                )
                .clicked()
            {
                self.game.bet(amount)?;
            }
        }
        if let Some(amount) = self.game.can_call() {
            if ui
                .add_sized(
                    [button_width, button_height],
                    Button::new(format!("Call\n{amount}")),
                )
                .clicked()
            {
                self.game.call()?;
            }
        }
        if self.game.can_check() {
            if ui
                .add_sized([button_width, button_height], Button::new("Check"))
                .clicked()
            {
                self.game.check()?;
            }
        }
        if ui
            .add_sized([button_width, button_height], Button::new("Fold"))
            .clicked()
        {
            self.game.fold()?;
        }
        ui.style_mut().text_styles = text_styles;
        Ok(())
    }

    fn draw_player(&self, painter: &Painter, player: usize, center: Pos2, size: Vec2) {
        let bounding_rect = Rect::from_center_size(center, size);
        let name_stack_rect = bounding_rect.with_min_y(bounding_rect.bottom() - size.y / 3.2);
        let name_stack_shape = Shape::rect_filled(
            name_stack_rect,
            name_stack_rect.width() / 50.0,
            Color32::BLACK,
        );
        painter.add(name_stack_shape);

        let name_rect = name_stack_rect.with_max_y(name_stack_rect.center().y);
        painter.text(
            name_rect.center(),
            Align2::CENTER_CENTER,
            self.game.player_name(player),
            FontId::new(name_rect.height() * 0.9, FontFamily::Proportional),
            Color32::WHITE,
        );
        let stack_rect = name_stack_rect.with_min_y(name_stack_rect.center().y);
        painter.text(
            stack_rect.center(),
            Align2::CENTER_CENTER,
            self.game.current_stacks()[player],
            FontId::new(stack_rect.height() * 0.9, FontFamily::Proportional),
            Color32::WHITE,
        );

        if !self.game.has_cards(player) {
            return;
        }
        let hand_bounding_rect = bounding_rect.with_max_y(name_stack_rect.top());
        let card_width = hand_bounding_rect.height() / 1.3;
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
        match self.visible_hand(player) {
            Some(hand) => {
                draw_card(painter, card_a_rect, hand.high());
                draw_card(painter, card_b_rect, hand.low());
            }
            None => {
                draw_hidden_card(painter, card_a_rect);
                draw_hidden_card(painter, card_b_rect);
            }
        }

        if self.game.current_player() == Some(player) {
            let name_stack_stroke = Shape::rect_stroke(
                name_stack_rect,
                name_stack_rect.width() / 50.0,
                Stroke::new(name_stack_rect.width() / 50.0, Color32::DARK_RED),
                StrokeKind::Middle,
            );
            painter.add(name_stack_stroke);
        }
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

    fn visible_hand(&self, player: usize) -> Option<Hand> {
        self.game.get_hand(player)
    }
}

fn draw_card(painter: &Painter, bounding_rect: Rect, card: Card) {
    let width = bounding_rect.width();
    let height = bounding_rect.height();
    assert!((width * 1.3 - height).abs() <= 0.1);
    let card_shape = Shape::rect_filled(bounding_rect, width / 10.0, suite_color(card.suite()));
    painter.add(card_shape);
    let rank_pos = Pos2 {
        x: bounding_rect.center_bottom().x,
        y: bounding_rect.center_bottom().y + width / 5.0,
    };
    painter.text(
        rank_pos,
        Align2::CENTER_BOTTOM,
        card.rank().to_string(),
        FontId::new(width * 1.3, FontFamily::Proportional),
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
        FontId::new(width / 4.0, FontFamily::Proportional),
        Color32::WHITE,
    );
}

fn draw_hidden_card(painter: &Painter, bounding_rect: Rect) {
    assert!((bounding_rect.width() * 1.3 - bounding_rect.height()).abs() <= 0.1);
    let card_shape = Shape::rect_filled(
        bounding_rect,
        bounding_rect.width() / 10.0,
        Rgba::from_black_alpha(0.5),
    );
    painter.add(card_shape);
}

fn suite_color(suite: Suite) -> Color32 {
    match suite {
        Suite::Diamonds => Color32::BLUE,
        Suite::Spades => Color32::from_rgb(35, 35, 35),
        Suite::Hearts => Color32::RED,
        Suite::Clubs => Color32::DARK_GREEN,
    }
}
