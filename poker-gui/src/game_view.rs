use std::{collections::HashMap, fmt::Write, fs, mem, sync::Arc};

use eframe::egui::{
    Align, Align2, Button, Color32, Context, DragValue, FontFamily, FontId, Id, Layout, Painter,
    Pos2, Rect, Rgba, Shape, Slider, Stroke, StrokeKind, TextStyle, Ui, UiBuilder, Vec2,
};

use poker_core::{
    ai::{AlwaysAllIn, AlwaysCheckCall, AlwaysFold, PlayerActionGenerator, SimpleStrategy},
    game::{Action, Game, GameData, State, Street},
    hand::Hand,
    range::{PreFlopRangeConfig, PreFlopRangeConfigData},
    result::Result,
};

use crate::{
    card::{draw_card, draw_hidden_card},
    card_selector::CardSelector,
    game_builder::GameBuilder,
    range_viewer::{RangeValue, RangeViewer},
};

// TODO:
// - Portable text drawing fonts
// - Max text sizes

pub struct GameView {
    game: Game,
    card_selector: CardSelector,
    enable_game_builder: bool,
    game_builder: GameBuilder,
    player_action_generators: Vec<(
        &'static str,
        Box<dyn FnMut() -> Box<dyn PlayerActionGenerator>>,
    )>,

    // TODO: Offload to background thread.
    current_player_action_generators: HashMap<usize, Box<dyn PlayerActionGenerator>>,
    /// The keys are the indices of the inserted action values.
    /// The values contain the length of the matching generator logs.
    current_range_histories: HashMap<usize, (Vec<RangeValue>, usize)>,
    current_generator_logs: Vec<String>,
    last_applied_action_index: usize,
    range_viewer: RangeViewer,

    current_amount: u32,
    pick_community_cards: bool,
    show_all_hands: bool,
}

impl GameView {
    pub fn new() -> Self {
        Self::new_inner(None).unwrap()
    }

    pub fn new_with_simple_strategy(pre_flop_ranges_config_path: &str) -> Result<Self> {
        Self::new_inner(Some(pre_flop_ranges_config_path))
    }

    fn new_inner(pre_flop_ranges_config_path: Option<&str>) -> Result<Self> {
        let mut player_action_generators: Vec<(
            &'static str,
            Box<dyn FnMut() -> Box<dyn PlayerActionGenerator>>,
        )> = vec![
            ("Fold", Box::new(|| Box::new(AlwaysFold))),
            ("Check/Call", Box::new(|| Box::new(AlwaysCheckCall))),
            ("AllIn", Box::new(|| Box::new(AlwaysAllIn))),
        ];

        let default_player_action = if let Some(path) = pre_flop_ranges_config_path {
            let pre_flop_ranges_raw = fs::read_to_string(path)?;
            let pre_flop_ranges: PreFlopRangeConfigData =
                serde_json::from_str(&pre_flop_ranges_raw)?;
            let pre_flop_ranges = Arc::new(PreFlopRangeConfig::from_data(pre_flop_ranges)?);

            player_action_generators.push((
                "Simple",
                Box::new(move || Box::new(SimpleStrategy::new(pre_flop_ranges.clone()))),
            ));

            player_action_generators.len() - 1
        } else {
            0
        };

        let player_action_generator_names = player_action_generators
            .iter()
            .map(|(name, _)| *name)
            .collect();

        let mut game_view = Self {
            game: Game::from_game_data(&GameData::default()).unwrap(),
            card_selector: CardSelector::new(),
            enable_game_builder: true,
            game_builder: GameBuilder::new(
                player_action_generator_names,
                Some(default_player_action),
            ),
            player_action_generators,
            current_player_action_generators: HashMap::new(),
            current_range_histories: HashMap::new(),
            current_generator_logs: vec![String::new(); Game::MAX_PLAYERS],
            last_applied_action_index: 2, // Skip initial posts.
            range_viewer: RangeViewer::new(),
            current_amount: 0,
            pick_community_cards: false,
            show_all_hands: true,
        };

        game_view.game.draw_unset_hands(&mut rand::thread_rng());
        game_view.game.post_small_and_big_blind().unwrap();

        Ok(game_view)
    }

    pub fn set_enable_game_builder(&mut self, enable_game_builder: bool) {
        self.enable_game_builder = enable_game_builder;
    }

    pub fn set_pick_community_cards(&mut self, pick_community_cards: bool) {
        self.pick_community_cards = pick_community_cards;
    }

    pub fn with_game_mut(&mut self, f: impl FnOnce(&mut Game)) -> Result<()> {
        f(&mut self.game);

        // TODO: Draw unset hands if required.

        if self.game.state() == State::Post && !self.game.can_next() {
            self.game.post_small_and_big_blind()?;
        }

        self.current_amount = 0;

        Ok(())
    }

