use std::{fmt::Write, path::Path, str::FromStr, sync::Arc};

use chrono::NaiveDateTime;
use rusqlite::{
    functions::FunctionFlags,
    params,
    types::{FromSql, FromSqlError, FromSqlResult, Type, Value, ValueRef},
    Connection, Params, Row, RowIndex, Transaction,
};

use crate::{
    bitset::Bitset,
    card::Card,
    cards::Cards,
    game::{Game, GameData, State, Street},
    hand,
    result::Result,
};

// TODO
// - Extra checks, e.g. every hand has matching metadata entries
// - Check entries for correctness when reading from the database

pub struct DB {
    conn: Connection,
}

const SCHEMA: &str = include_str!("schema.sql");

impl DB {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        // TODO: Extra open when not creating db.

        let conn = Connection::open(path)?;
        conn.pragma_update(None, "encoding", "UTF-8")?;
        conn.pragma_update(None, "synchronous", "EXTRA")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;

        conn.create_scalar_function("position", 3, FunctionFlags::SQLITE_DETERMINISTIC, |ctx| {
            let player_count: usize = ctx.get(0)?;
            let button_index: usize = ctx.get(1)?;
            let player: usize = ctx.get(2)?;
            let Some((short_name, _)) = Game::position_name(player_count, button_index, player)
            else {
                return Err(rusqlite::Error::UserFunctionError(
                    "position: invalid indices".into(),
                ));
            };
            Ok(short_name)
        })?;

