use eframe::egui::{self, Align, ComboBox, Context, DragValue, Layout, Ui, Window};
use egui_extras::{Column, TableBody, TableBuilder, TableRow};

use poker_core::{
    cards::Cards,
    game::{Game, GameData, Player},
};

use crate::card_selector::CardSelector;

pub struct GameBuilderConfig {
    pub game: Game,
    pub players: Vec<GameBuilderPlayer>,
    pub pick_community_cards: bool,
}

#[derive(Clone)]
pub struct GameBuilderPlayer {
    pub player: Player,
    pub action_generator: Option<usize>,
}

pub struct GameBuilder {
    players: Vec<GameBuilderPlayer>,
    button_index: u8,
    small_blind: u32,
    big_blind: u32,
    hand_selector: CardSelector,
    hand_selector_for: Option<usize>,
    player_action_generators: Vec<&'static str>,
    default_player_action_generator: Option<usize>,
    pick_community_cards: bool,
    error: String,
    remove_hand: Option<usize>,
    remove_player: Option<usize>,
    swap_players: Option<(usize, usize)>,
}

impl GameBuilder {
    const ROW_HEIGHT: f32 = 20.0;

    pub fn new(
        player_action_generators: Vec<&'static str>,
        default_player_action_generator: Option<usize>,
    ) -> Self {
        let mut hand_selector = CardSelector::new();
        hand_selector.set_min_max(2, 2);
        let default_game_data = GameData::default();
        let players = default_game_data
            .players
            .into_iter()
            .map(|player| GameBuilderPlayer {
                player,
                action_generator: default_player_action_generator,
            })
            .collect();

        Self {
            players,
            button_index: default_game_data.button_index,
            small_blind: default_game_data.small_blind,
            big_blind: default_game_data.big_blind,
            hand_selector,
            hand_selector_for: None,
            player_action_generators,
            default_player_action_generator,
            pick_community_cards: false,
            error: String::new(),
            remove_hand: None,
            remove_player: None,
            swap_players: None,
        }
    }

    pub fn window(&mut self, ctx: &Context, title: String) -> Option<GameBuilderConfig> {
        let response = Window::new(title)
            .resizable([true, true])
            .default_size([450.0, 600.0])
            .show(ctx, |ui| self.view(ui));
        response.and_then(|inner| inner.inner).flatten()
    }

    pub fn view(&mut self, ui: &mut Ui) -> Option<GameBuilderConfig> {
        self.view_reset();
        self.top_controls(ui);
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

    fn top_controls(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Small blind:");
            ui.add(DragValue::new(&mut self.small_blind));
            ui.separator();

            ui.label("Big blind:");
            ui.add(DragValue::new(&mut self.big_blind));
            ui.separator();

            if ui.button("100BB Stacks").clicked() {
                let Some(stack) = self.big_blind.checked_mul(100) else {
                    return;
                };
                for player in self.players.iter_mut() {
                    player.player.starting_stack = stack;
                }
            }
        });