    pub fn view(&mut self, ui: &mut Ui) -> Result<()> {
        let bounding_rect = ui.max_rect();
        assert!(((bounding_rect.width() / 4.0 * 3.0) - bounding_rect.height()).abs() <= 0.1);

        let table_bounding_rect =
            bounding_rect.with_max_y(bounding_rect.bottom() - bounding_rect.height() / 6.0);
        self.draw_table(ui.painter(), table_bounding_rect);

        let action_bar_bounding_rect =
            bounding_rect.with_min_y(table_bounding_rect.bottom() + bounding_rect.height() / 100.0);
        self.action_bar(ui, action_bar_bounding_rect)?;

        self.view_card_selector(ui)?;

        self.view_game_builder(ui.ctx())?;

        self.view_ranges(ui.ctx());

        self.finalize(ui.ctx())
    }

    fn finalize(&mut self, ctx: &Context) -> Result<()> {
        if self.game.can_next() {
            return Ok(());
        }

        self.apply_action_to_villains()?;

        match self.game.state() {
            State::Player(player)
                if self.current_player_action_generators.contains_key(&player) =>
            {
                let mut temp_log = String::new();

                // TODO: Gracefully handle errors and check action is valid.
                let (action, config, ranges) = self
                    .current_player_action_generators
                    .get_mut(&player)
                    .unwrap()
                    .update_hero(&self.game, &mut temp_log)?;

                action.apply_to_game(&mut self.game)?;

                let mut ranges_history_entry = vec![RangeValue::Full(config)];
                if let Some(ranges) = ranges {
                    for current_player in self.game.players_not_folded() {
                        if current_player == player {
                            continue;
                        }

                        // TODO: Check index is valid.
                        ranges_history_entry
                            .push(RangeValue::Simple(ranges[current_player].clone()))
                    }
                }

                let current_log_offset = self.write_generator_log(player, "Hero", &temp_log);

                self.current_range_histories.insert(
                    self.game.actions().len(),
                    (ranges_history_entry, current_log_offset),
                );

                self.apply_action_to_villains()?;
            }
            State::Street(_) if !self.pick_community_cards => {
                let mut rng = rand::thread_rng();
                self.game.draw_next_street(&mut rng)?;
            }
            State::UncalledBet { .. } => {
                self.game.uncalled_bet()?;
            }
            State::ShowOrMuck(_) => {
                self.game.show_hand()?;
            }
            State::ShowdownOrNextRunout => self.game.showdown_simple()?,
            _ => return Ok(()),
        }

        ctx.request_repaint();
        Ok(())
    }

