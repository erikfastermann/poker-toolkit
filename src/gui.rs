use eframe::{
    egui::{
        self, Align, Align2, Button, Color32, Context, DragValue, FontFamily, FontId, Id, Layout,
        Painter, Pos2, Rect, Rgba, Sense, Shape, Slider, Stroke, StrokeKind, Style, TextStyle, Ui,
        UiBuilder, Vec2, Visuals, Window,
    },
    Frame,
};
use egui_extras::{Column, TableBody, TableBuilder, TableRow};

use crate::{
    card::Card,
    cards::Cards,
    game::{Game, GameData, Player, State, Street},
    hand::Hand,
    rank::Rank,
    result::Result,
    suite::Suite,
};

// TODO:
// - Portable text drawing fonts
// - Max text sizes

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
    game_builder: GameBuilder,
}

impl App {
    fn new() -> Result<Self> {
        let mut game = Game::from_game_data(&GameData::default())?;
        game.post_small_and_big_blind()?;
        Ok(Self {
            game: GameView::new(game),
            game_builder: GameBuilder::new(),
        })
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut Frame) {
        egui::CentralPanel::default().show(ctx, |ui| self.game.view(ui).unwrap());
        if let Some(mut game) = self.game_builder.window(ctx, "New game".to_string()) {
            game.post_small_and_big_blind().unwrap();
            self.game = GameView::new(game);
        }
    }
}

struct GameView {
    game: Game,
    card_selector: CardSelector,
    current_amount: u32,
}

impl GameView {
    fn new(game: Game) -> Self {
        Self {
            game,
            card_selector: CardSelector::new(),
            current_amount: 0,
        }
    }

    fn view(&mut self, ui: &mut Ui) -> Result<()> {
        let table_height = ui.clip_rect().height() * 0.9;
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
        self.action_bar(ui, action_bar_bounding_rect)?;

        self.view_card_selector(ui)?;

        self.finalize()
    }

    fn finalize(&mut self) -> Result<()> {
        if self.game.can_next() {
            return Ok(());
        }

        loop {
            match self.game.state() {
                State::UncalledBet { .. } => {
                    self.game.uncalled_bet()?;
                }
                State::ShowOrMuck(_) => {
                    // TODO: Handle unknown hand more gracefully.
                    self.game.show_hand()?;
                }
                State::Showdown => self.game.showdown_simple()?,
                _ => return Ok(()),
            }
        }
    }

