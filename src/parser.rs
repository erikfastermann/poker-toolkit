use std::{collections::HashSet, iter::Peekable};

use regex::Regex;

use crate::{
    card::Card,
    game::{Game, State, Street},
    hand::Hand,
    result::Result,
};

fn option_to_result<T>(v: Option<T>, message: &str) -> Result<T> {
    v.ok_or_else(|| message.into())
}

pub struct GGHandHistoryParser {
    re_description: Regex,
    re_table_info: Regex,
    re_seat_config: Regex,
    re_post_blind: Regex,
    re_deal: Regex,
    re_action: Regex,
    re_flop: Regex,
    re_turn: Regex,
    re_river: Regex,
    re_uncalled_bet: Regex,
    re_shows: Regex,
    re_showdown: Regex,
    re_summary: Regex,
}

impl GGHandHistoryParser {
    const HERO: &'static str = "Hero";

    pub fn new() -> Self {
        const REGEX_PRICE: &'static str = r"\$(\d+(?:\.\d{1, 2})?)";
        const REGEX_CARD: &'static str = r"([2-9TJQKA][dshc])";
        const REGEX_NAME: &'static str = "([a-zA-Z0-9]+)";

        let re_description = r"^Poker Hand #[^:]+: Hold'em No Limit ".to_string()
            + &format!(r"\({REGEX_PRICE}/{REGEX_PRICE}\) - ")
            + r"(\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2})$";
        const RE_TABLE_INFO: &'static str =
            r"^Table '([a-zA-z0-9]+)' 6-max Seat #([1-6]) is the button$";
        let re_seat_config = format!(r"^Seat ([1-6]): {REGEX_NAME} \({REGEX_PRICE} in chips\)$");
        let re_post_blind = format!(r"^{REGEX_NAME}: posts ([a-z]+) blind {REGEX_PRICE}$");
        let re_deal = format!(r"^Dealt to {REGEX_NAME} (?:\[{REGEX_CARD} {REGEX_CARD}\])?$");
        let re_action = format!(r"^{REGEX_NAME}: ((folds)|(checks)")
            + &format!(r"|(calls {REGEX_PRICE}( and is all-in)?)")
            + &format!(r"|(bets {REGEX_PRICE}( and is all-in)?)")
            + &format!(r"|(raises {REGEX_PRICE} to {REGEX_PRICE}( and is all-in)?))$");
        let re_flop = format!(r"^\*\*\* FLOP \*\*\* \[{REGEX_CARD} {REGEX_CARD} {REGEX_CARD}\]$");
        let re_turn = format!(
            r"^\*\*\* TURN \*\*\* \[{REGEX_CARD} {REGEX_CARD} {REGEX_CARD}\] \[{REGEX_CARD}\]$"
        );
        let re_river = format!(
            r"^\*\*\* RIVER \*\*\* \[{REGEX_CARD} {REGEX_CARD} {REGEX_CARD} {REGEX_CARD}\] \[{REGEX_CARD}\]$"
        );
        let re_uncalled_bet = format!(r"^Uncalled bet \({REGEX_PRICE}\) returned to {REGEX_NAME}$");
        let re_shows = format!(r"^{REGEX_NAME}: shows \[{REGEX_CARD} {REGEX_CARD}\] .*$");
        let re_showdown = format!(r"^{REGEX_NAME} collected {REGEX_PRICE} from pot$");
        let re_summary = format!(r"^Total pot {REGEX_PRICE} \| Rake {REGEX_PRICE} \| ")
            + &format!(r"Jackpot {REGEX_PRICE} \| Bingo {REGEX_PRICE} \| ")
            + &format!(r"Fortune {REGEX_PRICE} \| Tax {REGEX_PRICE}$");
        Self {
            re_description: Regex::new(&re_description).unwrap(),
            re_table_info: Regex::new(RE_TABLE_INFO).unwrap(),
            re_seat_config: Regex::new(&re_seat_config).unwrap(),
            re_post_blind: Regex::new(&re_post_blind).unwrap(),
            re_deal: Regex::new(&re_deal).unwrap(),
            re_action: Regex::new(&re_action).unwrap(),
            re_flop: Regex::new(&re_flop).unwrap(),
            re_turn: Regex::new(&re_turn).unwrap(),
            re_river: Regex::new(&re_river).unwrap(),
            re_uncalled_bet: Regex::new(&re_uncalled_bet).unwrap(),
            re_shows: Regex::new(&re_shows).unwrap(),
            re_showdown: Regex::new(&re_showdown).unwrap(),
            re_summary: Regex::new(&re_summary).unwrap(),
        }
    }

