use eframe::egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Rgba, Shape, Vec2};
use poker_core::{card::Card, suite::Suite};

pub fn draw_cards(painter: &Painter, bounding_rect: Rect, cards: &[Card]) {
    if cards.is_empty() {
        return;
    }

    let total_width = bounding_rect.width();
    let mut height = bounding_rect.height();

    let mut card_width = height / 1.3;
    if total_width < card_width * cards.len() as f32 {
        card_width = total_width / cards.len() as f32;
        height = card_width * 1.3;
    }

    let start_left_x = bounding_rect.center().x - (card_width * cards.len() as f32 / 2.0);
    let top_y = bounding_rect.center().y + height / 2.0;
    let bottom_y = bounding_rect.center().y - height / 2.0;

    for (index, card) in cards.iter().copied().enumerate() {
        let left_x = start_left_x + card_width * index as f32;
        let card_rect = Rect {
            min: Pos2 {
                x: left_x,
                y: bottom_y,
            },
            max: Pos2 {
                x: left_x + card_width,
                y: top_y,
            },
        };

        let x_space = card_rect.width() * 0.05;
        let card_rect = card_rect.shrink2(Vec2::new(x_space, x_space * 1.3));

        draw_card(painter, card_rect, card);
    }
}

pub fn draw_card(painter: &Painter, bounding_rect: Rect, card: Card) {
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

pub fn draw_hidden_card(painter: &Painter, bounding_rect: Rect) {
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
