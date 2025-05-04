use std::{iter::Peekable, str::FromStr, sync::Arc};

use chrono::NaiveDateTime;
use regex::Regex;

use crate::{
    bitset::Bitset,
    card::Card,
    game::{Board, Game, Player, State, Street},
    hand::Hand,
    result::Result,
};

fn option_to_result<T>(v: Option<T>, message: &str) -> Result<T> {
    v.ok_or_else(|| message.into())
}

// TODO: Deduplicate String's.

pub struct GGHandHistoryParser {
    re_description: Regex,
    re_table_info: Regex,
    re_seat_config: Regex,
    re_post_blind: Regex,
    re_straddle: Regex,
    re_deal: Regex,
    re_action: Regex,
    re_flop: Regex,
    re_turn: Regex,
    re_river: Regex,
    re_uncalled_bet: Regex,
    re_shows: Regex,
    re_showdown_title: Regex,
    re_showdown: Regex,
    re_summary: Regex,
    re_summary_seat: Regex,
    re_all_in_insurance: Regex,

    sloppy_winnings_check: bool,
}

impl GGHandHistoryParser {
    pub fn new(sloppy_winnings_check: bool) -> Self {
        const REGEX_PRICE: &'static str = r"\$(\d+(?:\.\d{1, 2})?)";
        const REGEX_CARD: &'static str = r"([2-9TJQKA][dshc])";
        // Greedily match all characters. Should work,
        // because the other regexes are specific enough.
        const REGEX_NAME: &'static str = r"(.+)";

        let re_description = r"^Poker Hand (#[^:]+): Hold'em No Limit *".to_owned()
            + &format!(r"\({REGEX_PRICE}/{REGEX_PRICE}\) - ")
            + r"(\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2})$";
        const RE_TABLE_INFO: &'static str =
            r"^Table '([a-zA-z0-9]+)' 6-max Seat #([1-6]) is the button$";
        let re_seat_config = format!(r"^Seat ([1-6]): {REGEX_NAME} \({REGEX_PRICE} in chips\)$");
        let re_post_blind = format!(r"^{REGEX_NAME}: posts ([a-z]+) blind {REGEX_PRICE}$");
        let re_straddle = format!(r"^{REGEX_NAME}: straddle {REGEX_PRICE}$");
        let re_deal = format!(r"^Dealt to {REGEX_NAME} (?:\[{REGEX_CARD} {REGEX_CARD}\])?$");
        let re_action = format!(r"^{REGEX_NAME}: ((folds)|(checks)")
            + &format!(r"|(calls {REGEX_PRICE}( and is all-in)?)")
            + &format!(r"|(bets {REGEX_PRICE}( and is all-in)?)")
            + &format!(r"|(raises {REGEX_PRICE} to {REGEX_PRICE}( and is all-in)?))$");
        let re_flop = format!(r"^\*\*\* .*FLOP \*\*\* \[{REGEX_CARD} {REGEX_CARD} {REGEX_CARD}\]$");
        let re_turn = format!(
            r"^\*\*\* .*TURN \*\*\* \[{REGEX_CARD} {REGEX_CARD} {REGEX_CARD}\] \[{REGEX_CARD}\]$"
        );
        let re_river = format!(
            r"^\*\*\* .*RIVER \*\*\* \[{REGEX_CARD} {REGEX_CARD} {REGEX_CARD} {REGEX_CARD}\] \[{REGEX_CARD}\]$"
        );
        let re_uncalled_bet = format!(r"^Uncalled bet \({REGEX_PRICE}\) returned to {REGEX_NAME}$");
        let re_shows = format!(r"^{REGEX_NAME}: shows \[{REGEX_CARD} {REGEX_CARD}\].*$");
        let re_showdown_title = format!(r"^\*\*\* .*SHOWDOWN \*\*\*$");
        let re_showdown = format!(r"^{REGEX_NAME} collected {REGEX_PRICE} from pot$");
        let re_summary = format!(r"^Total pot {REGEX_PRICE} \| Rake {REGEX_PRICE}")
            + &format!(r"( \| Jackpot {REGEX_PRICE})?( \| Bingo {REGEX_PRICE})?")
            + &format!(r"( \| Fortune {REGEX_PRICE})?( \| Tax {REGEX_PRICE})?$");
        const RE_SUMMARY_SEAT: &'static str = r"^Seat ([1-6]): .*$";
        let re_all_in_insurance = format!("^{REGEX_NAME}: ((get an all-in insurance ")
            + &format!(r"\(premium \({REGEX_PRICE}/{REGEX_PRICE}/{REGEX_PRICE}\) for ")
            + &format!(
                r"\({REGEX_PRICE}/{REGEX_PRICE}/{REGEX_PRICE}\) - \(Mandatory/Main/Sub\)\))"
            )
            + &format!(r"|(pay premium of all-in insurance \({REGEX_PRICE}\))")
            + &format!(r"|(get main compensation of all-in insurance \({REGEX_PRICE}\)))$");

        Self {
            re_description: Regex::new(&re_description).unwrap(),
            re_table_info: Regex::new(RE_TABLE_INFO).unwrap(),
            re_seat_config: Regex::new(&re_seat_config).unwrap(),
            re_post_blind: Regex::new(&re_post_blind).unwrap(),
            re_straddle: Regex::new(&re_straddle).unwrap(),
            re_deal: Regex::new(&re_deal).unwrap(),
            re_action: Regex::new(&re_action).unwrap(),
            re_flop: Regex::new(&re_flop).unwrap(),
            re_turn: Regex::new(&re_turn).unwrap(),
            re_river: Regex::new(&re_river).unwrap(),
            re_uncalled_bet: Regex::new(&re_uncalled_bet).unwrap(),
            re_shows: Regex::new(&re_shows).unwrap(),
            re_showdown_title: Regex::new(&re_showdown_title).unwrap(),
            re_showdown: Regex::new(&re_showdown).unwrap(),
            re_summary: Regex::new(&re_summary).unwrap(),
            re_summary_seat: Regex::new(RE_SUMMARY_SEAT).unwrap(),
            re_all_in_insurance: Regex::new(&re_all_in_insurance).unwrap(),
            sloppy_winnings_check,
        }
    }

