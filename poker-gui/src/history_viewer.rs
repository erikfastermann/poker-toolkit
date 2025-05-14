use eframe::egui::{Align, Layout, ScrollArea, Sense, TextStyle, Ui};
use egui_extras::{Column, TableBody, TableBuilder, TableRow};
use poker_core::{db, game::Game};

pub struct HistoryViewer {
    entries: Vec<(db::Hand, Option<db::HandPlayer>)>,
}

impl HistoryViewer {
    pub fn new(entries: Vec<(db::Hand, Option<db::HandPlayer>)>) -> Self {
        Self { entries }
    }

    pub fn view(&mut self, ui: &mut Ui) {
        ScrollArea::horizontal().show(ui, |ui| self.table(ui)).inner
    }

    fn table(&mut self, ui: &mut Ui) {
        let available_height = ui.available_height();
        let text_height = Self::text_height(ui);

        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .min_scrolled_height(0.0)
            .max_scroll_height(available_height)
            .sense(Sense::click())
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
        body.rows(text_height, self.entries.len(), |row| {
            self.table_row(row.index(), row);
        });
    }

    fn table_row(&mut self, index: usize, mut row: TableRow<'_, '_>) {
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
                .and_then(|hand_player| hand_player.hand)
                .map(|hand| hand.to_string())
                .unwrap_or_else(|| "-".to_string());

            ui.label(hand);
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.pre_flop_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            let flop = hand
                .first_flop
                .map(|flop| flop.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(flop);
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.flop_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            let turn = hand
                .first_turn
                .map(|turn| turn.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(turn);
        });

        row.col(|ui| {
            let actions = hand_player
                .as_ref()
                .and_then(|hand_player| hand_player.turn_action.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(actions);
        });

        row.col(|ui| {
            let river = hand
                .first_river
                .map(|river| river.to_string())
                .unwrap_or_else(|| "-".to_owned());

            ui.label(river);
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
    }

    fn text_height(ui: &Ui) -> f32 {
        TextStyle::Body
            .resolve(ui.style())
            .size
            .max(ui.spacing().interact_size.y)
    }
}
