use std::{collections::HashMap, sync::Arc};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Deserializer};
use toml::value::Time;
use toml_edit::{Document, Item, Value};

use crate::{
    bitset::Bitset,
    cards::Cards,
    game::{Game, Player, State, Street},
    result::Result,
};

pub fn parse_phhs_str(
    phhs: &str,
    mut skip_unsupported: Option<&mut SkipReasons>,
) -> Result<Vec<Result<Game>>> {
    // TODO: As Iterator.

    // Somewhat specific to the handhq dataset,
    // the actual format allows more variety in some cases.

    // Have to use toml_edit, because the normal toml parser
    // always uses f64 for floats, which looses precision.

    let doc = phhs.parse::<Document<String>>()?;

    if !doc.as_item().is_table() {
        return Err("root of phhs must be a table".into());
    }

    let mut out = Vec::new();

    for (_, item) in doc.as_table().iter() {
        if let Some(result) = item_to_game(&doc, item, skip_unsupported.as_deref_mut()) {
            out.push(result);
        }
    }

    Ok(out)
}

pub type SkipReasons = HashMap<&'static str, u64>;

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
    #[serde(default)]
    #[serde(deserialize_with = "string_or_int")]
    hand: Option<String>,
    seats: Option<Vec<u8>>,
    seat_count: Option<u8>,
    #[serde(default)]
    #[serde(deserialize_with = "string_or_int")]
    table: Option<String>,
    players: Option<Vec<String>>,
    currency_symbol: Option<String>,
}

fn item_to_game(
    doc: &Document<String>,
    item: &Item,
    skip_unsupported: Option<&mut SkipReasons>,
) -> Option<Result<Game>> {
    if let Some(skip_reasons) = skip_unsupported {
        if unsupported_item(item, skip_reasons) {
            return None;
        }
    }

    let result = item_to_game_inner(doc, item).map_err(|err| {
        format!(
            "{}\nerror: {}\nThis can be caused by an internal parser error or an invalid hand history.",
            item.to_string(),
            err
        )
        .into()
    });
    Some(result)
}

fn unsupported_item(item: &Item, reasons: &mut SkipReasons) -> bool {
    let mut not_all_antes_zero = false;
    let mut not_two_blinds = false;
    let mut negative_blind_or_straddle = false;
    let mut contains_unknown_starting_stack = false;

    let antes = item.get("antes").and_then(|antes| antes.as_array());

    if let Some(antes) = antes {
        // Game currently does not support antes.
        not_all_antes_zero = !antes.iter().all(|value| is_float_or_int_zero(value));
    }

    let blinds_and_straddles = item
        .get("blinds_or_straddles")
        .and_then(|item| item.as_array());

    if let Some(blinds_and_straddles) = blinds_and_straddles {
        if let (Some(blind_1), Some(blind_2)) =
            (blinds_and_straddles.get(0), blinds_and_straddles.get(1))
        {
            // Game currently only supports small and big blind,
            // not single blind.
            not_two_blinds = is_float_or_int_zero(blind_1) || is_float_or_int_zero(blind_2);
        }

        // TODO:
        // Appears often in handhq pty histories, should we handle this?
        // What is the meaning?
        negative_blind_or_straddle = blinds_and_straddles.iter().any(|v| {
            v.as_integer().is_some_and(|n| n < 0) || v.as_float().is_some_and(|n| n < 0.0)
        });
    }

    let starting_stacks = item.get("starting_stacks").and_then(|item| item.as_array());

    if let Some(starting_stacks) = starting_stacks {
        // Game currently requires a starting stack.
        contains_unknown_starting_stack = starting_stacks
            .iter()
            .any(|n| n.as_float().is_some_and(|n| n.is_infinite()));
    }

    *reasons.entry("not_all_antes_zero").or_insert(0) += u64::from(not_all_antes_zero);
    *reasons.entry("not_two_blinds").or_insert(0) += u64::from(not_two_blinds);
    *reasons.entry("negative_blind_or_straddle").or_insert(0) +=
        u64::from(negative_blind_or_straddle);
    *reasons
        .entry("contains_unknown_starting_stack")
        .or_insert(0) += u64::from(contains_unknown_starting_stack);

    let skip = not_all_antes_zero
        || not_two_blinds
        || negative_blind_or_straddle
        || contains_unknown_starting_stack;

    *reasons.entry("total").or_insert(0) += 1;
    *reasons.entry("total_skipped").or_insert(0) += u64::from(skip);

    skip
}

fn is_float_or_int_zero(v: &Value) -> bool {
    v.as_integer().is_some_and(|n| n == 0) || v.as_float().is_some_and(|n| n == 0.0)
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

    let mut game = new_game_from_item(doc, item, &entry)?;

    game_update_metadata(&mut game, &entry)?;
    parse_player_hands(&mut game, &entry.actions)?;
    parse_actions(&mut game, &entry.actions)?;

    // TODO:
    // Winnings are not always provided and even if they are,
    // the reported winnings are sometimes not correct.
    // The data seems inconsistent, so I currently don't see a clear fix for this.
    // Just use our own simple showdown routine.
    // Could also check finishing_stacks in the future,
    // but they don't seem to be used often.
    game.showdown_simple()?;

    Ok(game)
}