    pub fn parse_str_full(&self, entries: &str) -> Vec<Result<Game>> {
        // TODO: Rewrite as Iterator.

        let mut lines_unix = entries.split('\n');
        let mut last_empty_index = 0;
        let mut index = 0;
        let mut games = Vec::new();

        while let Some(line) = lines_unix.next() {
            let next_index = index + line.len() + 1;
            if line.is_empty() {
                let entry = entries[last_empty_index..index].trim();
                if entry.len() != 0 {
                    games.push(self.parse_str_single(entry));
                }
                last_empty_index = next_index;
            }
            index = next_index;
        }

        if last_empty_index != index {
            let entry = entries[last_empty_index..].trim();
            if entry.len() != 0 {
                games.push(self.parse_str_single(entry));
            }
        }

        games
    }

    pub fn parse_str(&self, entries: &str) -> Result<Vec<Game>> {
        let mut lines_unix = entries.split('\n');
        let mut last_empty_index = 0;
        let mut index = 0;
        let mut games = Vec::new();

        while let Some(line) = lines_unix.next() {
            let next_index = index + line.len() + 1;
            if line.is_empty() {
                let entry = entries[last_empty_index..index].trim();
                if entry.len() != 0 {
                    let game = self.parse_str_single(entry)?;
                    games.push(game);
                }
                last_empty_index = next_index;
            }
            index = next_index;
        }

        if last_empty_index != index {
            let entry = entries[last_empty_index..].trim();
            if entry.len() != 0 {
                let game = self.parse_str_single(entry)?;
                games.push(game);
            }
        }

        Ok(games)
    }

    fn parse_str_single(&self, entry: &str) -> Result<Game> {
        self.parse_str_single_inner(entry).map_err(|err| {
            let message =
                format!("Error while parsing GGPoker hand history entry:\n\n{entry}\n\n{err}\n")
                    + "This can be caused by an internal parser error or an invalid hand history.";
            message.into()
        })
    }

