use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Deserializer};
use toml::value::Time;
use toml_edit::{Document, Item, Value};

use crate::{
    cards::Cards,
    game::{Game, Player, State, Street},
    result::Result,
};

pub fn parse_phhs_str(phhs: &str, skip_non_zero_ante: bool) -> Result<Vec<Result<Game>>> {
    // TODO: As Iterator.

    // Have to use toml_edit, because the normal toml parser
    // always uses f64 for floats, which looses precision.

    let doc = phhs.parse::<Document<String>>()?;

    if !doc.as_item().is_table() {
        return Err("root of phhs must be a table".into());
    }

    let out: Vec<_> = doc
        .as_table()
        .iter()
        .filter_map(|(_, item)| item_to_game(&doc, item, skip_non_zero_ante))
        .collect();

    Ok(out)
}

#[derive(Debug, Deserialize)]
struct Entry {
    variant: String,
    actions: Vec<String>,

    venue: Option<String>,
    /// Either in time zone, location or UTC.
    time: Option<Time>,
    day: Option<u8>,
    month: Option<u8>,
    year: Option<i32>,
    #[serde(deserialize_with = "string_or_int")]
    hand: Option<String>,
    seats: Option<Vec<u8>>,
    seat_count: Option<u8>,
    #[serde(deserialize_with = "string_or_int")]
    table: Option<String>,
    players: Option<Vec<String>>,
    currency_symbol: Option<String>,
}

fn item_to_game(
    doc: &Document<String>,
    item: &Item,
    skip_non_zero_ante: bool,
) -> Option<Result<Game>> {
    let antes = item.get("antes").and_then(|antes| antes.as_array());

    if let Some(antes) = antes {
        let all_antes_zero = antes
            .iter()
            .filter_map(|value| value.as_float())
            .all(|n| n == 0.0);

        if skip_non_zero_ante && !all_antes_zero {
            return None;
        }
    }

    Some(item_to_game_inner(doc, item))
}