        let db = Self { conn };
        db.check_schema()?;
        Ok(db)
    }

    fn check_schema(&self) -> Result<()> {
        let mem = Connection::open_in_memory()?;
        mem.execute_batch(SCHEMA)?;

        // Only a simple check, schemas might still be equal,
        // except for some formatting etc.
        if Self::schema(&self.conn)? != Self::schema(&mem)? {
            // TODO: Automatically update schema.
            Err("db: schema does not match expected schema".into())
        } else {
            Ok(())
        }
    }

    fn schema(conn: &Connection) -> Result<Vec<String>> {
        let mut stmt = conn.prepare(
            "SELECT sql FROM sqlite_schema
            WHERE name NOT LIKE 'sqlite_%'
            ORDER BY name",
        )?;
        let sql: std::result::Result<Vec<String>, _> =
            stmt.query_map((), |row| row.get(0))?.collect();
        Ok(sql?)
    }

    pub fn add_games<'a>(&mut self, games: impl Iterator<Item = &'a Game>) -> Result<u64> {
        let mut count = 0u64;
        let tx = self.conn.transaction()?;

        for game in games {
            if game.state() != State::End {
                return Err("db: added game not in end state".into());
            }

            if let Some(hand_name) = game.hand_name() {
                if Self::has_hand_name(&tx, &hand_name)? {
                    continue;
                }
            }

            let hand = HandBundle::from_game(&game);
            let id = Self::add_hand_data(&tx, &hand.data)?;
            Self::add_hand_info(&tx, hand.hand, id)?;
            for player in hand.players {
                Self::add_hand_player(&tx, player, id)?;
            }
            count += 1;
        }

        tx.commit()?;
        Ok(count)
    }

    fn add_hand_data(tx: &Transaction<'_>, data: &HandData) -> Result<u64> {
        tx.execute(
            "INSERT INTO hands_data(hand_data) VALUES(?)",
            (serde_json::to_string(&data.data)?,),
        )?;
        Ok(u64::try_from(tx.last_insert_rowid())?)
    }

    fn add_hand_info(tx: &Transaction<'_>, hand: Hand, id: u64) -> Result<()> {
        tx.execute(
            "INSERT INTO hands(
                id,
                unit,
                max_players,
                game_location,
                game_date,
                table_name,
                hand_name,
                hero_index,
                player_count,
                small_blind,
                big_blind,
                button_index,
                first_flop,
                first_turn,
                first_river,
                pot_kind,
                posting,
                straddling,
                pre_flop_limping,
                pre_flop_cold_calling,
                players_post_flop,
                players_at_showdown,
                single_winner,
                final_full_pot_size
            ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                hand.unit,
                hand.max_players,
                hand.game_location,
                hand.game_date,
                hand.table_name,
                hand.hand_name,
                hand.hero_index,
                hand.player_count,
                hand.small_blind,
                hand.big_blind,
                hand.button_index,
                hand.first_flop.map(|flop| flop.to_string()),
                hand.first_turn.map(|turn| turn.to_string()),
                hand.first_river.map(|river| river.to_string()),
                hand.pot_kind.to_str(),
                hand.posting,
                hand.straddling,
                hand.pre_flop_limping,
                hand.pre_flop_cold_calling,
                hand.players_post_flop,
                hand.players_at_showdown,
                hand.single_winner,
                hand.final_full_pot_size,
            ],
        )?;
        Ok(())
    }

    fn add_hand_player(tx: &Transaction<'_>, player: HandPlayer, hand_id: u64) -> Result<()> {
        tx.execute(
            "INSERT INTO hands_players(
                hand_id,
                player,
                player_name,
                seat,
                hand,
                went_to_showdown,
                starting_stack,
                pot_contribution,
                showdown_stack,
                pre_flop_action,
                flop_action,
                turn_action,
                river_action
            ) VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                hand_id,
                player.player,
                player.player_name,
                player.seat,
                player.hand.map(|hand| hand.to_string()),
                player.went_to_showdown,
                player.starting_stack,
                player.pot_contribution,
                player.showdown_stack,
                player.pre_flop_action.to_string().unwrap(),
                player.flop_action.to_string(),
                player.turn_action.to_string(),
                player.river_action.to_string(),
            ],
        )?;
        Ok(())
    }

    fn has_hand_name(tx: &Transaction<'_>, hand_name: &str) -> Result<bool> {
        let mut hands_with_name = tx.prepare("SELECT COUNT(*) FROM hands WHERE hand_name = ?")?;
        let count: u64 = hands_with_name.query_row((hand_name,), |row| row.get(0))?;
        Ok(count != 0)
    }

    pub fn query_for_each(
        &self,
        query: &str,
        params: impl Params,
        mut f: impl FnMut(&[Value]) -> Result<bool>,
    ) -> Result<()> {
        let mut stmt = self.conn.prepare(query)?;
        // TODO: Can still potentially modify the database.
        if !stmt.readonly() {
            return Err("db: non readonly query".into());
        }

        let mut rows = stmt.query(params)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next()? {
            values.truncate(0);

            for index in 0..usize::MAX {
                let value: Value = match row.get(index) {
                    Ok(value) => value,
                    Err(rusqlite::Error::InvalidColumnIndex(_)) => break,
                    Err(err) => return Err(err.into()),
                };
                values.push(value);
            }

            if !f(&values)? {
                break;
            }
        }

        Ok(())
    }

    pub fn load_hands_from_query(
        &self,
        query: &str,
        params: impl Params,
    ) -> Result<Vec<(Hand, Option<HandPlayer>)>> {
        let mut stmt = self.conn.prepare(query)?;
        // TODO: Can still potentially modify the database.
        if !stmt.readonly() {
            return Err("db: running non readonly query to get hands".into());
        }

        let hands = stmt
            .query_map(params, |row| {
                let hand = Hand::from_row(row)?;
                let hand_player = HandPlayer::from_row(row)?;
                Ok((hand, hand_player))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(hands)
    }

    pub fn get_game_data(&self, hand_id: u64) -> Result<GameData> {
        let mut stmt = self
            .conn
            .prepare("SELECT hand_data FROM hands_data WHERE id = ?")?;
        let game_data: String = stmt.query_row((hand_id,), |row| row.get("hand_data"))?;
        let game_data: GameData = serde_json::from_str(&game_data)?;
        Ok(game_data)
    }
}

fn get_string(row: &Row<'_>, idx: impl RowIndex) -> rusqlite::Result<Option<Arc<String>>> {
    let v = row.get::<_, Option<String>>(idx)?;
    Ok(v.map(|v| Arc::new(v)))
}

impl FromSql for crate::hand::Hand {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let hand = value.as_str()?;
        crate::hand::Hand::from_str(&hand).map_err(|err| FromSqlError::Other(err))
    }
}

impl FromSql for Card {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = value.as_str()?;
        Card::from_str(s).map_err(|err| FromSqlError::Other(err))
    }
}

#[derive(Debug, Clone)]
pub struct Hand {
    pub id: Option<u64>,
    pub unit: Option<Arc<String>>,
    pub max_players: Option<u8>,
    pub game_location: Option<Arc<String>>,
    pub game_date: Option<NaiveDateTime>,
    pub table_name: Option<Arc<String>>,
    pub hand_name: Option<Arc<String>>,
    pub hero_index: Option<u8>,