    fn parse_str_single_inner(&self, entry: &str) -> Result<Game> {
        let seats = self.parse_summary_seats(entry)?;

        // Currently the all-in insurance info is ignored.
        let mut lines = entry
            .lines()
            .filter(|line| !self.re_all_in_insurance.is_match(line))
            .peekable();

        let (hand_name, small_blind, big_blind, date) = self.parse_description(&mut lines)?;
        let (table_name, button_seat) = self.parse_table_info(&mut lines)?;
        let players = self.parse_stacks(&mut lines, seats)?;
        let Some(button_index) = players
            .iter()
            .position(|player| player.seat == Some(button_seat))
        else {
            return Err("parse: invalid button seat".into());
        };

        let mut game = Game::new(&players, button_index, small_blind, big_blind)?;
        game.set_table_name(table_name);
        game.set_hand_name(hand_name);
        game.set_date(date);

        self.parse_posts(&mut lines, &mut game)?;
        self.parse_straddles(&mut lines, &mut game)?;
        self.parse_hole_cards(&mut lines, &mut game)?;
        self.parse_and_apply_actions(&mut lines, &mut game)?;
        let winnings = self.parse_showdown(&mut lines, &mut game)?;
        self.parse_summary(&mut lines, &mut game, &winnings)?;

        Ok(game)
    }

    fn parse_summary_seats(&self, entry: &str) -> Result<Bitset<2>> {
        let mut seats = Bitset::<2>::EMPTY;

        for line in entry.lines().rev() {
            let Some(captures) = self.re_summary_seat.captures(line) else {
                break;
            };
            let [seat_one_based] = captures.extract().1;

            let seat = seat_one_based
                .parse::<u8>()
                .unwrap()
                .checked_sub(1)
                .unwrap();
            seats.set(usize::from(seat));
        }

        if seats.count() < 2 {
            return Err("parse seat summary: less than two seats configured".into());
        }
        Ok(seats)
    }

    fn parse_description<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(Arc<String>, u32, u32, NaiveDateTime)> {
        let description = option_to_result(lines.next(), "first line (description) missing")?;
        let [hand_name, small_blind, big_blind, date] = option_to_result(
            self.re_description.captures(description),
            "description: invalid format",
        )?
        .extract()
        .1;

        let hand_name = Arc::new(hand_name.to_owned());
        let small_blind = Self::parse_price_as_cent(small_blind)?;
        let big_blind = Self::parse_price_as_cent(big_blind)?;
        let date = NaiveDateTime::parse_from_str(date, "%Y/%m/%d %H:%M:%S")?;
        Ok((hand_name, small_blind, big_blind, date))
    }

    fn parse_table_info<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(Arc<String>, u8)> {
        let table_info = option_to_result(lines.next(), "second line (table info) missing")?;
        let [table_name, button_seat_one_based] = option_to_result(
            self.re_table_info.captures(table_info),
            "table info: invalid format",
        )?
        .extract()
        .1;

        let table_name = Arc::new(table_name.to_owned());
        let button_seat_one_based = button_seat_one_based.parse::<u8>()?;
        let Some(button_seat) = button_seat_one_based.checked_sub(1) else {
            return Err("table info: invalid button seat".into());
        };
        Ok((table_name, button_seat))
    }