    pub fn parse_str(&self, entries: &str) -> Result<Vec<Game>> {
        let mut lines_unix = entries.split('\n');
        let mut last_empty_index = 0;
        let mut index = 0;
        let mut games = Vec::new();
        while let Some(line) = lines_unix.next() {
            let next_index = index + line.len() + 1;
            if line.is_empty() {
                let entry = &entries[last_empty_index..index];
                if entry.trim().len() != 0 {
                    println!("{entry}"); // TODO
                    let game = self.parse_str_single(entry)?;
                    games.push(game);
                }
                last_empty_index = next_index;
            }
            index = next_index;
        }
        if last_empty_index != index {
            Err("parser: does not end with empty line".into())
        } else {
            Ok(games)
        }
    }

    fn parse_str_single(&self, entry: &str) -> Result<Game> {
        let entry = entry.trim();
        let mut lines = entry.lines().peekable();
        let (small_blind, big_blind) = self.parse_description(&mut lines)?;
        let button_index = self.parse_table_info(&mut lines)?;
        let (stacks, names, hero_index) = self.parse_stacks(&mut lines)?;
        // TODO: Use names of Game.
        let mut game = Game::new(
            stacks,
            Some(names.clone()),
            button_index,
            small_blind,
            big_blind,
        )?;
        self.validate_posts(&mut lines, button_index, &names, small_blind, big_blind)?;
        game.post_small_and_big_blind()?;
        let hero_hand = self.parse_hole_cards(&mut lines, &names)?;
        game.set_hand(hero_index, hero_hand)?;
        self.parse_and_apply_actions(&mut lines, &mut game, &names)?;
        let winnings = self.parse_showdown(&mut lines, &mut game, &names)?;
        self.parse_summary(&mut lines, &mut game, &winnings)?;
        game.internal_asserts_history();
        Ok(game)
    }