fn item_to_game_inner(doc: &Document<String>, item: &Item) -> Result<Game> {
    let entry: Entry = toml::from_str(&item.to_string())?;

    if entry.variant != "NT" {
        return Err(format!("only no-limit hold'em supported, not '{}'", entry.variant).into());
    }

    let Some(antes) = parse_chips_array(doc, item.get("antes"))? else {
        return Err("missing antes field".into());
    };

    let non_zero_ante = antes.iter().any(|ante| *ante != 0);
    if non_zero_ante {
        return Err("non zero ante not supported".into());
    }

    // We output with currency ct and adjust the sizings accordingly.
    if !entry
        .currency_symbol
        .as_ref()
        .is_some_and(|symbol| symbol == "$")
    {
        return Err("expected currency symbol dollar".into());
    }

    let Some(blinds_or_straddles) = parse_chips_array(doc, item.get("blinds_or_straddles"))? else {
        return Err("missing blinds_or_straddles field".into());
    };

    let player_count = blinds_or_straddles.len();
    if player_count < Game::MIN_PLAYERS || player_count > Game::MAX_PLAYERS {
        return Err("bad player count".into());
    }

    let button_index = if player_count == 2 {
        0
    } else {
        player_count - 1
    };

    // TODO: What if someone straddles from the blinds?
    let small_blind = blinds_or_straddles[0];
    let big_blind = blinds_or_straddles[1];

    let max_players = entry
        .seat_count
        .map(|n| usize::from(n))
        .unwrap_or(player_count);

    let Some(min_bet_raw) = item.get("min_bet").and_then(|item| item.as_value()) else {
        return Err("missing required min_bet value".into());
    };

    if parse_chips(doc, min_bet_raw)? != big_blind {
        return Err("min bet is not equal to the big blind".into());
    }

    // Can be inf, which we don't parse in this case.
    let Some(starting_stacks) = parse_chips_array(doc, item.get("starting_stacks"))? else {
        return Err("missing required starting_stacks array".into());
    };

    let mut players = Vec::new();

    for player_index in 0..player_count {
        let name = entry
            .players
            .as_ref()
            .and_then(|players| players.get(player_index))
            .map(|name| Arc::new(name.clone()));

        let seat = entry
            .seats
            .as_ref()
            .and_then(|seats| seats.get(player_index).copied())
            .and_then(|seat| seat.checked_sub(1));

        if seat.is_some_and(|seat| usize::from(seat) >= max_players) {
            return Err("invalid seat config: bigger than seat or player count".into());
        }

        let Some(starting_stack) = starting_stacks.get(player_index).copied() else {
            return Err("starting_stacks has invalid length".into());
        };

        let player = Player {
            name,
            seat,
            hand: None,
            starting_stack,
        };

        players.push(player);
    }

    let mut game = Game::new(&players, button_index, small_blind, big_blind)?;

    game.set_unit(Arc::new(entry.currency_symbol.unwrap()));

    game.set_max_players(max_players)?;

    if let Some(venue) = entry.venue {
        game.set_location(Arc::new(venue));
    }

    // Ignoring time zone information, game currently only stores `NaiveDateTime`.
    match (entry.year, entry.month, entry.day, entry.time) {
        (Some(year), Some(month), Some(day), Some(time)) => {
            let date = NaiveDate::from_ymd_opt(year, month.into(), day.into());
            let time =
                NaiveTime::from_hms_opt(time.hour.into(), time.minute.into(), time.second.into());

            let (Some(date), Some(time)) = (date, time) else {
                return Err("invalid year, month, day or time value".into());
            };

            let datetime = NaiveDateTime::new(date, time);

            game.set_date(datetime);
        }
        (None, None, None, None) => (),
        _ => return Err("year, month, day and time all have to be set or none of them".into()),
    }

    if let Some(table) = entry.table {
        game.set_table_name(Arc::new(table));
    }

    if let Some(hand) = entry.hand {
        game.set_hand_name(Arc::new(hand));
    }

    game.post_small_and_big_blind()?;

    // The PHH format does not differentiate between posts and straddles.

    for (player, post) in blinds_or_straddles.iter().copied().enumerate().skip(2) {
        if post <= big_blind {
            game.additional_post(player, post, false)?;
        }
    }

    for (player, straddle) in blinds_or_straddles.iter().copied().enumerate().skip(2) {
        if straddle > big_blind {
            game.straddle(player, straddle)?;
        }
    }

    for action in entry.actions {
        let comment_start_index = action.find('#');

        let action = &action[..comment_start_index.unwrap_or_else(|| action.len())];
        let action = action.trim();

        if action.is_empty() {
            continue;
        }

        let mut split = action.split(' ');

        let Some(actor) = split.next() else {
            return Err("missing actor in action string".into());
        };

        let Some(action_kind) = split.next() else {
            return Err("missing action kind in action string".into());
        };

        match (actor, action_kind) {
            ("d", "db") => {
                let Some(community_cards) = split.next() else {
                    return Err("missing community cards in deal".into());
                };

                let community_cards = Cards::from_str(community_cards)?;

                // TODO:
                // Does PHH support multiple board runouts?
                // How are they represented?
                let State::Street(street) = game.state() else {
                    return Err("game state does expect a new street".into());
                };

                if usize::from(community_cards.count()) != street.new_community_card_count() {
                    return Err("unexpected number of community cards for street".into());
                };

                let cards: Vec<_> = community_cards.iter().collect();
                match street {
                    Street::PreFlop => unreachable!(),
                    Street::Flop => game.flop([cards[0], cards[1], cards[2]])?,
                    Street::Turn => game.turn(cards[0])?,
                    Street::River => game.river(cards[0])?,
                }
            }
            ("d", "dh") => {
                let Some(player) = split.next() else {
                    return Err("missing player in hole card deal".into());
                };

                let player = parse_action_player(player)?;

                let Some(hand) = split.next() else {
                    return Err("missing hand in hole card deal".into());
                };

                if hand != "????" {
                    let Some(hand) = Cards::from_str(hand)?.to_hand() else {
                        return Err("could not convert dealt hole cards to player hand".into());
                    };

                    game.set_hand(usize::from(player), hand)?;
                }
            }
            (_, "cbr") => {
                let player = parse_action_player(actor)?;

                if game.current_player() != Some(usize::from(player)) {
                    return Err("unexpected player in bet/raise".into());
                }

                let Some(amount) = split.next() else {
                    return Err("missing amount in bet/raise".into());
                };

                let amount = parse_chips_str(amount)?;

                if game.can_bet().is_some() {
                    game.bet(amount)?;
                } else if game.can_raise().is_some() {
                    game.raise(amount)?;
                } else {
                    return Err("bet/raise not possible".into());
                }
            }
            (_, "cc") => {
                let player = parse_action_player(actor)?;

                if game.current_player() != Some(usize::from(player)) {
                    return Err("unexpected player in check/call".into());
                }

                if game.can_check() {
                    game.check()?;
                } else if game.can_call().is_some() {
                    game.call()?;
                } else {
                    return Err("check/call not possible".into());
                }
            }
            (_, "f") => {
                let player = parse_action_player(actor)?;

                if game.current_player() != Some(usize::from(player)) {
                    return Err("unexpected player in fold".into());
                }

                game.fold()?;
            }
            (_, "sm") => {
                let player = parse_action_player(actor)?;

                if game.state() != State::ShowOrMuck(usize::from(player)) {
                    return Err("unexpected player in show or muck".into());
                }

                if let Some(hand) = split.next() {
                    if hand != "-" && hand != "????" {
                        let Some(hand) = Cards::from_str(hand)?.to_hand() else {
                            return Err("could not convert shown hand to player hand".into());
                        };

                        game.set_hand(usize::from(player), hand)?;
                    }

                    game.show_hand()?;
                } else {
                    game.muck_hand()?;
                }
            }
            _ => return Err(format!("unsupported action kind {action_kind}").into()),
        }

        if split.next().is_some() {
            return Err("invalid action format: unexpected data at end".into());
        }
    }

    // Workaround, because winnings are not always provided.
    // TODO: Could also check finishing_stacks.

    if let Some(winnings) = parse_chips_array(doc, item.get("winnings"))? {
        let total_winnings = winnings
            .iter()
            .copied()
            .fold(Some(0u32), |acc, n| acc.and_then(|acc| acc.checked_add(n)));

        let Some(total_winnings) = total_winnings else {
            return Err("winnings sum overflowed an u32".into());
        };

        let Some(total_rake) = game.total_pot().checked_sub(total_winnings) else {
            return Err("total winnings is greater than total pot".into());
        };

        let player_pot_share = winnings
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, winning)| *winning != 0);

        game.showdown_custom(total_rake, player_pot_share)?;
    } else {
        game.showdown_simple()?;
    }

    Ok(game)
}

