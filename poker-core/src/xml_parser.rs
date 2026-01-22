use std::{
    array,
    result::Result as StdResult,
    str::FromStr,
    sync::{Arc, LazyLock},
};

use chrono::NaiveDateTime;
use serde::{Deserialize, Deserializer};
use serde_with::DeserializeFromStr;
use serde_xml_rs::SerdeXml;

use crate::{
    card::Card,
    cards::Cards,
    game::{self, Action, Amount, Game, Player, Seat, State},
    rank::Rank,
    result::{Error, Result},
    suite::Suite,
};

pub fn parse_xml_str(xml: &str) -> Result<Vec<Result<Game>>> {
    // TODO: As Iterator.

    let mut session: Session = SerdeXml::new().overlapping_sequences(true).from_str(xml)?;

    if session.general.mode != "real" {
        return Err("session mode is not `real`".into());
    }

    let (small_blind, big_blind) = match exact_chunks(session.general.game_type.split(' ')) {
        Some(["Holdem", "NL", stake]) => match exact_chunks(stake.split('/')) {
            Some([small_blind, big_blind]) => {
                (Price::from_str(small_blind)?, Price::from_str(big_blind)?)
            }
            _ => return Err("invalid `game_type` stake format".into()),
        },
        _ => return Err("invalid `game_type` format".into()),
    };

    if session.general.table_currency != CURRENCY_KIND {
        return Err("invalid `tablecurrency` kind".into());
    }

    if session.general.currency != CURRENCY_KIND {
        return Err("invalid `currency` kind".into());
    }

    if session.general.game_count != session.games.len() {
        return Err(format!(
            "`gamecount` ({}) does not match number of games ({})",
            session.general.game_count,
            session.games.len(),
        )
        .into());
    }

    let mut out = Vec::new();

    for game in &mut session.games {
        let result = game
            .to_game(
                session.general.table_name.clone(),
                &session.general.nickname,
                session.general.table_size,
                small_blind,
                big_blind,
            )
            .map_err(|err| format!("error in game `{}`: {}", game.game_code, err).into());

        out.push(result);
    }

    Ok(out)
}

#[derive(Debug, Deserialize)]
struct Session {
    general: SessionGeneral,

    #[serde(rename = "game")]
    games: Vec<GameData>,
}

#[derive(Debug, Deserialize)]
struct SessionGeneral {
    mode: String,
    #[serde(rename = "gametype")]
    game_type: String,
    #[serde(rename = "tablename")]
    table_name: Arc<String>,
    #[serde(rename = "tablecurrency")]
    table_currency: String,
    currency: String,
    nickname: String,
    #[serde(rename = "gamecount")]
    game_count: usize,
    #[serde(rename = "tablesize")]
    table_size: u8,
}

#[derive(Debug, Deserialize)]
struct GameData {
    #[serde(rename = "@gamecode")]
    game_code: Arc<String>,

    general: GameGeneral,

    #[serde(rename = "round")]
    rounds: Vec<Round>,
}