    fn draw_table(&self, painter: &Painter, bounding_rect: Rect) {
        let player_size = Vec2 {
            x: bounding_rect.width() / 6.0,
            y: bounding_rect.height() / 5.5,
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

        let mut card_size = Vec2::ZERO;
        for (player, center) in self
            .table_player_points(bounding_rect.center(), radius)
            .enumerate()
        {
            card_size = self.draw_player(painter, player, center, player_size);
            self.draw_invested(player, painter, bounding_rect, center);
        }

        draw_text_with_background(
            painter,
            format!("Total Pot: {}", self.game.total_pot()),
            bounding_rect.height() / 30.0,
            bounding_rect.center() - Vec2::new(0.0, card_size.y * 0.75),
            Rgba::from_black_alpha(0.3),
        );
        self.draw_board(
            painter,
            bounding_rect.center() + Vec2::new(0.0, card_size.y * 0.25),
            card_size,
        );
    }

    fn action_bar(&mut self, ui: &mut Ui, bounding_rect: Rect) -> Result<()> {
        // TODO: Controls shift funny on small window sizes.

        let action_bar = Shape::rect_filled(
            bounding_rect,
            bounding_rect.width() / 100.0,
            Rgba::from_black_alpha(0.5),
        );
        ui.painter().add(action_bar);

        let inner_bounding_rect = bounding_rect
            .with_min_x(bounding_rect.min.x + bounding_rect.size().x / 100.0)
            .with_max_x(bounding_rect.max.x - bounding_rect.size().x / 100.0);
        let text_styles = ui.ctx().style().text_styles.clone();
        ui.style_mut().text_styles.insert(
            TextStyle::Button,
            FontId::new(inner_bounding_rect.height() / 6.0, FontFamily::Proportional),
        );
        let result = self.action_bar_inner(ui, inner_bounding_rect);
        ui.style_mut().text_styles = text_styles;
        result
    }

    fn action_bar_inner(&mut self, ui: &mut Ui, bounding_rect: Rect) -> Result<()> {
        let upper_elements_max_y = bounding_rect.min.y + bounding_rect.size().y * 0.4;
        ui.allocate_new_ui(
            UiBuilder::new()
                .max_rect(bounding_rect.with_max_y(upper_elements_max_y))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| self.action_bar_upper_left_buttons(ui),
        )
        .inner?;
        ui.allocate_new_ui(
            UiBuilder::new()
                .max_rect(bounding_rect.with_min_y(upper_elements_max_y))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| self.action_bar_history_buttons(ui),
        )
        .inner?;

        if self.game.can_next() || self.game.current_player().is_none() {
            return Ok(());
        }
        ui.allocate_new_ui(
            UiBuilder::new()
                .max_rect(bounding_rect.with_max_y(upper_elements_max_y))
                .layout(Layout::right_to_left(Align::Center)),
            |ui| self.action_bar_upper_right_elements(ui),
        )
        .inner?;
        ui.allocate_new_ui(
            UiBuilder::new()
                .max_rect(bounding_rect.with_min_y(upper_elements_max_y))
                .layout(Layout::right_to_left(Align::Center)),
            |ui| self.action_bar_lower_buttons(ui),
        )
        .inner
    }

    fn action_bar_history_buttons(&mut self, ui: &mut Ui) -> Result<()> {
        let bounding_rect = ui.max_rect();
        let button_height = bounding_rect.height() * 0.8;
        let button_width = button_height;
        let button_size = Vec2::new(button_width, button_height);

        let previous_button = ui
            .add_enabled_ui(self.game.can_previous(), |ui| {
                ui.add_sized(button_size, Button::new("<"))
            })
            .inner;
        if previous_button.clicked() {
            assert!(self.game.previous());
        }

        let next_button = ui
            .add_enabled_ui(self.game.can_next(), |ui| {
                ui.add_sized(button_size, Button::new(">"))
            })
            .inner;
        if next_button.clicked() {
            assert!(self.game.next());
        }

        Ok(())
    }

    fn action_bar_lower_buttons(&mut self, ui: &mut Ui) -> Result<()> {
        let bounding_rect = ui.max_rect();
        let button_width = bounding_rect.width() / 6.0;
        let button_height = bounding_rect.height() * 0.8;
        let mut did_action = false;

        if self.game.can_raise().is_some() {
            if ui
                .add_sized(
                    [button_width, button_height],
                    Button::new(format!("Raise\n{}", self.current_amount)),
                )
                .clicked()
            {
                self.game.raise(self.current_amount)?;
                did_action = true;
            }
        } else if self.game.can_bet().is_some() {
            if ui
                .add_sized(
                    [button_width, button_height],
                    Button::new(format!("Bet\n{}", self.current_amount)),
                )
                .clicked()
            {
                self.game.bet(self.current_amount)?;
                did_action = true;
            }
        } else {
            ui.allocate_space(Vec2::new(button_width, button_height));
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
                did_action = true;
            }
        } else if self.game.can_check() {
            if ui
                .add_sized([button_width, button_height], Button::new("Check"))
                .clicked()
            {
                self.game.check()?;
                did_action = true;
            }
        }

        if ui
            .add_sized([button_width, button_height], Button::new("Fold"))
            .clicked()
        {
            self.game.fold()?;
            did_action = true;
        }

        if did_action {
            self.current_amount = 0;
        }
        Ok(())
    }