    fn parse_description<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
    ) -> Result<(u32, u32)> {
        let description = option_to_result(lines.next(), "first line (description) missing")?;
        let [small_blind, big_blind, _] = option_to_result(
            self.re_description.captures(description),
            "description: invalid format",
        )?
        .extract()
        .1;
        let small_blind = Self::parse_price_as_cent(small_blind)?;
        let big_blind = Self::parse_price_as_cent(big_blind)?;
        Ok((small_blind, big_blind))
    }

    fn parse_table_info<'a>(&self, lines: &mut impl Iterator<Item = &'a str>) -> Result<usize> {
        let table_info = option_to_result(lines.next(), "second line (table info) missing")?;
        let [_, button_index_one_based] = option_to_result(
            self.re_table_info.captures(table_info),
            "table info: invalid format",
        )?
        .extract()
        .1;
        let index = button_index_one_based.parse::<usize>()? - 1;
        Ok(index)
    }

    fn parse_stacks<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
    ) -> Result<(Vec<u32>, Vec<String>, usize)> {
        let mut stacks = Vec::new();
        let mut names = Vec::new();
        loop {
            let seat_config = option_to_result(lines.peek(), "seat config line missing")?;
            let Some(seat_config) = self.re_seat_config.captures(seat_config) else {
                break;
            };
            let [seat_one_based, name, stack] = seat_config.extract().1;
            if stacks.len().checked_add(1) != Some(seat_one_based.parse::<usize>()?) {
                return Err("seat config: bad seat index".into());
            }
            names.push(name.to_string());
            stacks.push(Self::parse_price_as_cent(stack)?);
            lines.next().unwrap();
        }

        if names.iter().collect::<HashSet<_>>().len() != names.len() {
            return Err("seat config: duplicate name".into());
        }
        let hero_index = names.iter().position(|name| name == Self::HERO);
        if let Some(hero_index) = hero_index {
            Ok((stacks, names, hero_index))
        } else {
            Err("seat config: hero index missing".into())
        }
    }

    fn validate_posts<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        button_index: usize,
        names: &[impl AsRef<str>],
        small_blind: u32,
        big_blind: u32,
    ) -> Result<()> {
        let small_blind_name = names[(button_index + 1) % names.len()].as_ref();
        let big_blind_name = names[(button_index + 2) % names.len()].as_ref();
        self.validate_post(lines, small_blind_name, "small", small_blind)?;
        self.validate_post(lines, big_blind_name, "big", big_blind)
    }

    fn validate_post<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        expected_name: &str,
        expected_kind: &str,
        expected_price: u32,
    ) -> Result<()> {
        let post_blind = option_to_result(lines.next(), "post blind line is missing")?;
        let [name, kind, price] = option_to_result(
            self.re_post_blind.captures(post_blind),
            "post blind: invalid format",
        )?
        .extract()
        .1;
        let price = Self::parse_price_as_cent(price)?;
        if name != expected_name || kind != expected_kind || price != expected_price {
            Err("post blind: invalid format".into())
        } else {
            Ok(())
        }
    }

    fn parse_hole_cards<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        names: &[impl AsRef<str>],
    ) -> Result<Hand> {
        let has_hole_cards_title = lines
            .next()
            .is_some_and(|title| title == "*** HOLE CARDS ***");
        if !has_hole_cards_title {
            return Err("hole cards title line is missing".into());
        }

        let mut hero_hand = None;
        for expected_name in names.iter().map(|name| name.as_ref()) {
            let deal = option_to_result(lines.next(), "deal line is missing")?;
            let deal = option_to_result(self.re_deal.captures(deal), "deal: invalid format")?;
            let name = &deal[1];
            if name != expected_name {
                return Err(format!("deal: unexpected name {name}").into());
            }
            if name == Self::HERO {
                if hero_hand.is_some() {
                    return Err("deal: duplicate hero hand".into());
                }
                let (Some(card_a), Some(card_b)) = (deal.get(2), deal.get(3)) else {
                    return Err("deal: hero hand missing".into());
                };
                let card_a = Card::from_str(card_a.as_str())?;
                let card_b = Card::from_str(card_b.as_str())?;
                hero_hand = Hand::of_two_cards(card_a, card_b);
                if hero_hand.is_none() {
                    return Err("deal: hero hand invalid".into());
                }
            }
        }
        if let Some(hero_hand) = hero_hand {
            Ok(hero_hand)
        } else {
            Err("deal: missing hero hand".into())
        }
    }

    fn parse_and_apply_actions<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
        names: &[impl AsRef<str>],
    ) -> Result<()> {
        loop {
            match game.state() {
                State::Post | State::End => unreachable!(),
                State::Player(_) => self.parse_and_apply_player_action(lines, game, names)?,
                State::Street(_) => self.parse_and_apply_street_action(lines, game)?,
                State::UncalledBet(_, _) => self.parse_uncalled_bet(lines, game, names)?,
                State::ShowOrMuck(_) => self.parse_shows(lines, game, names)?,
                State::Showdown => break Ok(()),
            }
        }
    }

    fn parse_and_apply_player_action<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
        names: &[impl AsRef<str>],
    ) -> Result<()> {
        let action = option_to_result(dbg!(lines.next()), "action line is missing")?; // TODO
        let Some(action) = self.re_action.captures(action) else {
            return Err("action: invalid format".into());
        };
        let name = &action[1];
        let player_index = names
            .iter()
            .position(|current_name| current_name.as_ref() == name);
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

        assert!(game.board().street() != Street::PreFlop || game.can_bet().is_none());
        assert!(game.can_next_street().is_none());
        assert_eq!(game.state(), State::Player(player_index));
        assert!(game.has_cards(player_index));
        assert!(game.next_show_or_muck().is_none());
        if action.get(FOLD_INDEX).is_some() {
            let asserts = (game.can_check()
                && game.board().street() == Street::PreFlop
                && player_index == game.big_blind_index()
                && game.can_bet().is_none()
                && game.can_call().is_none())
                || (game.can_check()
                    && game.can_bet().is_some()
                    && game.can_call().is_none()
                    && game.can_raise().is_none())
                || (!game.can_check() && game.can_bet().is_none() && game.can_call().is_some());
            game.fold()?;
            assert!(asserts);
        } else if action.get(CHECK_INDEX).is_some() {
            let asserts = game.can_check()
                && ((player_index == game.big_blind_index()
                    && game.can_bet().is_none()
                    && game.can_call().is_none())
                    || (game.can_bet().is_some()
                        && game.can_call().is_none()
                        && game.can_raise().is_none()));
            game.check()?;
            assert!(asserts);
        } else if action.get(CALL_INDEX).is_some() {
            let asserts =
                game.can_call().is_some() && !game.can_check() && game.can_bet().is_none();
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
            assert!(asserts);
        } else if action.get(BET_INDEX).is_some() {
            let asserts = game.can_bet().is_some()
                && game.can_check()
                && game.can_call().is_none()
                && game.can_raise().is_none();
            let bet_amount = Self::parse_price_as_cent(&action[BET_INDEX + 1])?;
            game.bet(bet_amount)?;
            if action.get(BET_ALL_IN_INDEX).is_some() && game.current_stacks()[player_index] != 0 {
                return Err("action: invalid bet all-in".into());
            }
            assert!(asserts);
        } else if action.get(RAISE_INDEX).is_some() {
            let asserts = game.can_raise().is_some()
                && ((game.board().street() == Street::PreFlop
                    && player_index == game.big_blind_index()
                    && game.can_bet().is_none()
                    && game.can_call().is_none()
                    && game.can_check())
                    || (!game.can_check()
                        && game.can_call().is_some()
                        && game.can_bet().is_none()));
            let raise_to = Self::parse_price_as_cent(&action[RAISE_INDEX + 2])?;
            game.raise(raise_to)?;
            if action.get(RAISE_ALL_IN_INDEX).is_some() && game.current_stacks()[player_index] != 0
            {
                return Err("action: invalid raise all-in".into());
            }
            assert!(asserts);
        } else {
            unreachable!()
        }

        Ok(())
    }

    fn parse_and_apply_street_action<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
    ) -> Result<()> {
        assert!(
            !game.can_check()
                && game.can_call().is_none()
                && game.can_bet().is_none()
                && game.can_raise().is_none()
                && game.can_next_street().is_some()
                && game.current_player().is_none()
                && game.next_show_or_muck().is_none()
                && game.next_show_or_muck().is_none()
        );
        let State::Street(street) = game.state() else {
            unreachable!()
        };
        let regex = match street {
            Street::PreFlop => unreachable!(),
            Street::Flop => &self.re_flop,
            Street::Turn => &self.re_turn,
            Street::River => &self.re_river,
        };
        let street_line = option_to_result(lines.next(), "street line is missing")?;
        let Some(street_captures) = regex.captures(street_line) else {
            return Err("street: invalid format".into());
        };
        let cards = street_captures
            .iter()
            .skip(1)
            .map(|m| Card::from_str(m.unwrap().as_str()))
            .collect::<Result<Vec<_>>>()?;
        let known_cards_match = cards
            .iter()
            .zip(game.board().cards())
            .all(|(a, b)| *a == *b);
        if !known_cards_match {
            return Err("street: known cards don't match".into());
        }
        match street {
            Street::PreFlop => unreachable!(),
            Street::Flop if cards.len() == 3 => game.flop([cards[0], cards[1], cards[2]]),
            Street::Turn if cards.len() == 4 => game.turn(cards[3]),
            Street::River if cards.len() == 5 => game.river(cards[4]),
            _ => Err("street: bad number of cards".into()),
        }
    }

    fn parse_uncalled_bet<'a>(
        &self,
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
        names: &[impl AsRef<str>],
    ) -> Result<()> {
        let State::UncalledBet(expected_player, expected_amount) = game.state() else {
            unreachable!();
        };

        let uncalled = option_to_result(lines.next(), "uncalled bet line is missing")?;
        let Some(uncalled) = self.re_uncalled_bet.captures(uncalled) else {
            return Err("uncalled bet: invalid format".into());
        };
        let [amount, name] = uncalled.extract().1;
        let amount = Self::parse_price_as_cent(amount)?;
        let player = names
            .iter()
            .position(|current_name| current_name.as_ref() == name);
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
        lines: &mut impl Iterator<Item = &'a str>,
        game: &mut Game,
        names: &[impl AsRef<str>],
    ) -> Result<()> {
        let players_in_hand = game.players_in_hand().count();
        if players_in_hand == 1 {
            return Ok(());
        }
        for _ in 0..players_in_hand {
            let shows = option_to_result(lines.next(), "shows line is missing")?;
            let Some(shows) = self.re_shows.captures(shows) else {
                return Err("shows: invalid format".into());
            };
            let [name, card_a, card_b] = shows.extract().1;
            let player = names
                .iter()
                .position(|current_name| current_name.as_ref() == name);
            let Some(player) = player else {
                return Err(format!("shows: unknown name {name}").into());
            };
            let Some(hand) = Hand::of_two_cards(Card::from_str(card_a)?, Card::from_str(card_b)?)
            else {
                return Err("shows: invalid hand".into());
            };
            if game.state() != State::ShowOrMuck(player) {
                return Err("shows: bad state".into());
            }
            game.set_hand(player, hand)?;
            game.show_hand()?;
        }
        Ok(())
    }

    fn parse_showdown<'a>(
        &self,
        lines: &mut Peekable<impl Iterator<Item = &'a str>>,
        game: &mut Game,
        names: &[impl AsRef<str>],
    ) -> Result<Vec<u32>> {
        let has_header_line = lines.next().is_some_and(|line| line == "*** SHOWDOWN ***");
        if !has_header_line {
            return Err("showdown: missing header line".into());
        }
        let mut winnings = vec![0u32; game.player_count()];
        loop {
            if lines.peek().is_some_and(|line| line.starts_with("***")) {
                break Ok(winnings);
            }
            let showdown = option_to_result(lines.next(), "showdown line is missing")?;
            let Some(showdown) = self.re_showdown.captures(showdown) else {
                return Err("showdown: invalid format".into());
            };
            let [name, amount_won] = showdown.extract().1;
            let player = names
                .iter()
                .position(|current_name| current_name.as_ref() == name);
            let Some(player) = player else {
                return Err(format!("showdown: unknown player name {name}").into());
            };
            let amount_won = Self::parse_price_as_cent(amount_won)?;
            winnings[player] = amount_won;
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
        let [total, rake, jackpot, bingo, fortune, tax] = summary.extract().1;
        let (total, rake, jackpot, bingo, fortune, tax) = (
            Self::parse_price_as_cent(total)?,
            Self::parse_price_as_cent(rake)?,
            Self::parse_price_as_cent(jackpot)?,
            Self::parse_price_as_cent(bingo)?,
            Self::parse_price_as_cent(fortune)?,
            Self::parse_price_as_cent(tax)?,
        );
        if game.total_pot() != total {
            return Err("summary: bad total pot".into());
        }
        if bingo != 0 || fortune != 0 || tax != 0 {
            return Err("summary: bingo, fortune or tax is not zero".into());
        }
        let Some(total_rake) = rake.checked_add(jackpot) else {
            return Err("summary: overflow calculating total rake".into());
        };

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
    use std::fs;

    use super::*;

    #[test]
    fn parse_example_gg_hand_history() {
        unsafe {
            crate::init::init();
        }

        // TODO
        let history = fs::read_to_string(r"/home/erik/Downloads/gg_hands.txt").unwrap();
        let games = GGHandHistoryParser::new().parse_str(&history).unwrap();
        dbg!(games);
    }
}