    pub player_count: u8,
    pub small_blind: u32,
    pub big_blind: u32,
    pub button_index: u8,

    pub first_flop: Option<Flop>,
    pub first_turn: Option<Card>,
    pub first_river: Option<Card>,

    pub pot_kind: PotKind,
    pub posting: bool,
    pub straddling: bool,
    pub pre_flop_limping: bool,
    pub pre_flop_cold_calling: bool,

    pub players_post_flop: Option<u8>,
    pub players_at_showdown: Option<u8>,
    pub single_winner: Option<u8>,
    pub final_full_pot_size: u32,
}

impl Hand {
    fn from_game(game: &Game, game_data: &GameData) -> Self {
        assert_eq!(game.state(), State::End);
        debug_assert_eq!(&game.to_game_data(), game_data);

        let first_runout = game.runouts()[0];

        let players_at_showdown = {
            let not_folded = game.players_not_folded().count();
            if not_folded <= 1 {
                None
            } else {
                Some(u8::try_from(not_folded).unwrap())
            }
        };

        let mut winners = (0..game.player_count()).filter(|player| {
            game.current_stacks()[*player] > game.current_street_stacks()[*player]
        });
        let mut single_winner = winners.next().map(|n| u8::try_from(n).unwrap());
        if winners.next().is_some() {
            single_winner = None;
        }

        let mut hand = Self {
            id: None,
            unit: game.unit(),
            max_players: game_data.max_players,
            game_location: game.location(),
            game_date: game.date(),
            table_name: game.table_name(),
            hand_name: game.hand_name(),
            player_count: u8::try_from(game.player_count()).unwrap(),
            small_blind: game.small_blind(),
            big_blind: game.big_blind(),
            button_index: game_data.button_index,
            hero_index: game_data.hero_index,
            first_flop: first_runout.flop().map(|flop| Flop(flop)),
            first_turn: first_runout.turn(),
            first_river: first_runout.river(),
            pot_kind: PotKind::Walk,
            posting: false,
            straddling: false,
            pre_flop_limping: false,
            pre_flop_cold_calling: false,
            players_post_flop: None,
            players_at_showdown,
            single_winner,
            final_full_pot_size: game.total_pot(),
        };

        hand.set_pre_flop(game);
        hand
    }

    fn set_pre_flop(&mut self, game: &Game) {
        use crate::game::Action::*;
        use PotKind::*;

        assert_eq!(game.state(), State::End);
        assert_eq!(self.pot_kind, PotKind::Walk);

        let actions = game
            .actions()
            .iter()
            .copied()
            .take_while(|action| !matches!(action, crate::game::Action::Flop(_)));

        let mut previous_raisers = Bitset::<2>::EMPTY;
        let mut folded = Bitset::<2>::EMPTY;
        for action in actions {
            match (action, self.pot_kind) {
                (Post { player, .. }, _)
                    if usize::from(player) != game.small_blind_index()
                        && usize::from(player) != game.big_blind_index() =>
                {
                    self.posting = true;
                }
                (Straddle { .. }, _) => {
                    self.straddling = true;
                }
                (Fold(player), _) => {
                    folded.set(usize::from(player));
                }
                (Call { .. }, Walk | Limped) => {
                    self.pre_flop_limping = true;
                    self.pot_kind = Limped;
                }
                (Call { player, .. }, ThreeBet | FourBet | FiveBetPlus)
                    if !previous_raisers.has(usize::from(player)) =>
                {
                    self.pre_flop_cold_calling = true;
                }
                (Raise { .. }, Walk | Limped) => {
                    self.pot_kind = SRP;
                }
                (Raise { .. }, SRP) => {
                    self.pot_kind = ThreeBet;
                }
                (Raise { .. }, ThreeBet) => {
                    self.pot_kind = FourBet;
                }
                (Raise { .. }, FourBet) => {
                    self.pot_kind = FiveBetPlus;
                }
                _ => (),
            }

            if let Raise { player, .. } = action {
                previous_raisers.set(usize::from(player));
            }
        }

        let remaining_players = game.player_count() - usize::try_from(folded.count()).unwrap();
        if remaining_players > 1 {
            self.players_post_flop = Some(u8::try_from(remaining_players).unwrap());
        }
    }

    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let id: u64 = row.get("id")?;
        let hand = Self {
            id: Some(id),
            unit: get_string(row, "unit")?,
            max_players: row.get("max_players")?,
            game_location: get_string(row, "game_location")?,
            game_date: row.get("game_date")?,
            table_name: get_string(row, "table_name")?,
            hand_name: get_string(row, "hand_name")?,
            hero_index: row.get("hero_index")?,
            player_count: row.get("player_count")?,
            small_blind: row.get("small_blind")?,
            big_blind: row.get("big_blind")?,
            button_index: row.get("button_index")?,
            first_flop: row.get("first_flop")?,
            first_turn: row.get("first_turn")?,
            first_river: row.get("first_river")?,
            pot_kind: row.get("pot_kind")?,
            posting: row.get("posting")?,
            straddling: row.get("straddling")?,
            pre_flop_limping: row.get("pre_flop_limping")?,
            pre_flop_cold_calling: row.get("pre_flop_cold_calling")?,
            players_post_flop: row.get("players_post_flop")?,
            players_at_showdown: row.get("players_at_showdown")?,
            single_winner: row.get("single_winner")?,
            final_full_pot_size: row.get("final_full_pot_size")?,
        };
        Ok(hand)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Flop(pub [Card; 3]);

impl FromSql for Flop {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = value.as_str()?;
        if s.len() != 6 {
            return Err(FromSqlError::Other(
                format!("invalid cards '{s}': bad length").into(),
            ));
        }
        if !s.is_ascii() {
            return Err(FromSqlError::Other(
                format!("invalid cards '{s}': not ascii").into(),
            ));
        }