impl GameData {
    fn to_game(
        &mut self,
        table_name: Arc<String>,
        hero_name: &str,
        table_size: u8,
        small_blind: Price,
        big_blind: Price,
    ) -> Result<Game> {
        self.general
            .players
            .players
            .sort_by_key(|player| player.seat);

        if !self
            .general
            .players
            .players
            .iter()
            .all(|player| !player.cash_out && player.cash_out_fee.0 == 0)
        {
            return Err("`cashout` currently not supported".into());
        }

        let players: Vec<_> = self
            .general
            .players
            .players
            .iter()
            .map(|player| game::PlayerData {
                name: Some(player.name.clone()),
                // TODO: Could give a nicer error message if this fails.
                seat: player
                    .seat
                    .checked_sub(1)
                    .and_then(|player| Seat::try_from(player).ok()),
                hand: None,
                starting_stack: player.chips.0.into(),
            })
            .collect();

        let button_index = self
            .general
            .players
            .players
            .iter()
            .position(|player| player.dealer);
        let Some(button_index) = button_index else {
            return Err("dealer not set".into());
        };

        let hero_index = self
            .general
            .players
            .players
            .iter()
            .position(|player| player.name.as_str() == hero_name);

        let mut game = Game::new(
            &players,
            Player::try_from(button_index).unwrap(),
            small_blind.0.into(),
            big_blind.0.into(),
        )?;
        game.set_max_players(table_size.into())?;
        game.set_unit(UNIT.clone());
        game.set_date(self.general.start_date);
        game.set_location(LOCATION.clone());
        game.set_table_name(table_name);
        game.set_hand_name(self.game_code.clone());

        if let Some(hero_index) = hero_index {
            game.set_hero(Player::try_from(hero_index)?)?;
        };

        if self.rounds.len() < 2 || self.rounds.len() > 5 {
            return Err("invalid number of rounds".into());
        }

        if !self
            .rounds
            .iter()
            .enumerate()
            .all(|(index, round)| usize::from(round.no) == index)
        {
            return Err("invalid round number(s)".into());
        }

        if !self
            .rounds
            .iter()
            .flat_map(|round| &round.actions)
            .enumerate()
            .all(|(index, action)| action.no.checked_sub(1).is_some_and(|n| n == index))
        {
            return Err("invalid action number(s)".into());
        }

        if !self
            .rounds
            .iter()
            .flat_map(|round| &round.actions)
            .all(|action| {
                (action.dealt.is_none() || action.dealt == Some(true))
                    && (action.discard.is_none() || action.discard == Some(true))
            })
        {
            // I currently don't know the purpose of these fields,
            // assert and ignore for now.
            return Err("invalid action dealt or discard state".into());
        }

        if self.rounds[0].actions.len() < 2 {
            return Err("malformed post round: expected at least two actions".into());
        }

        let small_blind_action = &self.rounds[0].actions[0];
        let big_blind_action = &self.rounds[0].actions[1];

        if small_blind_action.kind != ActionKind::PostSmallBlind
            || small_blind_action.player != game.player_name(game.small_blind_player())
        {
            return Err("invalid small blind post".into());
        }

        if big_blind_action.kind != ActionKind::PostBigBlind
            || big_blind_action.player != game.player_name(game.big_blind_player())
        {
            return Err("invalid big blind post".into());
        }

        game.post_small_and_big_blind()?;

        for action in &self.rounds[0].actions[2..] {
            let Some(player) = game.player_by_name(&action.player) else {
                return Err("unknown player name".into());
            };

            if !matches!(
                action.kind,
                ActionKind::PostSmallBlind | ActionKind::PostBigBlind,
            ) {
                return Err("only posts allowed in round zero".into());
            }

            game.additional_post(player, action.sum.0.into(), false)?;
        }

        for round in &self.rounds[1..] {
            for cards_data in &round.cards {
                let player = cards_data
                    .player
                    .as_ref()
                    .and_then(|name| game.player_by_name(&name));

                let cards = cards_data
                    .cards
                    .split(' ')
                    .filter(|card| *card != "X")
                    .map(|card| parse_card(card))
                    .collect::<Result<Vec<_>>>()?;

                match cards_data.kind {
                    CardsKind::Pocket if !cards.is_empty() => {
                        let hand = Cards::from_slice(&cards).and_then(|cards| cards.to_hand());
                        let (Some(player), Some(hand)) = (player, hand) else {
                            return Err("invalid player hand or unknown player name".into());
                        };
                        game.set_hand(player, hand)?;
                    }
                    CardsKind::Pocket => (),
                    CardsKind::Flop if cards_data.board == 1 => {
                        let Ok(flop) = <[Card; 3]>::try_from(cards) else {
                            return Err("invalid flop".into());
                        };
                        game.flop(flop)?;
                    }
                    CardsKind::Turn if cards_data.board == 1 => {
                        if cards.len() != 1 {
                            return Err("invalid turn".into());
                        }
                        game.turn(cards[0])?;
                    }
                    CardsKind::River if cards_data.board == 1 => {
                        if cards.len() != 1 {
                            return Err("invalid river".into());
                        }
                        game.river(cards[0])?;
                    }
                    _ => (),
                }
            }

            for action in &round.actions {
                let Some(player) = game.player_by_name(&action.player) else {
                    return Err("unknown player name".into());
                };

                if game.current_player() != Some(player) {
                    return Err(format!(
                        "unexpected player action: action={:?} game_state={:?}",
                        action,
                        game.state()
                    )
                    .into());
                }

                match action.kind {
                    ActionKind::Fold => game.fold()?,
                    ActionKind::PostSmallBlind | ActionKind::PostBigBlind => {
                        return Err("invalid post action".into())
                    }
                    ActionKind::Call => {
                        let Some(call_amount) = game.can_call() else {
                            return Err("call not allowed in current state".into());
                        };
                        if call_amount != action.sum.0.into() {
                            return Err("call amount does not match expected amount".into());
                        }
                        game.call()?;
                    }
                    ActionKind::Check => game.check()?,
                    ActionKind::Bet => game.bet(action.sum.0.into())?,
                    ActionKind::AllIn => {
                        if let Some(_) = game.can_all_in() {
                            let amount = game.current_stack().unwrap();

                            if amount != Amount::from(action.sum.0) {
                                return Err(format!(
                                    "all-in amount {} does not match expected amount {}",
                                    amount, action.sum.0
                                )
                                .into());
                            }

                            game.all_in()?
                        } else if let Some(call_amount) = game.can_call() {
                            if call_amount != action.sum.0.into() {
                                return Err("call amount does not match expected amount".into());
                            }

                            game.call()?
                        } else {
                            return Err("all-in not allowed in current state".into());
                        }
                    }
                    ActionKind::Raise => game.raise(action.sum.0.into())?,
                }
            }

            if matches!(game.state(), State::UncalledBet { .. }) {
                self.general.players.check_invested(&game)?;
                game.uncalled_bet()?;
            }

            for _ in 0..Game::MAX_PLAYERS {
                let State::ShowOrMuck(_) = game.state() else {
                    break;
                };

                // Ignore muck info, can be confusing with pre river all-ins,
                // depending on how the site handles that.

                // Assume the hand is set if we go to a showdown.
                game.show_hand()?;
            }
        }

        // Counting from one.
        for board in 2..=Game::MAX_RUNOUTS {
            for round in &self.rounds {
                for cards_data in &round.cards {
                    if usize::from(cards_data.board) != board {
                        continue;
                    }

                    let cards = cards_data
                        .cards
                        .split(' ')
                        .map(|card| parse_card(card))
                        .collect::<Result<Vec<_>>>()?;

                    match cards_data.kind {
                        CardsKind::Pocket => {
                            return Err(
                                "cards type `Pocket` not allowed with `board` attribute".into()
                            )
                        }
                        CardsKind::Flop => {
                            let Ok(flop) = <[Card; 3]>::try_from(cards) else {
                                return Err("invalid flop".into());
                            };
                            game.flop(flop)?;
                        }
                        CardsKind::Turn => {
                            if cards.len() != 1 {
                                return Err("invalid turn".into());
                            }
                            game.turn(cards[0])?;
                        }
                        CardsKind::River => {
                            if cards.len() != 1 {
                                return Err("invalid river".into());
                            }
                            game.river(cards[0])?;
                        }
                    }
                }
            }
        }

        // Counting from one.
        if self
            .rounds
            .iter()
            .flat_map(|round| &round.cards)
            .any(|cards_data| usize::from(cards_data.board) > Game::MAX_RUNOUTS)
        {
            return Err("more runouts than maximally supported".into());
        }

        match game.state() {
            State::Post | State::End => unreachable!(),
            State::Player(_) | State::Street(_) => {
                return Err("expected more player or street actions".into())
            }
            State::UncalledBet { .. } => return Err("unexpected uncalled bet required".into()),
            State::ShowOrMuck(_) => return Err("unexpected show or muck required".into()),
            State::ShowdownOrNextRunout => {
                if game
                    .actions()
                    .iter()
                    .find(|action| matches!(action, Action::UncalledBet { .. }))
                    .is_none()
                {
                    self.general.players.check_invested(&game)?;
                }

                let total_rake = self
                    .general
                    .players
                    .players
                    .iter()
                    .map(|player| player.rake_amount)
                    .fold(Some(Amount::ZERO), |acc, n| {
                        acc.and_then(|acc| acc.checked_add(n.0.into()))
                    });

                let Some(total_rake) = total_rake else {
                    return Err("total rake calculation overflowed".into());
                };

                if total_rake >= game.total_pot() {
                    return Err("total rake greater or equal to the pot".into());
                }

                // TODO: Could check the winnings.
                let pot_share = self
                    .general
                    .players
                    .players
                    .iter()
                    .map(|player| Amount::from(player.win.0));

                match game.showdown_custom(total_rake, game.players().zip(pot_share.clone())) {
                    Ok(()) => (),
                    Err(_) => {
                        // In some example hands, the rake was off by one,
                        // probably caused by bad rounding from the site.
                        // Correct for this here.
                        game.showdown_custom(
                            total_rake.checked_add(1.into()).unwrap(),
                            game.players().zip(pot_share.clone()),
                        )?
                    }
                }
            }
        }

        assert!(matches!(game.state(), State::End));
        Ok(game)
    }
}