    fn parse_stacks<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        seats: Bitset<2>,
    ) -> Result<Vec<Player>> {
        let mut players = Vec::new();
        loop {
            let seat_config = option_to_result(lines.peek(), "seat config line missing")?;
            let Some(seat_config) = self.re_seat_config.captures(seat_config) else {
                break;
            };
            lines.next().unwrap();

            let [seat_one_based, name, stack] = seat_config.extract().1;
            let seat = seat_one_based
                .parse::<u8>()
                .unwrap()
                .checked_sub(1)
                .unwrap();

            if !seats.has(usize::from(seat)) {
                continue;
            }

            players.push(Player {
                name: Some(Arc::new(name.to_owned())),
                seat: Some(seat),
                hand: None,
                starting_stack: Self::parse_price_as_cent(stack)?,
            });
        }

        Ok(players)
    }

    fn parse_posts<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
    ) -> Result<()> {
        let small_blind_name = game.player_name(game.small_blind_index()).as_ref();
        let big_blind_name = game.player_name(game.big_blind_index()).as_ref();
        self.validate_post(lines, Some(small_blind_name), "small", game.small_blind())?;
        self.validate_post(lines, Some(big_blind_name), "big", game.big_blind())?;
        game.post_small_and_big_blind()?;

        let mut additional_posters = Bitset::<2>::EMPTY;
        while lines
            .peek()
            .is_some_and(|line| self.re_post_blind.is_match(line))
        {
            let name = self.validate_post(lines, None, "big", game.big_blind())?;
            let Some(player) = game.player_by_name(name) else {
                return Err(format!("post: invalid player name '{name}'").into());
            };

            if additional_posters.has(player) {
                return Err(format!("post: additional post for '{name}' already made").into());
            }
            additional_posters.set(player);
        }

        let players =
            (game.small_blind_index()..game.player_count()).chain(0..game.small_blind_index());
        for player in players {
            if additional_posters.has(player) {
                game.additional_post(player)?;
            }
        }

        Ok(())
    }

    fn parse_straddles<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
    ) -> Result<()> {
        while lines
            .peek()
            .is_some_and(|line| self.re_straddle.is_match(line))
        {
            let [name, price] = self
                .re_straddle
                .captures(lines.next().unwrap())
                .unwrap()
                .extract()
                .1;
            let Some(player) = game.player_by_name(name) else {
                return Err("straddle: invalid player name".into());
            };
            let amount = Self::parse_price_as_cent(price)?;
            game.straddle(player, amount)?;
        }

        Ok(())
    }

    fn validate_post<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        expected_name: Option<&str>,
        expected_kind: &str,
        expected_price: u32,
    ) -> Result<&'a str> {
        let post_blind = option_to_result(lines.next(), "post blind line is missing")?;
        let [name, kind, price] = option_to_result(
            self.re_post_blind.captures(post_blind),
            "post blind: invalid format",
        )?
        .extract()
        .1;
        let price = Self::parse_price_as_cent(price)?;
        if expected_name.is_some_and(|n| name != n)
            || kind != expected_kind
            || price != expected_price
        {
            Err("post blind: invalid format".into())
        } else {
            Ok(name)
        }
    }

    fn parse_hole_cards<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
    ) -> Result<()> {
        let has_hole_cards_title = lines
            .next()
            .is_some_and(|title| title == "*** HOLE CARDS ***");
        if !has_hole_cards_title {
            return Err("hole cards title line is missing".into());
        }

        if lines
            .peek()
            .is_some_and(|deal| !self.re_deal.is_match(deal))
        {
            return Ok(());
        }

        for player in 0..game.player_count() {
            let deal = option_to_result(lines.next(), "deal line is missing")?;
            let deal = option_to_result(self.re_deal.captures(deal), "deal: invalid format")?;
            let name = &deal[1];
            let expected_name = game.player_name(player);
            if name != expected_name {
                return Err(format!("deal: unexpected name {name}").into());
            }
            if let (Some(card_a), Some(card_b)) = (deal.get(2), deal.get(3)) {
                let card_a = Card::from_str(card_a.as_str())?;
                let card_b = Card::from_str(card_b.as_str())?;
                let Some(hand) = Hand::of_two_cards(card_a, card_b) else {
                    return Err("deal: invalid hand".into());
                };
                game.set_hand(player, hand)?;
            }
        }

        Ok(())
    }

    fn parse_and_apply_actions<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
    ) -> Result<()> {
        loop {
            game.internal_asserts_state();

            match game.state() {
                State::Post | State::End => unreachable!(),
                State::Player(_) => self.parse_and_apply_player_action(lines, game)?,
                State::Street(_) => {
                    if !self.parse_and_apply_street_action(lines, game)? {
                        return Err("street: line missing or invalid format".into());
                    }
                }
                State::UncalledBet { .. } => self.parse_uncalled_bet(lines, game)?,
                State::ShowOrMuck(_) => self.parse_shows(lines, game)?,
                State::Showdown => {
                    if !self.parse_and_apply_street_action(lines, game)? {
                        break Ok(());
                    }
                }
            }
        }
    }

    fn parse_and_apply_player_action<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
    ) -> Result<()> {
        let action = option_to_result(lines.next(), "action line is missing")?;
        let Some(action) = self.re_action.captures(action) else {
            return Err("action: invalid format".into());
        };
        let name = &action[1];
        let player_index = game.player_by_name(name);
        if player_index.is_none() || player_index != game.current_player() {
            return Err(format!(
                "action: player {name} is not expected index {:?}",
                game.current_player()
            )
            .into());
        }
        let player_index = player_index.unwrap();

        const FOLD_INDEX: usize = 3;
        const CHECK_INDEX: usize = 4;
        const CALL_INDEX: usize = 5;
        const CALL_ALL_IN_INDEX: usize = 7;
        const BET_INDEX: usize = 8;
        const BET_ALL_IN_INDEX: usize = 10;
        const RAISE_INDEX: usize = 11;
        const RAISE_ALL_IN_INDEX: usize = 14;

        game.internal_asserts_state();
        if action.get(FOLD_INDEX).is_some() {
            game.fold()?;
        } else if action.get(CHECK_INDEX).is_some() {
            game.check()?;
        } else if action.get(CALL_INDEX).is_some() {
            let call_amount = Self::parse_price_as_cent(&action[CALL_INDEX + 1])?;
            if game
                .can_call()
                .is_some_and(|expected_amount| expected_amount != call_amount)
            {
                return Err(format!(
                    "action: player {name} call amount {call_amount} not equals expected amount",
                )
                .into());
            }
            game.call()?;
            if action.get(CALL_ALL_IN_INDEX).is_some() && game.current_stacks()[player_index] != 0 {
                return Err("action: invalid call all-in".into());
            }
        } else if action.get(BET_INDEX).is_some() {
            let bet_amount = Self::parse_price_as_cent(&action[BET_INDEX + 1])?;
            game.bet(bet_amount)?;
            if action.get(BET_ALL_IN_INDEX).is_some() && game.current_stacks()[player_index] != 0 {
                return Err("action: invalid bet all-in".into());
            }
        } else if action.get(RAISE_INDEX).is_some() {
            let raise_amount = Self::parse_price_as_cent(&action[RAISE_INDEX + 1])?;
            let raise_to = Self::parse_price_as_cent(&action[RAISE_INDEX + 2])?;
            let Some((expected_raise_amount, _)) = game.can_raise() else {
                return Err("action: player not allowed to raise".into());
            };
            if raise_amount > raise_to {
                return Err("action: raise amount > to".into());
            }
            if raise_amount < expected_raise_amount {
                return Err("action: raise amount too small".into());
            }

            game.raise(raise_to)?;
            if action.get(RAISE_ALL_IN_INDEX).is_some() && game.current_stacks()[player_index] != 0
            {
                return Err("action: invalid raise all-in".into());
            }
        } else {
            unreachable!()
        }

        Ok(())
    }

    fn parse_street_action<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
    ) -> Result<Option<Board>> {
        let Some(street_line) = lines.peek() else {
            return Ok(None);
        };

        let street_regex = [&self.re_flop, &self.re_turn, &self.re_river];
        let captures = street_regex
            .into_iter()
            .filter_map(|regex| regex.captures(street_line))
            .next();

        let Some(captures) = captures else {
            return Ok(None);
        };
        lines.next().unwrap();

        let cards = captures
            .iter()
            .skip(1)
            .map(|m| Card::from_str(m.unwrap().as_str()))
            .collect::<Result<Vec<_>>>()?;

        let board = Board::from_cards(&cards)?;
        Ok(Some(board))
    }

    fn apply_street_action<'a>(&self, game: &mut Game, board: Board) -> Result<()> {
        let previous_street = board.street().previous().unwrap();
        let known_cards_match = board
            .cards()
            .iter()
            .zip(
                game.board()
                    .cards()
                    .iter()
                    .take(previous_street.community_card_count()),
            )
            .all(|(a, b)| *a == *b);
        if !known_cards_match {
            return Err("street: known cards don't match".into());
        }

        let cards = board.cards();
        match board.street() {
            Street::PreFlop => unreachable!(),
            Street::Flop => game.flop([cards[0], cards[1], cards[2]])?,
            Street::Turn => game.turn(cards[3])?,
            Street::River => game.river(cards[4])?,
        }

        Ok(())
    }

    fn parse_and_apply_street_action<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
    ) -> Result<bool> {
        let Some(board) = self.parse_street_action(lines)? else {
            return Ok(false);
        };
        self.apply_street_action(game, board)?;
        Ok(true)
    }

    fn parse_uncalled_bet<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
    ) -> Result<()> {
        let State::UncalledBet {
            player: expected_player,
            amount: expected_amount,
        } = game.state()
        else {
            unreachable!();
        };

        let uncalled = option_to_result(lines.next(), "uncalled bet line is missing")?;
        let Some(uncalled) = self.re_uncalled_bet.captures(uncalled) else {
            return Err("uncalled bet: invalid format".into());
        };
        let [amount, name] = uncalled.extract().1;
        let amount = Self::parse_price_as_cent(amount)?;
        let player = game.player_by_name(name);
        let Some(player) = player else {
            return Err(format!("uncalled bet: unknown name {name}").into());
        };
        if player != expected_player || amount != expected_amount {
            return Err(
                format!("uncalled bet: bad player index {player} or amount {amount}").into(),
            );
        }
        game.uncalled_bet()
    }

    fn parse_shows<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
    ) -> Result<()> {
        let players_in_hand = game.players_in_hand().count();
        if players_in_hand == 1 {
            return Ok(());
        }

        let mut extra_streets = Vec::new();
        while let Some(board) = self.parse_street_action(lines)? {
            extra_streets.push(board);
        }

        if lines
            .peek()
            .is_some_and(|line| self.re_showdown_title.is_match(line))
        {
            lines.next().unwrap();
        }

        let mut show_players = Bitset::<2>::EMPTY;
        for _ in 0..players_in_hand {
            let shows = option_to_result(lines.next(), "shows line is missing")?;
            let Some(shows) = self.re_shows.captures(shows) else {
                return Err("shows: invalid format".into());
            };
            let [name, card_a, card_b] = shows.extract().1;

            let player = game.player_by_name(name);
            let Some(player) = player else {
                return Err(format!("shows: unknown name {name}").into());
            };

            let Some(hand) = Hand::of_two_cards(Card::from_str(card_a)?, Card::from_str(card_b)?)
            else {
                return Err("shows: invalid hand".into());
            };

            if show_players.has(player) {
                return Err("shows: duplicate player show".into());
            }
            show_players.set(player);
            game.set_hand(player, hand)?;
        }

        while let State::ShowOrMuck(player) = game.state() {
            if !show_players.has(player) {
                return Err("shows: show not allowed for player".into());
            }
            game.show_hand()?;
            show_players.remove(player);
        }

        if !show_players.is_empty() {
            return Err("shows: additional show(s) for player(s)".into());
        }

        for board in extra_streets {
            self.apply_street_action(game, board)?;
        }

        Ok(())
    }

    fn parse_showdown<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &Game,
    ) -> Result<Vec<u32>> {
        let mut winnings = vec![0u32; game.player_count()];

        loop {
            if lines
                .peek()
                .is_some_and(|line| self.re_showdown_title.is_match(line))
            {
                lines.next().unwrap();
            }

            self.parse_showdown_single(lines, game, &mut winnings)?;

            let Some(next_line) = lines.peek() else {
                return Err("showdown: unexpected end of input".into());
            };
            if *next_line == "*** SUMMARY ***" {
                break;
            }
        }

        Ok(winnings)
    }

    fn parse_showdown_single<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &Game,
        winnings: &mut [u32],
    ) -> Result<()> {
        while lines
            .peek()
            .is_some_and(|line| self.re_shows.is_match(line))
        {
            lines.next().unwrap();
        }

        loop {
            if lines.peek().is_some_and(|line| line.starts_with("***")) {
                break Ok(());
            }

            let showdown = option_to_result(lines.next(), "showdown line is missing")?;
            let Some(showdown) = self.re_showdown.captures(showdown) else {
                return Err("showdown: invalid format".into());
            };
            let [name, amount_won] = showdown.extract().1;

            let player = game.player_by_name(name);
            let Some(player) = player else {
                return Err(format!("showdown: unknown player name {name}").into());
            };

            let amount_won = Self::parse_price_as_cent(amount_won)?;
            let Some(new_winnings) = winnings[player].checked_add(amount_won) else {
                return Err("showdown: overflow calculating winnings".into());
            };
            winnings[player] = new_winnings;
        }
    }

    fn parse_summary<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
        winnings: &[u32],
    ) -> Result<()> {
        let has_header_line = lines.next().is_some_and(|line| line == "*** SUMMARY ***");
        if !has_header_line {
            return Err("summary: missing header line".into());
        }
        let summary = option_to_result(lines.next(), "summary line is missing")?;
        let Some(summary) = self.re_summary.captures(summary) else {
            return Err("summary: invalid format".into());
        };

        const TOTAL_INDEX: usize = 1;
        const RAKE_INDEX: usize = 2;
        const JACKPOT_INDEX: usize = 4;
        const BINGO_INDEX: usize = 6;
        const FORTUNE_INDEX: usize = 8;
        const TAX_INDEX: usize = 10;

        let total = Self::parse_price_as_cent(summary.get(TOTAL_INDEX).unwrap().as_str())?;
        let rake = Self::parse_price_as_cent(summary.get(RAKE_INDEX).unwrap().as_str())?;
        let jackpot = summary
            .get(JACKPOT_INDEX)
            .map(|jackpot| Self::parse_price_as_cent(jackpot.as_str()))
            .unwrap_or(Ok(0))?;
        for index in [BINGO_INDEX, FORTUNE_INDEX, TAX_INDEX] {
            let amount = summary
                .get(index)
                .map(|amount| Self::parse_price_as_cent(amount.as_str()))
                .unwrap_or(Ok(0))?;
            if amount != 0 {
                return Err("summary: bingo, fortune or tax is not zero".into());
            }
        }

        if game.total_pot() != total {
            return Err("summary: bad total pot".into());
        }
        let Some(mut total_rake) = rake.checked_add(jackpot) else {
            return Err("summary: overflow calculating total rake".into());
        };
        let Some(pot_without_total_rake) = total.checked_sub(total_rake) else {
            return Err("summary: total rake is larger than the total pot".into());
        };

        let total_winnings = winnings
            .iter()
            .fold(Some(0u32), |acc, n| acc.and_then(|acc| acc.checked_add(*n)));
        let Some(total_winnings) = total_winnings else {
            return Err("summary: overflow calculating total winnings".into());
        };
        if total_winnings > total {
            return Err("summary: total winnings are larger than total pot".into());
        }

        if self.sloppy_winnings_check && total_winnings < pot_without_total_rake {
            total_rake = total.checked_sub(total_winnings).unwrap();
        }

        let player_pot_share = winnings
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, winning)| *winning != 0);
        game.showdown_custom(total_rake, player_pot_share)?;

        // The rest of the summary is currently ignored.
        // Could be used for more correctness checks.
        Ok(())
    }

    fn parse_price_as_cent(price: &str) -> Result<u32> {
        let mut split = price.split('.');
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
        dollar
            .checked_mul(100)
            .and_then(|n| n.checked_add(cent))
            .ok_or_else(|| format!("price {price} too large").into())
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;

    #[test]
    fn parse_example_gg_hand_history() {
        unsafe {
            crate::init::init();
        }

        let path = Path::new("src")
            .join("test_data")
            .join("gg_hands_example.txt");
        let history = fs::read_to_string(path).unwrap();
        let games = GGHandHistoryParser::new(false).parse_str(&history).unwrap();
        for game in games {
            game.internal_asserts_full();
        }
    }
}