        let mut cards = [Card::MIN; 3];
        let mut cards_set = Cards::EMPTY;
        for i in 0..3 {
            let card_raw = &s[i * 2..i * 2 + 2];
            let card = Card::from_str(card_raw).map_err(|err| FromSqlError::Other(err))?;

            if !cards_set.try_add(card) {
                return Err(FromSqlError::Other(
                    format!("invalid cards '{s}': duplicate card {card}").into(),
                ));
            };
            cards[i] = card;
        }

        Ok(Flop(cards))
    }
}

impl Flop {
    pub fn to_string(self) -> String {
        let mut out = String::with_capacity(6);
        for card in self.0 {
            write!(&mut out, "{card}").unwrap();
        }
        out
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PotKind {
    Walk,
    Limped,
    SRP,
    ThreeBet,
    FourBet,
    FiveBetPlus,
}

impl FromSql for PotKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let s = value.as_str()?;
        Self::from_str(s)
            .ok_or_else(|| FromSqlError::Other(format!("unknown pot kind '{s}'").into()))
    }
}

impl PotKind {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "walk" => Some(PotKind::Walk),
            "limped" => Some(PotKind::Limped),
            "srp" => Some(PotKind::SRP),
            "3-bet" => Some(PotKind::ThreeBet),
            "4-bet" => Some(PotKind::FourBet),
            "5-bet+" => Some(PotKind::FiveBetPlus),
            _ => None,
        }
    }

    fn to_str(self) -> &'static str {
        match self {
            PotKind::Walk => "walk",
            PotKind::Limped => "limped",
            PotKind::SRP => "srp",
            PotKind::ThreeBet => "3-bet",
            PotKind::FourBet => "4-bet",
            PotKind::FiveBetPlus => "5-bet+",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HandPlayer {
    pub hand_id: Option<u64>,
    pub player: u8,

    pub player_name: Option<Arc<String>>,
    pub seat: Option<u8>,
    pub hand: Option<hand::Hand>,
    pub went_to_showdown: bool,

    pub starting_stack: u32,
    pub pot_contribution: u32,
    pub showdown_stack: u32,

    pub pre_flop_action: Actions,
    pub flop_action: Actions,
    pub turn_action: Actions,
    pub river_action: Actions,
}

impl HandPlayer {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Option<Self>> {
        let hand_id_and_player: rusqlite::Result<(Option<u64>, u8)> = row
            .get("hand_id")
            .and_then(|hand_id| Ok((hand_id, row.get("player")?)));

        let (hand_id, player) = match hand_id_and_player {
            Ok((Some(hand_id), player)) => (hand_id, player),
            Ok((None, _)) => return Ok(None),
            Err(rusqlite::Error::InvalidColumnName(_)) => return Ok(None),
            Err(rusqlite::Error::InvalidColumnType(_, _, Type::Null)) => return Ok(None),
            Err(err) => return Err(err),
        };

        let hand_player = HandPlayer {
            hand_id: Some(hand_id),
            player,
            player_name: get_string(row, "player_name")?,
            seat: row.get("seat")?,
            hand: row.get("hand")?,
            went_to_showdown: row.get("went_to_showdown")?,
            starting_stack: row.get("starting_stack")?,
            pot_contribution: row.get("pot_contribution")?,
            showdown_stack: row.get("showdown_stack")?,
            pre_flop_action: row.get("pre_flop_action")?,
            flop_action: row.get("flop_action")?,
            turn_action: row.get("turn_action")?,
            river_action: row.get("river_action")?,
        };

        Ok(Some(hand_player))
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum Action {
    Post = b'p',
    Straddle = b's',
    Fold = b'f',
    Check = b'x',
    Call = b'c',
    Bet = b'b',
    Raise = b'r',
}

impl Action {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'p' => Some(Action::Post),
            b's' => Some(Action::Straddle),
            b'f' => Some(Action::Fold),
            b'x' => Some(Action::Check),
            b'c' => Some(Action::Call),
            b'b' => Some(Action::Bet),
            b'r' => Some(Action::Raise),
            _ => None,
        }
    }

    fn from_game(action: crate::game::Action) -> Option<Self> {
        use crate::game::Action::*;

        match action {
            Post { .. } => Some(Action::Post),
            Straddle { .. } => Some(Action::Straddle),
            Fold(_) => Some(Action::Fold),
            Check(_) => Some(Action::Check),
            Call { .. } => Some(Action::Call),
            Bet { .. } => Some(Action::Bet),
            Raise { .. } => Some(Action::Raise),
            _ => None,
        }
    }

    fn to_char(self) -> char {
        self as u8 as char
    }
}

#[derive(Debug, Clone)]
pub struct Actions(pub Vec<Action>);

impl FromSql for Actions {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let actions = value
            .as_str_or_null()?
            .map(|s| Self::from_str(s).map_err(|err| FromSqlError::Other(err)))
            .transpose()?
            .unwrap_or_else(|| Self::empty());
        Ok(actions)
    }
}