fn parse_card(card: &str) -> Result<Card> {
    if card.len() < 2 {
        return Err(format!("card {card}: invalid format").into());
    }

    let suite = Suite::from_ascii(card.as_bytes()[0].to_ascii_lowercase())?;

    let rank = match card[1..].as_bytes() {
        b"10" => Rank::Ten,
        [rank] => Rank::from_ascii(*rank)?,
        _ => return Err(format!("card {card}: unknown rank").into()),
    };

    Ok(Card::of(rank, suite))
}

#[derive(Debug, Deserialize)]
struct GameGeneral {
    #[serde(rename = "startdate")]
    #[serde(deserialize_with = "de_datetime")]
    start_date: NaiveDateTime,
    players: Players,
}

#[derive(Debug, Deserialize)]
struct Players {
    #[serde(rename = "player")]
    players: Vec<PlayerData>,
}

impl Players {
    fn check_invested(&self, game: &Game) -> Result<()> {
        if !game
            .players()
            .zip(&self.players)
            .all(|(index, player)| game.invested(index) == player.bet.0.into())
        {
            let investments_formatted: String = game
                .players()
                .zip(&self.players)
                .map(|(index, player)| {
                    format!(
                        "{}: expected: {}, got: {}\n",
                        player.name,
                        game.invested(index),
                        player.bet.0
                    )
                })
                .collect();

            Err(format!("player investments don't match player data `bet` amounts:\n{investments_formatted}").into())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Deserialize)]
struct PlayerData {
    #[serde(rename = "@seat")]
    seat: u8,

