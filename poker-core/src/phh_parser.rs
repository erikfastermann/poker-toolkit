use std::{str::FromStr, sync::Arc};

use bigdecimal::{BigDecimal, ToPrimitive, Zero};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};
use toml::value::Time;

use crate::{
    cards::Cards,
    game::{Game, Player, State, Street},
    result::Result,
};

pub fn parse_phhs_str(
    phhs: &str,
    skip_non_zero_ante: bool,
) -> Result<impl Iterator<Item = Result<Game>>> {
    let entries: IndexMap<String, Entry> = toml::from_str(phhs)?;
    Ok(entries
        .into_values()
        .filter(move |entry| !skip_non_zero_ante || entry.antes.iter().all(|ante| ante.is_zero()))
        .map(Entry::to_game))
}

#[derive(Debug, Deserialize)]
struct Entry {
    variant: String,
    antes: Vec<BigDecimal>,
    blinds_or_straddles: Vec<BigDecimal>,
    min_bet: BigDecimal,
    /// Can be inf, which we don't parse in this case.
    starting_stacks: Vec<BigDecimal>,
    actions: Vec<String>,

    venue: Option<String>,
    time: Option<Time>, // either in time zone, location or utc
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
    winnings: Option<Vec<BigDecimal>>,
    currency_symbol: Option<String>,
}

impl Entry {
    fn to_game(self) -> Result<Game> {
        if self.variant != "NT" {
            return Err(format!("only no-limit hold'em supported, not '{}'", self.variant).into());
        }

        let non_zero_ante = self.antes.iter().any(|ante| !ante.is_zero());
        if non_zero_ante {
            return Err("non zero ante not supported".into());
        }

        // We output with currency ct and adjust the sizings accordingly.
        if !self
            .currency_symbol
            .as_ref()
            .is_some_and(|symbol| symbol == "$")
        {
            return Err("expected currency symbol dollar".into());
        }

        let player_count = self.blinds_or_straddles.len();
        if player_count < Game::MIN_PLAYERS || player_count > Game::MAX_PLAYERS {
            return Err("bad player count".into());
        }

        let button_index = if player_count == 2 {
            0
        } else {
            player_count - 1
        };

        // TODO: What if someone straddles from the blinds?
        let small_blind = convert_chips(&self.blinds_or_straddles[0])?;
        let big_blind = convert_chips(&self.blinds_or_straddles[1])?;

        let max_players = self
            .seat_count
            .map(|n| usize::from(n))
            .unwrap_or(player_count);

        if convert_chips(&self.min_bet)? != big_blind {
            return Err("min bet is not equal to the big blind".into());
        }

        let mut players = Vec::new();

        for player_index in 0..player_count {
            let name = self
                .players
                .as_ref()
                .and_then(|players| players.get(player_index))
                .map(|name| Arc::new(name.clone()));

            let seat = self
                .seats
                .as_ref()
                .and_then(|seats| seats.get(player_index).copied())
                .and_then(|seat| seat.checked_sub(1));

            if seat.is_some_and(|seat| usize::from(seat) >= max_players) {
                return Err("invalid seat config: bigger than seat or player count".into());
            }

            let Some(starting_stack) = self.starting_stacks.get(player_index) else {
                return Err("starting_stacks has invalid length".into());
            };
            let starting_stack = convert_chips(starting_stack)?;

            let player = Player {
                name,
                seat,
                hand: None,
                starting_stack,
            };

            players.push(player);
        }

        let mut game = Game::new(&players, button_index, small_blind, big_blind)?;

        game.set_unit(Arc::new(self.currency_symbol.unwrap()));

        game.set_max_players(max_players)?;

        if let Some(venue) = self.venue {
            game.set_location(Arc::new(venue));
        }

        // Ignoring time zone information, game currently only stores `NaiveDateTime`.
        match (self.year, self.month, self.day, self.time) {
            (Some(year), Some(month), Some(day), Some(time)) => {
                let date = NaiveDate::from_ymd_opt(year, month.into(), day.into());
                let time = NaiveTime::from_hms_opt(
                    time.hour.into(),
                    time.minute.into(),
                    time.second.into(),
                );

                let (Some(date), Some(time)) = (date, time) else {
                    return Err("invalid year, month, day or time value".into());
                };

                let datetime = NaiveDateTime::new(date, time);

                game.set_date(datetime);
            }
            (None, None, None, None) => (),
            _ => return Err("year, month, day and time all have to be set or none of them".into()),
        }

        if let Some(table) = self.table {
            game.set_table_name(Arc::new(table));
        }

        if let Some(hand) = self.hand {
            game.set_hand_name(Arc::new(hand));
        }

        game.post_small_and_big_blind()?;

        // The PHH format does not differentiate between posts and straddles.

        for (player, post) in self.blinds_or_straddles.iter().enumerate().skip(2) {
            let post = convert_chips(post)?;

            if post <= big_blind {
                game.additional_post(player, post, false)?;
            }
        }

        for (player, straddle) in self.blinds_or_straddles.iter().enumerate().skip(2) {
            let straddle = convert_chips(straddle)?;

            if straddle > big_blind {
                game.straddle(player, straddle)?;
            }
        }

        for action in self.actions {
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

                    let amount = parse_chips(amount)?;

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

        if let Some(winnings) = self.winnings {
            let total_winnings: BigDecimal = winnings.iter().sum();
            let total_winnings = convert_chips(&total_winnings)?;

            let Some(total_rake) = game.total_pot().checked_sub(total_winnings) else {
                return Err("total winnings is greater than total pot".into());
            };

            let player_pot_share = winnings
                .iter()
                .enumerate()
                .filter(|(_, winning)| !winning.is_zero())
                .map(|(player, winning)| Result::Ok((player, convert_chips(winning)?)))
                .collect::<Result<Vec<_>>>()?;

            game.showdown_custom(total_rake, player_pot_share.into_iter())?;
        } else {
            game.showdown_simple()?;
        }

        Ok(game)
    }
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

fn parse_chips(chips: &str) -> Result<u32> {
    convert_chips(&BigDecimal::from_str(chips)?)
}

fn convert_chips(n: &BigDecimal) -> Result<u32> {
    let ct = n.clone() * 100i32;

    if ct.abs().round(0) != ct {
        return Err(format!(
            "failed chip conversion: value {n} negative or not representable as cent"
        )
        .into());
    }

    let Some(ct) = ct.to_u32() else {
        return Err(format!(
            "failed chip conversion: value {n} cannot be represented as a u32 in cent"
        )
        .into());
    };

    Ok(ct)
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