    fn action_bar_upper_left_buttons(&self, ui: &mut Ui) -> Result<()> {
        let bounding_rect = ui.max_rect();
        let button_size = Vec2::new(bounding_rect.width() / 15.0, bounding_rect.height() * 0.8);
        if ui.add_sized(button_size, Button::new("📋")).clicked() {
            let json = serde_json::to_string_pretty(&self.game.clone().to_validation_data())?;
            ui.ctx().copy_text(json);
        }
        Ok(())
    }

    fn action_bar_upper_right_elements(&mut self, ui: &mut Ui) -> Result<()> {
        let Some(amount) = self
            .game
            .can_bet()
            .or_else(|| self.game.can_raise().map(|(_, to)| to))
        else {
            return Ok(());
        };
        let bounding_rect = ui.max_rect();
        let current_player = self.game.current_player().unwrap();
        let value_range = amount..=self.game.previous_street_stacks()[current_player];
        let drag_value = DragValue::new(&mut self.current_amount)
            .range(value_range.clone())
            .speed(1.0);
        let control_height = bounding_rect.height() * 0.8;
        ui.add_sized([bounding_rect.width() / 10.0, control_height], drag_value);
        let slider = Slider::new(&mut self.current_amount, value_range).show_value(false);
        ui.add_sized([bounding_rect.width() / 10.0, control_height], slider);

        let button_size = [bounding_rect.width() / 15.0, control_height];
        // TODO: Configurable percent / big blind buttons.
        const PERCENT_BUTTONS: &[(&str, f64)] = &[("20%", 0.2), ("60%", 0.6), (("130%", 1.3))];
        const BIG_BLIND_BUTTONS: &[(&str, f64)] = &[("2BB", 2.0), ("2.5BB", 2.5), (("3BB", 3.0))];

        let use_big_blind_buttons =
            self.game.board().street() == Street::PreFlop && !self.game.raise_in_street();
        let button_config = if use_big_blind_buttons {
            BIG_BLIND_BUTTONS
        } else {
            PERCENT_BUTTONS
        };
        for button_config in button_config.iter().rev() {
            let button = Button::new(button_config.0);
            if ui.add_sized(button_size, button).clicked() {
                if use_big_blind_buttons {
                    self.current_amount =
                        (f64::from(self.game.big_blind()) * button_config.1) as u32;
                } else {
                    self.current_amount =
                        (f64::from(self.game.total_pot()) * button_config.1) as u32;
                }
            }
        }

        Ok(())
    }

    fn view_card_selector(&mut self, ui: &mut Ui) -> Result<()> {
        if self.game.can_next() {
            return Ok(());
        }
        let State::Street(street) = self.game.state() else {
            return Ok(());
        };
        match street {
            Street::PreFlop => unreachable!(),
            Street::Flop => self.card_selector.set_min_max(3, 3),
            Street::Turn => self.card_selector.set_min_max(1, 1),
            Street::River => self.card_selector.set_min_max(1, 1),
        }
        self.card_selector.set_disabled(self.game.known_cards());
        if self
            .card_selector
            .window(ui.ctx(), format!("Select {street}"))
        {
            match street {
                Street::PreFlop => unreachable!(),
                Street::Flop => self
                    .game
                    .flop(self.card_selector.in_order.as_slice().try_into().unwrap())?,
                Street::Turn => self.game.turn(self.card_selector.in_order[0])?,
                Street::River => self.game.river(self.card_selector.in_order[0])?,
            }
            self.card_selector.reset();
        }
        Ok(())
    }

