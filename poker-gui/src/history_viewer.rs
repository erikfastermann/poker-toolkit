use eframe::egui::{
    Align, CentralPanel, Context, Layout, ScrollArea, Sense, TextStyle, TopBottomPanel, Ui,
    UiBuilder,
};
use egui_extras::{Column, TableBody, TableBuilder, TableRow};
use poker_core::{db, game::Game, result::Result};

use crate::{card::draw_cards, game_view::GameView};

pub struct HistoryView {
    entries: Vec<(db::Hand, Option<db::HandPlayer>)>,
    current_entry: Option<usize>,
    scroll_to_current_entry: bool,
    game_view: Option<GameView>,
    game_getter: Box<dyn FnMut(u64) -> Result<Game>>,
}

impl HistoryView {
    pub fn new(
        entries: Vec<(db::Hand, Option<db::HandPlayer>)>,
        game_getter: Box<dyn FnMut(u64) -> Result<Game>>,
    ) -> Self {
        Self {
            entries,
            scroll_to_current_entry: false,
            current_entry: None,
            game_view: None,
            game_getter,
        }
    }

    pub fn view(&mut self, ctx: &Context) {
        let old_entry = self.current_entry;
        let has_game = self.game_view.is_some();

        if has_game {
            CentralPanel::default().show(ctx, |ui| self.game(ui));
        }

        let table_panel_height = if has_game {
            ctx.screen_rect().height() * 0.3
        } else {
            ctx.screen_rect().height()
        };

        TopBottomPanel::bottom("table_panel")
            .resizable(false)
            .exact_height(table_panel_height)
            .show(ctx, |ui| {
                ScrollArea::horizontal().show(ui, |ui| self.table(ui));
            });

        if old_entry != self.current_entry {
            self.update_game_view();
            ctx.request_repaint();
        }
    }

    fn update_game_view(&mut self) {
        if let Some(current_entry) = self.current_entry {
            if self.game_view.is_none() {
                self.scroll_to_current_entry = true;
            }

            // TODO:
            // - Handle errors
            // - Run in background
            let hand_id = self.entries[current_entry].0.id.unwrap();
            let mut game = (self.game_getter)(hand_id).unwrap();
            game.rewind();

            let mut game_view = GameView::new();
            game_view.set_enable_game_builder(false);
            game_view
                .with_game_mut(|current_game| *current_game = game)
                .unwrap();
            self.game_view = Some(game_view);
        } else {
            self.game_view = None;
        }
    }

    fn game(&mut self, ui: &mut Ui) {
        let mut max_rect = ui.max_rect().shrink(ui.max_rect().height() * 0.02);
        max_rect.set_height(max_rect.height() * 0.7);
        max_rect.set_width(max_rect.height() * 4.0 / 3.0);

        ui.allocate_new_ui(UiBuilder::new().max_rect(max_rect), |ui| {
            self.game_view.as_mut().unwrap().view(ui).unwrap();
        });
    }

    fn table(&mut self, ui: &mut Ui) {
        let available_height = ui.available_height();
        let text_height = Self::text_height(ui);

        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .column(Column::remainder())
            .min_scrolled_height(0.0)
            .max_scroll_height(available_height)
            .sense(Sense::click());

        if self.scroll_to_current_entry {
            builder = builder.scroll_to_row(self.current_entry.unwrap(), Some(Align::TOP));
            self.scroll_to_current_entry = false;
        }

        builder
            .header(text_height, |header| self.table_header(header))
            .body(|body| self.table_body(body));
    }