    fn apply_action_to_villains(&mut self) -> Result<()> {
        if self.game.can_next() {
            return Ok(());
        }

        if self.game.actions().len() <= self.last_applied_action_index {
            return Ok(());
        }

        let Some(action) = self.game.actions().last().copied() else {
            return Ok(());
        };

        let Some(action_player) = action.player() else {
            return Ok(());
        };

        let mut generators = mem::take(&mut self.current_player_action_generators);

        for (player, action_generator) in &mut generators {
            if self.game.folded(*player) || *player == action_player {
                continue;
            }

            let mut temp_log = String::new();

            // TODO: Handle errors gracefully and don't forget to restore the generator hashmap.
            action_generator.update_villain(&self.game, &mut temp_log)?;

            self.write_generator_log(
                *player,
                &format!("Villain {}", self.game.player_name(action_player)),
                &temp_log,
            );
        }

        self.current_player_action_generators = generators;
        self.last_applied_action_index = self.game.actions().len();

        Ok(())
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

        if self.game.can_next()
            || self.game.current_player().is_none()
            || self
                .current_player_action_generators
                .contains_key(&self.game.current_player().unwrap())
        {
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

        let rewind_button = ui
            .add_enabled_ui(self.game.can_previous(), |ui| {
                ui.add_sized(button_size, Button::new("<<"))
            })
            .inner;
        if rewind_button.clicked() {
            self.game.rewind();
        }

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

        let forward_button = ui
            .add_enabled_ui(self.game.can_next(), |ui| {
                ui.add_sized(button_size, Button::new(">>"))
            })
            .inner;
        if forward_button.clicked() {
            self.game.forward();
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

    fn action_bar_upper_left_buttons(&mut self, ui: &mut Ui) -> Result<()> {
        let bounding_rect = ui.max_rect();
        let widget_size = Vec2::new(bounding_rect.width() / 15.0, bounding_rect.height() * 0.8);

        if ui.add_sized(widget_size, Button::new("📋")).clicked() {
            let json = serde_json::to_string_pretty(&self.game.clone().to_validation_data())?;
            ui.ctx().copy_text(json);
        }

        ui.checkbox(&mut self.show_all_hands, "Show all");

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

        let use_big_blind_buttons = self.game.board().street() == Street::PreFlop
            && !self
                .game
                .actions_in_street()
                .iter()
                .any(|action| matches!(action, Action::Raise { .. }));
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
                    .flop(self.card_selector.cards_in_order().try_into().unwrap())?,
                Street::Turn => self.game.turn(self.card_selector.cards_in_order()[0])?,
                Street::River => self.game.river(self.card_selector.cards_in_order()[0])?,
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
            self.stack_text(player),
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

        if self.game.folded(player) || self.game.hand_mucked(player) {
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

    fn stack_text(&self, player: usize) -> String {
        let show_last_action = self.game.state() != State::End
            && self
                .game
                .actions()
                .last()
                .filter(|action| !matches!(action, Action::Post { .. } | Action::Straddle { .. }))
                .and_then(|action| action.player_all())
                .is_some_and(|action_player| action_player == player);

        if show_last_action {
            let action = self.game.actions().last().unwrap();
            return action.kind_str().to_uppercase();
        }

        let only_post_straddle = self
            .game
            .actions()
            .iter()
            .all(|action| matches!(action, Action::Post { .. } | Action::Straddle { .. }));

        if only_post_straddle {
            let action = self
                .game
                .actions()
                .iter()
                .copied()
                .rev()
                .find(|action| action.player().is_some_and(|current| current == player));

            if let Some(action) = action {
                return action.kind_str().to_uppercase();
            }
        }

        self.game.current_stacks()[player].to_string()
    }

    fn draw_invested(
        &self,
        player: usize,
        painter: &Painter,
        bounding_rect: Rect,
        player_center: Pos2,
    ) {
        if self.game.state() == State::End {
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

    fn view_game_builder(&mut self, ctx: &Context) -> Result<()> {
        if !self.enable_game_builder {
            return Ok(());
        }

        let Some(config) = self.game_builder.window(ctx, "Configure game".to_string()) else {
            return Ok(());
        };

        self.with_game_mut(|game| *game = config.game)?;
        self.game.draw_unset_hands(&mut rand::thread_rng());
        self.set_pick_community_cards(config.pick_community_cards);

        self.current_range_histories = HashMap::new();
        self.current_generator_logs
            .iter_mut()
            .for_each(String::clear);
        self.last_applied_action_index = 2; // Skip initial posts.
        self.current_player_action_generators.clear();
        self.range_viewer = RangeViewer::new();

        for (player_index, player) in config.players.into_iter().enumerate() {
            let Some(ai_index) = player.action_generator else {
                continue;
            };
            let action_generator = (self.player_action_generators[ai_index].1)();
            self.current_player_action_generators
                .insert(player_index, action_generator);
        }

        ctx.request_repaint();
        Ok(())
    }

    fn view_ranges(&mut self, ctx: &Context) {
        let Some((ranges, log_offset)) =
            self.current_range_histories.get(&self.game.actions().len())
        else {
            return;
        };

        let player = self.game.actions().last().unwrap().player().unwrap();

        self.range_viewer.replace_ranges(ranges.clone());
        self.range_viewer
            .set_details(self.current_generator_logs[player][..*log_offset].to_owned());

        let villain_title = if self.range_viewer.selected() != 0 {
            // TODO: A little hacky that this depends on the concrete insertion order in the array.
            let villain = self
                .game
                .players_not_folded()
                .filter(|villain| *villain != player)
                .nth(self.range_viewer.selected() - 1)
                .unwrap();

            format!(" ({})", self.game.player_name(villain))
        } else {
            String::new()
        };

        let title = format!("Range - {}{}", self.game.player_name(player), villain_title);
        // TODO: Nicer rendering and default position of window.
        self.range_viewer.window(ctx, Id::new("Range"), title);
    }

    fn write_generator_log(&mut self, player: usize, name: &str, log: &str) -> usize {
        let out = &mut self.current_generator_logs[player];

        if log.is_empty() {
            out.len()
        } else {
            write!(
                out,
                "{name} at {} - {}:\n\n{log}",
                self.game.board().street(),
                self.game
                    .actions_in_street()
                    .iter()
                    .filter_map(|action| action.player_char())
                    .collect::<String>(),
            )
            .unwrap();
            let offset = out.trim_end().len();
            out.push_str("\n---------\n\n");
            offset
        }
    }

    fn visible_hand(&self, player: usize) -> Option<Hand> {
        if self.show_all_hands
            || self.game.hand_shown(player)
            || !self.current_player_action_generators.contains_key(&player)
        {
            self.game.get_hand(player)
        } else {
            None
        }
    }
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