    fn draw_player(&self, painter: &Painter, player: usize, center: Pos2, size: Vec2) -> Vec2 {
        let bounding_rect = Rect::from_center_size(center, size);
        let mut name_stack_rect = bounding_rect.with_min_y(bounding_rect.bottom() - size.y / 3.2);
        let name_stack_shape = Shape::rect_filled(
            name_stack_rect,
            name_stack_rect.width() / 50.0,
            Color32::BLACK,
        );
        painter.add(name_stack_shape);
        let full_name_stack_rect = name_stack_rect;
        if self.game.button_index() == player {
            let button_radius = name_stack_rect.height() / 4.0;
            let button_center = name_stack_rect.center()
                + (name_stack_rect.right_center() - name_stack_rect.center()) * 0.75;
            painter.circle_filled(button_center, button_radius, Color32::from_rgb(200, 200, 0));
            name_stack_rect.set_right(button_center.x - button_radius);
        }

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

        if !self.game.has_cards(player) {
            return card_a_rect.size();
        }
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
                full_name_stack_rect,
                full_name_stack_rect.width() / 50.0,
                Stroke::new(full_name_stack_rect.width() / 50.0, Color32::DARK_RED),
                StrokeKind::Middle,
            );
            painter.add(name_stack_stroke);
        }
        card_a_rect.size()
    }

    fn draw_invested(
        &self,
        player: usize,
        painter: &Painter,
        bounding_rect: Rect,
        player_center: Pos2,
    ) {
        if self.game.current_player().is_none() {
            return;
        }
        let invested = self.game.invested_in_street(player);
        if invested == 0 {
            return;
        }
        let invested_point = player_center + (bounding_rect.center() - player_center) * 0.4;
        let text_size = bounding_rect.height() / 30.0;
        draw_text_with_background(
            painter,
            invested.to_string(),
            text_size,
            invested_point,
            Rgba::from_black_alpha(0.3),
        );
    }

    fn draw_board(&self, painter: &Painter, center: Pos2, card_size: Vec2) {
        let space = card_size.x / 10.0;
        let board_rect = Rect::from_center_size(
            center,
            Vec2::new(
                card_size.x * Game::TOTAL_CARDS as f32 + space * (Game::TOTAL_CARDS - 1) as f32,
                card_size.y,
            ),
        );
        for (i, card) in self.game.board().cards().iter().copied().enumerate() {
            let left = board_rect.left() + i as f32 * (card_size.x + space);
            let card_rect = board_rect.with_min_x(left).with_max_x(left + card_size.x);
            draw_card(painter, card_rect, card);
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
    let painter = painter.with_clip_rect(bounding_rect);
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
        FontId::new(width * 1.2, FontFamily::Monospace),
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

fn draw_text_with_background(
    painter: &Painter,
    text: String,
    text_size: f32,
    center: Pos2,
    background_color: impl Into<Color32>,
) -> Rect {
    let space = Vec2::new(text_size / 2.0, text_size / 4.0);
    let galley = painter.layout_no_wrap(
        text,
        FontId::new(text_size, FontFamily::Monospace),
        Color32::WHITE,
    );
    let background_rect = Rect::from_center_size(center, galley.rect.size() + 2.0 * space);
    painter.rect_filled(
        background_rect,
        background_rect.height() / 2.0,
        background_color,
    );
    painter.galley(background_rect.left_top() + space, galley, Color32::WHITE);
    background_rect
}

fn suite_color(suite: Suite) -> Color32 {
    match suite {
        Suite::Diamonds => Color32::BLUE,
        Suite::Spades => Color32::from_rgb(35, 35, 35),
        Suite::Hearts => Color32::RED,
        Suite::Clubs => Color32::DARK_GREEN,
    }
}

struct GameBuilder {
    game: GameData,
    hand_selector: CardSelector,
    hand_selector_for: Option<usize>,
    error: String,
    remove_hand: Option<usize>,
    remove_player: Option<usize>,
    swap_players: Option<(usize, usize)>,
}

impl GameBuilder {
    const ROW_HEIGHT: f32 = 20.0;

    fn new() -> Self {
        let mut hand_selector = CardSelector::new();
        hand_selector.set_min_max(2, 2);
        Self {
            game: GameData::default(),
            hand_selector,
            hand_selector_for: None,
            error: String::new(),
            remove_hand: None,
            remove_player: None,
            swap_players: None,
        }
    }

    fn window(&mut self, ctx: &Context, title: String) -> Option<Game> {
        let response = Window::new(title)
            .resizable([true, true])
            .default_size([450.0, 600.0])
            .show(ctx, |ui| self.view(ui));
        response.and_then(|inner| inner.inner).flatten()
    }

    fn view(&mut self, ui: &mut Ui) -> Option<Game> {
        self.view_reset();
        self.blinds(ui);
        ui.separator();
        self.players_table(ui);
        self.hand_selector(ui);
        ui.separator();
        self.bottom_controls(ui)
    }

    fn view_reset(&mut self) {
        self.remove_hand = None;
        self.remove_player = None;
        self.swap_players = None;
    }

    fn blinds(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Small blind:");
            ui.add(DragValue::new(&mut self.game.small_blind));
            ui.separator();

            ui.label("Big blind:");
            ui.add(DragValue::new(&mut self.game.big_blind));
            ui.separator();

            if ui.button("100BB Stacks").clicked() {
                let Some(stack) = self.game.big_blind.checked_mul(100) else {
                    return;
                };
                for player in self.game.players.iter_mut() {
                    player.starting_stack = stack;
                }
            }
        });
    }

    fn players_table(&mut self, ui: &mut Ui) {
        let table = TableBuilder::new(ui)
            .striped(true)
            .cell_layout(Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto());

        table
            .header(Self::ROW_HEIGHT, |mut header| {
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Stack");
                });
                header.col(|ui| {
                    ui.strong("Hand");
                });
                header.col(|ui| {
                    ui.strong("Button");
                });
            })
            .body(|body| self.players_table_body(body));
    }

    fn players_table_body(&mut self, mut body: TableBody<'_>) {
        for player_index in 0..self.game.players.len() {
            body.row(Self::ROW_HEIGHT, |row| {
                self.players_table_row(row, player_index)
            });
        }

        self.players_table_add_remove();
    }

    fn players_table_row(&mut self, mut row: TableRow<'_, '_>, player_index: usize) {
        let player_count = self.game.players.len();

        row.col(|ui| self.players_table_name_column(ui, player_index));

        row.col(|ui| {
            let player = &mut self.game.players[player_index];
            let drag_value = DragValue::new(&mut player.starting_stack).speed(1.0);
            ui.add(drag_value);
        });

        row.col(|ui| self.players_table_hand_column(ui, player_index));

        row.col(|ui| {
            ui.radio_value(
                &mut self.game.button_index,
                u8::try_from(player_index).unwrap(),
                "",
            );
        });

        row.col(|ui| {
            if ui.button("⬇").clicked() {
                self.swap_players = Some((player_index, (player_index + 1) % player_count));
            };
        });

        row.col(|ui| {
            if ui.button("⬆").clicked() {
                self.swap_players = Some((
                    player_index,
                    player_index
                        .checked_sub(1)
                        .unwrap_or_else(|| player_count - 1),
                ));
            };
        });

        row.col(|ui| {
            if ui.button("🗑").clicked() {
                self.remove_player = Some(player_index);
            };
        });
    }

    fn players_table_name_column(&mut self, ui: &mut Ui, player_index: usize) {
        let player_count = self.game.players.len();
        let player = &mut self.game.players[player_index];

        let position_name = Game::position_name(
            player_count,
            usize::from(self.game.button_index),
            player_index,
        )
        .map(|(short_name, _)| short_name)
        .unwrap_or("Player");

        let mut name = player
            .name
            .clone()
            .unwrap_or_else(|| position_name.to_string());

        ui.text_edit_singleline(&mut name);

        if name.is_empty() || name == position_name {
            player.name = None;
        } else {
            player.name = Some(name);
        }
    }

    fn players_table_hand_column(&mut self, ui: &mut Ui, player_index: usize) {
        let known_cards = self
            .game
            .players
            .iter()
            .filter_map(|player| player.hand)
            .flat_map(|hand| hand.to_cards().iter())
            .fold(Cards::EMPTY, |cards, card| cards.with(card));
        let player = &mut self.game.players[player_index];

        let text = if let Some(hand) = player.hand {
            hand.to_string()
        } else {
            "...".to_string()
        };

        if ui.button(text).clicked() {
            let known_cards = if let Some(hand) = player.hand {
                known_cards.without(hand.high()).without(hand.low())
            } else {
                known_cards
            };
            self.hand_selector.set_disabled(known_cards);
            // TODO: Cancel select hand.
            self.hand_selector_for = Some(player_index);
        };

        if player.hand.is_some() {
            if ui.button("🗑").clicked() {
                self.remove_hand = Some(player_index);
            };
        }
    }

    fn players_table_add_remove(&mut self) {
        if self.hand_selector_for.is_some() {
            return;
        }

        if let Some(player) = self.remove_hand {
            self.game.players[player].hand = None;
        }

        if let Some(remove_player) = self.remove_player {
            if self.game.players.len() > Game::MIN_PLAYERS {
                self.game.players.remove(remove_player);

                if remove_player == usize::from(self.game.button_index) {
                    self.game.button_index = 0;
                }
            }
        }

        if let Some((a, b)) = self.swap_players {
            self.game.players.swap(a, b);

            let a = u8::try_from(a).unwrap();
            let b = u8::try_from(b).unwrap();
            if self.game.button_index == a {
                self.game.button_index = b;
            } else if self.game.button_index == b {
                self.game.button_index = a;
            }
        }
    }

    fn hand_selector(&mut self, ui: &mut Ui) {
        if let Some(player) = self.hand_selector_for {
            if self
                .hand_selector
                .window(ui.ctx(), "Select hand".to_string())
            {
                let hand = self.hand_selector.cards.to_hand().unwrap();
                self.game.players[player].hand = Some(hand);
                self.hand_selector.reset();
                self.hand_selector_for = None;
            }
        }
    }

    fn bottom_controls(&mut self, ui: &mut Ui) -> Option<Game> {
        let game = ui
            .horizontal(|ui| {
                let game = if ui.button("Create").clicked() {
                    self.error = String::new();
                    match Game::from_game_data(&self.game) {
                        Ok(game) => Some(game),
                        Err(err) => {
                            self.error = err.to_string();
                            None
                        }
                    }
                } else {
                    None
                };

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("+").clicked()
                        && self.hand_selector_for.is_none()
                        && self.game.players.len() < Game::MAX_PLAYERS
                    {
                        let starting_stack = self.game.big_blind.saturating_mul(100);
                        self.game
                            .players
                            .push(Player::with_starting_stack(starting_stack));
                    };
                });

                game
            })
            .inner;

        if !self.error.is_empty() {
            ui.label(&self.error);
        }
        game
    }
}