    fn table_header(&self, mut header: TableRow<'_, '_>) {
        header.col(|ui| {
            ui.strong("Loc");
        });
        header.col(|ui| {
            ui.strong("Kind");
        });
        header.col(|ui| {
            ui.strong("Stake");
        });
        header.col(|ui| {
            ui.strong("Date");
        });
        header.col(|ui| {
            ui.strong("ID");
        });
        header.col(|ui| {
            ui.strong("Pos");
        });
        header.col(|ui| {
            ui.strong("Hand");
        });
        header.col(|ui| {
            ui.strong("P-Act");
        });
        header.col(|ui| {
            ui.strong("Flop");
        });
        header.col(|ui| {
            ui.strong("F-Act");
        });
        header.col(|ui| {
            ui.strong("Turn");
        });
        header.col(|ui| {
            ui.strong("T-Act");
        });
        header.col(|ui| {
            ui.strong("River");
        });
        header.col(|ui| {
            ui.strong("R-Act");
        });
        header.col(|ui| {
            ui.strong("Pot");
        });
        header.col(|ui| {
            ui.strong("W/L");
        });
    }

    fn table_body(&mut self, mut body: TableBody<'_>) {
        let text_height = Self::text_height(body.ui_mut());
        body.rows(1.5 * text_height, self.entries.len(), |row| {
            self.table_row(row.index(), row);
        });
    }

    fn table_row(&mut self, index: usize, mut row: TableRow<'_, '_>) {
        let selected = self
            .current_entry
            .is_some_and(|current_entry| current_entry == index);
        row.set_selected(selected);

        let (hand, hand_player) = &self.entries[index];

        row.col(|ui| {
            let location = hand
                .game_location
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("-");

            ui.label(location);
        });

        row.col(|ui| {
            let max_players = hand
                .max_players
                .map(|n| format!("{n}-max"))
                .unwrap_or_else(|| "-".to_owned());

            ui.label(max_players);
        });

        row.col(|ui| {
            let mut stake = format!("{}/{}", hand.small_blind, hand.big_blind);
            if let Some(unit) = hand.unit.as_ref() {
                stake += unit;
            }

            ui.label(stake);
        });

        row.col(|ui| {
            let date = hand
                .game_date
                .map(|date| date.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(date);
        });

        row.col(|ui| {
            let id = hand.hand_name.as_ref().map(|s| s.as_str()).unwrap_or("-");

            ui.label(id);
        });

        row.col(|ui| {
            let position = hand_player
                .as_ref()
                .and_then(|hand_player| {
                    Game::position_name(
                        usize::from(hand.player_count),
                        usize::from(hand.button_index),
                        usize::from(hand_player.player),
                    )
                    .map(|(short_name, _)| short_name)
                })
                .unwrap_or("-");

            ui.label(position);
        });

        row.col(|ui| {
            let hand = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.hand);

            if let Some(hand) = hand {
                draw_cards(ui.painter(), ui.max_rect(), &hand.to_card_array());
            }
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.pre_flop_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            if let Some(flop) = hand.first_flop {
                draw_cards(ui.painter(), ui.max_rect(), &flop.0);
            }
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.flop_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            if let Some(turn) = hand.first_turn {
                draw_cards(ui.painter(), ui.max_rect(), &[turn]);
            }
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.turn_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            if let Some(river) = hand.first_river {
                draw_cards(ui.painter(), ui.max_rect(), &[river]);
            }
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.river_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            let mut pot = format!("{}", hand.final_full_pot_size);
            if let Some(unit) = hand.unit.as_ref() {
                pot += unit;
            }

            ui.label(pot);
        });

        row.col(|ui| {
            let unit = hand.unit.as_ref().map(|unit| unit.as_str()).unwrap_or("");

            let win_loss = hand_player
                .as_ref()
                .map(|hand_player| {
                    i64::from(hand_player.showdown_stack) - i64::from(hand_player.starting_stack)
                })
                .map(|win_loss| format!("{win_loss}{unit}"))
                .unwrap_or_else(|| "-".to_owned());

            ui.label(win_loss);
        });

        if row.response().clicked() {
            if selected {
                self.current_entry = None;
            } else {
                self.current_entry = Some(index);
            }
        }
    }

    fn text_height(ui: &Ui) -> f32 {
        TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y)
    }
}
