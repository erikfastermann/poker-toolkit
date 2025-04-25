use eframe::egui::{
    Align, Button, Context, Id, Layout, Pos2, Rect, Rgba, Sense, Stroke, StrokeKind, Ui, UiBuilder,
    Vec2, Window,
};

use poker_core::{card::Card, cards::Cards, rank::Rank, suite::Suite};

use crate::card::draw_card;

pub struct CardSelector {
    cards: Cards,
    in_order: Vec<Card>,
    disabled: Cards,
    min: usize,
    max: usize,
}

impl CardSelector {
    pub fn new() -> Self {
        Self {
            cards: Cards::EMPTY,
            in_order: Vec::new(),
            disabled: Cards::EMPTY,
            min: 1,
            max: 1,
        }
    }

    pub fn cards(&self) -> Cards {
        self.cards
    }

    pub fn cards_in_order(&self) -> &[Card] {
        &self.in_order
    }

    pub fn reset(&mut self) {
        self.cards = Cards::EMPTY;
        self.in_order.truncate(0);
    }

    pub fn set_min_max(&mut self, min: usize, max: usize) {
        assert!(max <= Card::COUNT);
        assert!(min <= max);
        assert!(max > 0);
        self.min = min;
        self.max = max;
    }

    pub fn set_disabled(&mut self, disabled: Cards) {
        for card in (self.cards & disabled).iter() {
            self.remove_card(card);
        }
        self.disabled = disabled;
        assert_eq!(self.cards & self.disabled, Cards::EMPTY);
    }

    pub fn window(&mut self, ctx: &Context, title: String) -> bool {
        let response = Window::new(title)
            .resizable([true, true])
            .default_size([400.0, 200.0])
            .show(ctx, |ui| self.view(ui));
        response.is_some_and(|apply| apply.inner == Some(true))
    }

    pub fn view(&mut self, ui: &mut Ui) -> bool {
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
