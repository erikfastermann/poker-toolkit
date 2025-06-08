use std::cmp;

use eframe::egui::{
    Align2, Color32, Context, FontFamily, FontId, Painter, Pos2, Rect, Sense, Ui, Vec2, Window,
};

use poker_core::{
    range::{RangeActionKind, RangeConfigEntry, RangeEntry},
    rank::Rank,
};

pub struct RangeViewer {
    range: RangeConfigEntry,
}

impl RangeViewer {
    pub fn new() -> Self {
        Self {
            range: RangeConfigEntry::default(),
        }
    }

    pub fn range(&self) -> &RangeConfigEntry {
        &self.range
    }

    pub fn set_range(&mut self, range: RangeConfigEntry) {
        self.range = range;
    }

    pub fn window(&mut self, ctx: &Context, title: String) {
        Window::new(title)
            .resizable([false, false])
            .default_size([500.0, 500.0])
            .show(ctx, |ui| self.view(ui));
    }

    pub fn view(&mut self, ui: &mut Ui) {
        // TODO: This inside a Window has some weird resize behavior.

        let bounding_rect = ui.available_rect_before_wrap();

        let range_rect_size = if bounding_rect.height() > bounding_rect.width() {
            bounding_rect.width()
        } else {
            bounding_rect.height()
        };
        let range_rect =
            Rect::from_center_size(bounding_rect.center(), Vec2::splat(range_rect_size));

        self.draw_range(ui, range_rect);
        ui.allocate_rect(range_rect, Sense::empty());
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

        let height_percent = self.range.total_entry_frequency(entry);
        let top_y = field_rect.bottom() - height_percent as f32 * field_rect.height();
        let mut left_x = field_rect.left();

        for (action, color) in action_kinds.iter().copied() {
            let frequency = self.range.entry_frequency(action, entry);

            let right_x = left_x + frequency as f32 * field_rect.width();
            let frequency_rect = Rect::from_two_pos(
                Pos2::new(left_x, top_y),
                Pos2::new(right_x, field_rect.bottom()),
            );

            painter.rect_filled(frequency_rect, 0.0, color);

            left_x = right_x;
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
        let mut actions: Vec<_> = self
            .range
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
}