fn parse_action_player(player: &str) -> Result<u8> {
    if player.len() <= 2 {
        return Err("invalid action player format: not long enough".into());
    }

    if player.as_bytes()[0].to_ascii_lowercase() != b'p' {
        return Err("invalid action player format: does not start with `P` or `p`".into());
    }

    let index: u8 = player[1..].parse()?;

    let Some(index) = index.checked_sub(1) else {
        return Err("invalid action player format: expected one-based index".into());
    };

    Ok(index)
}

fn parse_chips_array(doc: &Document<String>, item: Option<&Item>) -> Result<Option<Vec<u32>>> {
    let Some(item) = item else {
        return Ok(None);
    };

    if item.is_none() {
        return Ok(None);
    }

    let Some(array) = item.as_array() else {
        return Err("chips is not an array".into());
    };

    let mut out = Vec::new();

    for value in array {
        out.push(parse_chips(doc, value)?);
    }

    Ok(Some(out))
}

fn parse_chips(doc: &Document<String>, value: &Value) -> Result<u32> {
    if !value.is_float() {
        return Err("chips value must be a float".into());
    }

    let Some(span) = value.span() else {
        return Err("unknown error while parsing chips".into());
    };

    let Some(chips_raw) = doc.raw().get(span) else {
        return Err("unknown error while parsing chips".into());
    };

    parse_chips_str(chips_raw)
}

fn parse_chips_str(chips: &str) -> Result<u32> {
    let mut split = chips.split('.');

    let int: u32 = split.next().unwrap().parse()?;

    let frac = match split.next() {
        Some(s) => {
            let frac: u32 = s.parse()?;
            if s.len() == 1 {
                frac * 10
            } else if s.len() == 2 {
                frac
            } else {
                return Err(format!("chips {chips}: invalid format").into());
            }
        }
        None => 0,
    };

    if split.next().is_some() {
        return Err(format!("chips {chips}: invalid format").into());
    }

    int.checked_mul(100)
        .and_then(|n| n.checked_add(frac))
        .ok_or_else(|| format!("chips {chips} too large").into())
}

fn string_or_int<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct StringOrIntVisitor;

    impl<'de> Visitor<'de> for StringOrIntVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or integer")
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value.to_string()))
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(StringOrIntVisitor)
        }
    }

    deserializer.deserialize_option(StringOrIntVisitor)
}
