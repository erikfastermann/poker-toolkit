use eframe::egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Rgba, Shape};
use poker_core::{card::Card, suite::Suite};

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