    #[serde(rename = "@name")]
    name: Arc<String>,

    #[serde(rename = "@chips")]
    chips: Price,

    #[serde(rename = "@dealer")]
    dealer: bool,

    #[serde(rename = "@win")]
    win: Price,

    #[serde(rename = "@bet")]
    bet: Price,

    #[serde(rename = "@cashout", default)]
    cash_out: bool,

    #[serde(rename = "@cashout_fee", default)]
    cash_out_fee: Price,

    #[serde(rename = "@rakeamount", default)]
    rake_amount: Price,
}

#[derive(Debug, Deserialize)]
struct Round {
    #[serde(rename = "@no")]
    no: u8,

    #[serde(rename = "cards")]
    #[serde(default)]
    cards: Vec<CardsData>,

    #[serde(rename = "action", default)]
    actions: Vec<ActionData>,
}

#[derive(Debug, Deserialize)]
struct CardsData {
    #[serde(rename = "@type")]
    kind: CardsKind,

    #[serde(rename = "@player")]
    player: Option<String>,

    #[serde(
        rename = "@board",
        default = "default_board",
        deserialize_with = "de_board"
    )]
    board: u8,

    #[serde(rename = "#text")]
    cards: String,
}

fn default_board() -> u8 {
    1
}

#[derive(Debug, Deserialize)]
enum CardsKind {
    Pocket,
    Flop,
    Turn,
    River,
}