        ui.horizontal(|ui| ui.checkbox(&mut self.pick_community_cards, "Pick community cards"));
    }

    fn players_table(&mut self, ui: &mut Ui) {
        let table = TableBuilder::new(ui)
            .striped(true)
            .cell_layout(Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder())
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
                    ui.strong("AI");
                });
                header.col(|ui| {
                    ui.strong("Button");
                });
            })
            .body(|body| self.players_table_body(body));
    }

    fn players_table_body(&mut self, mut body: TableBody<'_>) {
        for player_index in 0..self.players.len() {
            body.row(Self::ROW_HEIGHT, |row| {
                self.players_table_row(row, player_index)
            });
        }

        self.players_table_add_remove();
    }

    fn players_table_row(&mut self, mut row: TableRow<'_, '_>, player_index: usize) {
        let player_count = self.players.len();

        row.col(|ui| self.players_table_name_column(ui, player_index));

        row.col(|ui| {
            let player = &mut self.players[player_index].player;
            let drag_value = DragValue::new(&mut player.starting_stack).speed(1.0);
            ui.add(drag_value);
        });

        row.col(|ui| self.players_table_hand_column(ui, player_index));

        row.col(|ui| self.players_table_ai_column(ui, player_index));

        row.col(|ui| {
            ui.radio_value(
                &mut self.button_index,
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
        let player_count = self.players.len();
        let player = &mut self.players[player_index].player;

        let position_name =
            Game::position_name(player_count, usize::from(self.button_index), player_index)
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

    fn players_table_ai_column(&mut self, ui: &mut Ui, player_index: usize) {
        let player = &mut self.players[player_index];
        const NONE: &str = "None";
        let action_generator_name = player
            .action_generator
            .map(|index| self.player_action_generators[index])
            .unwrap_or(NONE);

        ComboBox::from_id_salt(format!("AI Player Combobox - {player_index}"))
            .selected_text(action_generator_name)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut player.action_generator, None, NONE);
                for (ai_index, ai_name) in self.player_action_generators.iter().copied().enumerate()
                {
                    ui.selectable_value(&mut player.action_generator, Some(ai_index), ai_name);
                }
            });
    }

    fn players_table_hand_column(&mut self, ui: &mut Ui, player_index: usize) {
        let known_cards = self
            .players
            .iter()
            .filter_map(|player| player.player.hand)
            .flat_map(|hand| hand.to_cards().iter())
            .fold(Cards::EMPTY, |cards, card| cards.with(card));
        let player = &mut self.players[player_index].player;

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
            self.players[player].player.hand = None;
        }

        if let Some(remove_player) = self.remove_player {
            if self.players.len() > Game::MIN_PLAYERS {
                self.players.remove(remove_player);

                if remove_player == usize::from(self.button_index) {
                    self.button_index = 0;
                }
            }
        }

        if let Some((a, b)) = self.swap_players {
            self.players.swap(a, b);

            let a = u8::try_from(a).unwrap();
            let b = u8::try_from(b).unwrap();
            if self.button_index == a {
                self.button_index = b;
            } else if self.button_index == b {
                self.button_index = a;
            }
        }
    }

    fn hand_selector(&mut self, ui: &mut Ui) {
        if let Some(player) = self.hand_selector_for {
            if self
                .hand_selector
                .window(ui.ctx(), "Select hand".to_string())
            {
                let hand = self.hand_selector.cards().to_hand().unwrap();
                self.players[player].player.hand = Some(hand);
                self.hand_selector.reset();
                self.hand_selector_for = None;
            }
        }
    }

    fn bottom_controls(&mut self, ui: &mut Ui) -> Option<GameBuilderConfig> {
        let game = ui
            .horizontal(|ui| {
                let game = if ui.button("Configure").clicked() {
                    self.error = String::new();

                    let players = self
                        .players
                        .iter()
                        .map(|player| player.player.clone())
                        .collect();
                    let game_data = GameData {
                        table_name: None,
                        hand_name: None,
                        date: None,
                        players,
                        button_index: self.button_index,
                        small_blind: self.small_blind,
                        big_blind: self.big_blind,
                        actions: Vec::new(),
                        showdown_stacks: None,
                    };

                    match Game::from_game_data(&game_data) {
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
                        && self.players.len() < Game::MAX_PLAYERS
                    {
                        let starting_stack = self.big_blind.saturating_mul(100);
                        self.players.push(GameBuilderPlayer {
                            player: Player::with_starting_stack(starting_stack),
                            action_generator: self.default_player_action_generator,
                        });
                    };
                });

                game
            })
            .inner;

        if !self.error.is_empty() {
            ui.label(&self.error);
        }
        game.map(|game| GameBuilderConfig {
            game,
            players: self.players.clone(),
            pick_community_cards: self.pick_community_cards,
        })
    }
}