struct CardSelector {
    cards: Cards,
    in_order: Vec<Card>,
    disabled: Cards,
    min: usize,
    max: usize,
}

impl CardSelector {
    fn new() -> Self {
        Self {
            cards: Cards::EMPTY,
            in_order: Vec::new(),
            disabled: Cards::EMPTY,
            min: 1,
            max: 1,
        }
    }

    fn reset(&mut self) {
        self.cards = Cards::EMPTY;
        self.in_order.truncate(0);
    }

    fn set_min_max(&mut self, min: usize, max: usize) {
        assert!(max <= Card::COUNT);
        assert!(min <= max);
        assert!(max > 0);
        self.min = min;
        self.max = max;
    }

    fn set_disabled(&mut self, disabled: Cards) {
        for card in (self.cards & disabled).iter() {
            self.remove_card(card);
        }
        self.disabled = disabled;
        assert_eq!(self.cards & self.disabled, Cards::EMPTY);
    }

    fn window(&mut self, ctx: &Context, title: String) -> bool {
        let response = Window::new(title)
            .resizable([true, true])
            .default_size([400.0, 200.0])
            .show(ctx, |ui| self.view(ui));
        response.is_some_and(|apply| apply.inner == Some(true))
    }

    fn view(&mut self, ui: &mut Ui) -> bool {
        // TODO: This inside a Window has some weird resize behavior.

        let bounding_rect = ui.max_rect();
        let candidate_height = bounding_rect.width() / 2.0;
        let (inner_width, inner_height) = if candidate_height <= bounding_rect.height() {
            (bounding_rect.width(), candidate_height)
        } else {
            (bounding_rect.height() * 2.0, bounding_rect.height())
        };
        let bounding_rect = Rect::from_two_pos(
            bounding_rect.left_top(),
            Pos2::new(
                bounding_rect.left() + inner_width,
                bounding_rect.top() + inner_height,
            ),
        );
        assert!((bounding_rect.width() - bounding_rect.height() * 2.0).abs() <= 0.1);
        ui.allocate_rect(bounding_rect, Sense::click());
        let space = bounding_rect.width() / (Rank::COUNT * 3 + 1) as f32;
        let width = space * 2.0;

        let size = Vec2::new(width, width * 1.3);
        let space = width / 2.0;
        let start_x_offset = bounding_rect.left() + space + size.x / 2.0;
        let mut y_offset = bounding_rect.top() + space + size.y / 2.0;
        for suite in Suite::SUITES {
            let mut x_offset = start_x_offset;
            for rank in Rank::RANKS.into_iter().rev() {
                let card = Card::of(rank, suite);
                let card_rect = Rect::from_center_size(Pos2::new(x_offset, y_offset), size);
                self.card(ui, card_rect, card);
                x_offset += space + size.x;
            }
            y_offset += space + size.y;
        }

        ui.allocate_new_ui(
            UiBuilder::new()
                .max_rect(
                    bounding_rect
                        .with_min_y(y_offset)
                        .with_min_x(bounding_rect.left() + space),
                )
                .layout(Layout::left_to_right(Align::LEFT)),
            |ui| self.bar(ui),
        )
        .inner
    }