#[derive(Debug, Deserialize)]
struct ActionData {
    #[serde(rename = "@no")]
    no: usize,

    #[serde(rename = "@player")]
    player: String,

    #[serde(rename = "@type")]
    #[serde(deserialize_with = "de_action_kind")]
    kind: ActionKind,

    #[serde(rename = "@discard")]
    discard: Option<bool>,

    #[serde(rename = "@dealt")]
    dealt: Option<bool>,

    #[serde(rename = "@sum")]
    sum: Price,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
enum ActionKind {
    Fold,
    PostSmallBlind,
    PostBigBlind,
    Call,
    Check,
    Bet,
    AllIn,
    Raise,
}

impl TryFrom<u8> for ActionKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        use ActionKind::*;

        match value {
            0 => Ok(Fold),
            1 => Ok(PostSmallBlind),
            2 => Ok(PostBigBlind),
            3 => Ok(Call),
            4 => Ok(Check),
            5 => Ok(Bet),
            7 => Ok(AllIn),
            15 => Err("ante currently not supported".into()),
            23 => Ok(Raise),
            _ => Err(format!("unknown action type {value}").into()),
        }
    }
}

fn de_action_kind<'de, D>(deserializer: D) -> StdResult<ActionKind, D::Error>
where
    D: Deserializer<'de>,
{
    let n = u8::deserialize(deserializer)?;
    ActionKind::try_from(n).map_err(serde::de::Error::custom)
}

fn de_board<'de, D>(deserializer: D) -> StdResult<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;

    value
        .strip_prefix("board")
        .unwrap_or(&value)
        .parse()
        .map_err(serde::de::Error::custom)
}

fn de_datetime<'de, D>(deserializer: D) -> StdResult<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").map_err(serde::de::Error::custom)
}

#[derive(Debug, Clone, Copy, DeserializeFromStr, Default, PartialEq, Eq)]
struct Price(u32);

impl FromStr for Price {
    type Err = Error;

    fn from_str(price: &str) -> Result<Self> {
        let Some(without_unit) = price.strip_prefix(UNIT_SYMBOL) else {
            return Err(format!("price {price}: missing prefix unit symbol {UNIT_SYMBOL}").into());
        };

        let decimal_separator = if without_unit.contains(',') { ',' } else { '.' };

        let mut split = without_unit.split(decimal_separator);
        let dollar: u32 = split.next().unwrap().parse()?;
        let cent = match split.next() {
            Some(s) => {
                let cent: u32 = s.parse()?;
                if s.len() == 1 {
                    cent * 10
                } else if s.len() == 2 {
                    cent
                } else {
                    return Err(format!("price {price}: invalid format").into());
                }
            }
            None => 0,
        };
        if split.next().is_some() {
            return Err(format!("price {price}: invalid format").into());
        }

        let Some(price) = dollar.checked_mul(100).and_then(|n| n.checked_add(cent)) else {
            return Err(format!("price {price}: too large").into());
        };
        Ok(Self(price))
    }
}

fn exact_chunks<T: Default, const N: usize>(iter: impl Iterator<Item = T>) -> Option<[T; N]> {
    let mut arr = array::from_fn(|_| T::default());
    let mut index = 0;

    for entry in iter {
        if index >= arr.len() {
            return None;
        }
        arr[index] = entry;
        index += 1;
    }

    if index != arr.len() {
        None
    } else {
        Some(arr)
    }
}

const CURRENCY_KIND: &str = "EUR";

const UNIT_SYMBOL: &str = "€";

static LOCATION: LazyLock<Arc<String>> = LazyLock::new(|| Arc::new("XML".to_owned()));

static UNIT: LazyLock<Arc<String>> = LazyLock::new(|| Arc::new("ct".to_owned()));