fn new_game_from_item(doc: &Document<String>, item: &Item, entry: &Entry) -> Result<Game> {
    let Some(mut blinds_or_straddles) = parse_chips_array(doc, item.get("blinds_or_straddles"))?
    else {
        return Err("missing blinds_or_straddles field".into());
    };

    let player_count = blinds_or_straddles.len();
    if player_count < Game::MIN_PLAYERS || player_count > Game::MAX_PLAYERS {
        return Err("bad player count".into());
    }

    if player_count == 2 {
        blinds_or_straddles.reverse();
    }

    let button_index = player_count - 1;

    // TODO:
    // What if someone straddles from the blinds
    // or some other blind structure is used?
    let (small_blind, big_blind) = if player_count == 2 {
        (blinds_or_straddles[1], blinds_or_straddles[0])
    } else {
        (blinds_or_straddles[0], blinds_or_straddles[1])
    };

    let max_players = entry
        .seat_count
        .map(|n| usize::from(n))
        .unwrap_or(Game::MAX_PLAYERS);

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

    game.post_small_and_big_blind()?;

    // The PHH format does not differentiate between posts and straddles.

    for (player, post) in blinds_or_straddles.iter().copied().enumerate().skip(2) {
        if post != 0 && post <= big_blind {
            game.additional_post(player, post, false)?;
        }
    }

    for (player, straddle) in blinds_or_straddles.iter().copied().enumerate().skip(2) {
        if straddle > big_blind {
            game.straddle(player, straddle)?;
        }
    }

    Ok(game)
}

fn game_update_metadata(game: &mut Game, entry: &Entry) -> Result<()> {
    game.set_unit(Arc::new("ct".to_owned()));

    if let Some(seat_count) = entry.seat_count {
        game.set_max_players(usize::from(seat_count))?;
    }

    if let Some(venue) = entry.venue.clone() {
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

    if let Some(table) = entry.table.clone() {
        game.set_table_name(Arc::new(table));
    }

    if let Some(hand) = entry.hand.clone() {
        game.set_hand_name(Arc::new(hand));
    }

    Ok(())
}

fn parse_player_hands(game: &mut Game, actions: &[String]) -> Result<()> {
    // Don't validate or report some parsing errors, just skip.
    // The actual parsing happens later anyway.

    for action in actions {
        let mut split = action.split(' ');

        let (Some(actor), Some(action_kind)) = (split.next(), split.next()) else {
            continue;
        };

        let player_hand = match (actor, action_kind) {
            ("d", "dh") => {
                let Some(player) = split.next() else {
                    return Err("missing player in hole card deal".into());
                };

                let Some(hand) = split.next() else {
                    return Err("missing hand in hole card deal".into());
                };

                Some((player, hand))
            }
            (player, "sm") => {
                if let Some(hand) = split.next() {
                    Some((player, hand))
                } else {
                    None
                }
            }
            _ => continue,
        };

        let Some((player, hand)) = player_hand else {
            continue;
        };

        let player = parse_action_player(player)?;

        if hand != "-" && hand != "????" {
            let Some(hand) = Cards::from_str(hand)?.to_hand() else {
                return Err("could not convert cards to player hand".into());
            };

            game.set_hand(usize::from(player), hand)?;
        }
    }

    Ok(())
}

fn parse_actions(game: &mut Game, actions: &[String]) -> Result<()> {
    let mut show_muck = Bitset::<2>::EMPTY;

    for action in actions {
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
                handle_show_muck(game, show_muck)?;

                let Some(community_cards) = split.next() else {
                    return Err("missing community cards in deal".into());
                };

                let community_cards = Cards::from_str(community_cards)?;

                // TODO:
                // Does PHH support multiple board runouts?
                // How are they represented?
                let State::Street(street) = game.state() else {
                    return Err(
                        "unexpected community card deal: game state does not expect new street"
                            .into(),
                    );
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
                split.next();
                split.next();
                ()
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
                split.next();

                let player = usize::from(parse_action_player(actor)?);

                if player >= game.player_count() {
                    return Err("invalid player index in show/muck".into());
                }

                // Order does not always match what we expect,
                // save for later usage.
                show_muck.set(player);
            }
            _ => return Err(format!("unsupported action kind {action_kind}").into()),
        }

        if split.next().is_some() {
            return Err("invalid action format: unexpected data at end".into());
        }

        if matches!(game.state(), State::UncalledBet { .. }) {
            game.uncalled_bet()?;
        }
    }

    handle_show_muck(game, show_muck)?;

    Ok(())
}

fn parse_action_player(player: &str) -> Result<u8> {
    if player.len() < 2 {
        return Err(format!("invalid action player format '{player}': not long enough").into());
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

fn handle_show_muck(game: &mut Game, show_muck: Bitset<2>) -> Result<()> {
    for _ in 0..Game::MAX_PLAYERS {
        let State::ShowOrMuck(player) = game.state() else {
            break;
        };

        if !show_muck.has(player) {
            return Err("expected show/muck for player".into());
        }

        if game.get_hand(usize::from(player)).is_some() {
            // We assume shows if we know the player hand.
            game.show_hand()?;
        } else {
            game.muck_hand()?;
        }
    }

    Ok(())
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
