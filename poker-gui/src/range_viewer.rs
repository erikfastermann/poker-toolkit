use std::{cmp, mem};

use eframe::egui::{
    Align2, Button, Color32, Context, FontFamily, FontId, Id, Label, Painter, Pos2, Rect,
    ScrollArea, Sense, Ui, Vec2, Window,
};

use poker_core::{
    range::{range_entry_frequency, RangeActionKind, RangeConfigEntry, RangeEntry, RangeTableWith},
    rank::Rank,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeValue {
    Simple(RangeTableWith<u16>),
    Full(RangeConfigEntry),
}

#[derive(Debug, Clone)]
pub struct RangeViewer {
    ranges: Vec<RangeValue>,
    selected: usize,
    details: String,
}

impl RangeViewer {
    pub fn new() -> Self {
        Self {
            ranges: vec![RangeValue::Full(RangeConfigEntry::default())],
            selected: 0,
            details: String::new(),
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn ranges(&self) -> &[RangeValue] {
        &self.ranges
    }

    pub fn ranges_mut(&mut self) -> &mut [RangeValue] {
        &mut self.ranges
    }

    pub fn push_range(&mut self, range: RangeValue) {
        self.ranges.push(range);
    }

    pub fn remove_range(&mut self, index: usize) -> Option<RangeValue> {
        if self.ranges.len() == 1 || index >= self.ranges.len() {
            None
        } else {
            let old = self.ranges.remove(index);
            if self.selected >= self.ranges.len() {
                self.selected = self.ranges.len() - 1;
            }
            Some(old)
        }
    }

    pub fn replace_ranges(&mut self, mut ranges: Vec<RangeValue>) -> Option<Vec<RangeValue>> {
        if ranges.len() < 1 {
            None
        } else {
            if ranges != self.ranges {
                mem::swap(&mut self.ranges, &mut ranges);
                self.selected = 0;
                Some(ranges)
            } else {
                None
            }
        }
    }

    pub fn details(&mut self) -> &str {
        &self.details
    }

    pub fn set_details(&mut self, details: String) {
        self.details = details;
    }

    pub fn window(&mut self, ctx: &Context, id: Id, title: String) {
        Window::new(title)
            .id(id)
            .resizable([false, false])
            .default_size([400.0, 400.0]) // TODO: Resize.
            .show(ctx, |ui| self.view(ui));
    }

    pub fn view(&mut self, ui: &mut Ui) {
        // TODO: This inside a Window has some weird resize behavior.

        let bounding_rect = ui.available_rect_before_wrap();

        let range_rect =
            Rect::from_min_size(bounding_rect.left_top(), Vec2::splat(bounding_rect.width()));

        self.draw_range(ui, range_rect);
        ui.allocate_rect(range_rect, Sense::empty());

        self.navigation_bar(ui);

        self.details_text(ui);
    }

    fn draw_range(&self, ui: &mut Ui, bounding_rect: Rect) {
        assert!((bounding_rect.width() - bounding_rect.height()).abs() <= 0.1);

        let field_size = bounding_rect.width() / Rank::COUNT as f32;
        let field_size = Vec2::splat(field_size);

        let action_kinds = self.action_kinds_colors();

        for (row_index, row) in Rank::RANKS.into_iter().rev().enumerate() {
            for (column_index, column) in Rank::RANKS.into_iter().rev().enumerate() {
                let left_corner = Pos2 {
                    x: bounding_rect.min.x + field_size.x * column_index as f32,
                    y: bounding_rect.min.y + field_size.y * row_index as f32,
                };
                let field_rect = Rect::from_min_size(left_corner, field_size);

                let entry = RangeEntry::from_row_column(row, column);
                self.draw_entry(&ui.painter_at(field_rect), entry, &action_kinds);
            }
        }
    }

    fn draw_entry(
        &self,
        painter: &Painter,
        entry: RangeEntry,
        action_kinds: &[(RangeActionKind, Color32)],
    ) {
        let field_rect = painter.clip_rect();

        let range = &self.ranges[self.selected];

        let height_percent = match range {
            RangeValue::Simple(range) => range_entry_frequency(range, entry),
            RangeValue::Full(range) => range.total_entry_frequency(entry),
        };

        let top = field_rect.bottom() - height_percent as f32 * field_rect.height();

        match range {
            RangeValue::Simple(_) => {
                let frequency_rect = Rect::from_two_pos(
                    Pos2::new(field_rect.left(), top),
                    Pos2::new(field_rect.right(), field_rect.bottom()),
                );

                painter.rect_filled(frequency_rect, 0.0, Color32::WHITE);
            }
            RangeValue::Full(range) => {
                let mut left = field_rect.left();

                for (action, color) in action_kinds.iter().copied() {
                    let frequency = range.entry_frequency(action, entry);

                    let right = left + frequency as f32 * field_rect.width();
                    let frequency_rect = Rect::from_two_pos(
                        Pos2::new(left, top),
                        Pos2::new(right, field_rect.bottom()),
                    );

                    painter.rect_filled(frequency_rect, 0.0, color);

                    left = right;
                }
            }
        }

        painter.rect_filled(field_rect, 0, Color32::from_black_alpha(80));
        painter.text(
            field_rect.center(),
            Align2::CENTER_CENTER,
            entry.to_regular_string(),
            FontId::new(field_rect.width() / 3.0, FontFamily::Monospace),
            Color32::WHITE,
        );
    }

    fn action_kinds_colors(&self) -> Vec<(RangeActionKind, Color32)> {
        let RangeValue::Full(range) = &self.ranges[self.selected] else {
            return Vec::new();
        };

        let mut actions: Vec<_> = range
            .action_kinds()
            .map(|action| (action, Color32::BLACK))
            .collect();

        actions.sort_by_key(|(action, _)| *action);
        actions.reverse();

        let bet_raise_count = actions
            .iter()
            .filter(|(action, _)| {
                matches!(action, RangeActionKind::Bet(_) | RangeActionKind::Raise(_))
            })
            .count();

        let bet_raise_red_steps = cmp::max(8, u8::try_from(128 / (bet_raise_count + 1)).unwrap());
        let mut bet_raise_red_value = 128u8.saturating_add(bet_raise_red_steps);

        for (action, color) in &mut actions {
            *color = match action {
                RangeActionKind::Post { .. } | RangeActionKind::Straddle { .. } => Color32::YELLOW,
                RangeActionKind::Fold => Color32::BLUE,
                RangeActionKind::Check | RangeActionKind::Call => Color32::GREEN,
                RangeActionKind::Bet(_) | RangeActionKind::Raise(_) => {
                    bet_raise_red_value = bet_raise_red_value.saturating_add(bet_raise_red_steps);
                    Color32::from_rgb(bet_raise_red_value, 0, 0)
                }
            };
        }

        actions
    }

    fn navigation_bar(&mut self, ui: &mut Ui) {
        let old_selected = self.selected;

        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.selected > 0, Button::new("<"))
                .clicked()
            {
                self.selected -= 1;
            }

            if ui
                .add_enabled(self.selected < self.ranges.len() - 1, Button::new(">"))
                .clicked()
            {
                self.selected += 1;
            }
        });

        if self.selected != old_selected {
            ui.ctx().request_repaint();
        }
    }

    fn details_text(&self, ui: &mut Ui) {
        if self.details.is_empty() {
            return;
        }

        ui.separator();

        ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            ui.add_sized(ui.available_size(), Label::new(&self.details));
        });
    }
}