    fn card(&mut self, ui: &mut Ui, card_rect: Rect, card: Card) {
        draw_card(ui.painter(), card_rect, card);
        if self.cards.has(card) {
            ui.painter().rect(
                card_rect,
                card_rect.width() / 10.0,
                Rgba::from_black_alpha(0.3),
                Stroke::new(
                    card_rect.width() / 10.0,
                    Rgba::from_luminance_alpha(1.0, 0.5),
                ),
                StrokeKind::Middle,
            );
        }
        if self.disabled.has(card) {
            ui.painter().rect_filled(
                card_rect,
                card_rect.width() / 10.0,
                Rgba::from_black_alpha(0.8),
            );
        }
        let interact = ui.interact(card_rect, Id::new(card), Sense::click());
        if interact.clicked() && !self.disabled.has(card) {
            if self.cards.has(card) {
                self.remove_card(card);
            } else {
                self.add_card(card);
            }
        }
    }

    fn bar(&self, ui: &mut Ui) -> bool {
        let count = usize::from(self.cards.count());
        let enabled = count >= self.min && count <= self.max;
        let clicked = ui.add_enabled(enabled, Button::new("Apply")).clicked();
        let label = if self.min == self.max {
            format!("Add {} cards", self.min)
        } else {
            format!("Add {} to {} cards", self.min, self.max)
        };
        ui.label(label);
        clicked
    }

    fn add_card(&mut self, card: Card) {
        self.cards.add(card);
        self.in_order.push(card);
    }

    fn remove_card(&mut self, card: Card) {
        self.cards.remove(card);
        self.in_order.retain(|current_card| *current_card != card);
    }
}