impl Actions {
    fn empty() -> Self {
        Self(Vec::new())
    }

    fn from_str(s: &str) -> Result<Self> {
        let actions = s
            .bytes()
            .map(|b| {
                Action::from_byte(b).ok_or_else(|| format!("db: invalid actions '{s}'").into())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(actions))
    }

    pub fn to_string(&self) -> Option<String> {
        if self.0.is_empty() {
            None
        } else {
            Some(self.0.iter().map(|action| action.to_char()).collect())
        }
    }
}

pub struct HandData {
    pub id: Option<u64>,
    pub data: GameData,
}

pub struct HandBundle {
    data: HandData,
    hand: Hand,
    players: Vec<HandPlayer>,
}

impl HandBundle {
    fn from_game(game: &Game) -> Self {
        assert_eq!(game.state(), State::End);

        let game_data = game.to_game_data();
        let hand = Hand::from_game(game, &game_data);

        let players: Vec<_> = game_data
            .players
            .iter()
            .enumerate()
            .map(|(index, player)| HandPlayer {
                hand_id: None,
                player: u8::try_from(index).unwrap(),
                player_name: player.name.clone(),
                seat: player.seat,
                hand: player.hand,
                went_to_showdown: hand.players_at_showdown.is_some() && !game.folded(index),
                starting_stack: player.starting_stack,
                pot_contribution: game.total_invested(index),
                showdown_stack: game.current_stacks()[index],
                pre_flop_action: Actions::empty(),
                flop_action: Actions::empty(),
                turn_action: Actions::empty(),
                river_action: Actions::empty(),
            })
            .collect();

        let data = HandData {
            id: None,
            data: game_data,
        };

        let mut hand_bundle = Self {
            data,
            hand,
            players,
        };
        hand_bundle.fill_player_actions();
        hand_bundle
    }

    fn fill_player_actions(&mut self) {
        let mut street = Street::PreFlop;
        for game_action in self.data.data.actions.iter().copied() {
            if let Some(next_street) = game_action.street() {
                street = next_street;
            } else if let Some(action) = Action::from_game(game_action) {
                let player = &mut self.players[game_action.player().unwrap()];
                let actions = match street {
                    Street::PreFlop => &mut player.pre_flop_action,
                    Street::Flop => &mut player.flop_action,
                    Street::Turn => &mut player.turn_action,
                    Street::River => &mut player.river_action,
                };
                actions.0.push(action);
            }
        }
    }
}
