use std::{
    fmt::Write,
    io::ErrorKind,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use rand::Rng;
use reqwest::Client;
use reqwest::{
    header::{
        HeaderMap, HeaderValue, ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONNECTION, CONTENT_TYPE,
        ORIGIN, REFERER, USER_AGENT,
    },
    StatusCode,
};
use serde::Deserialize;
use serde_json::json;

use poker_core::{
    game::{milli_big_blind_from_f64, Game, MilliBigBlind},
    range::{
        PreFlopAction, PreFlopRangeTable, PreFlopRangeTableWith, RangeAction, RangeConfigEntry,
        RangeEntry,
    },
    result::Result,
};
use tokio::{fs, time::sleep};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    gw_client_id: String,
    bearer_token: String,
    refresh_token: String,
    game_type: String,
    max_players: usize,
    depth: MilliBigBlind,
    min_frequency: f64,
    max_calls: usize,
    out_dir: String,
}

pub struct Crawler {
    gw_client_id: String,
    bearer_token: String,
    refresh_token: String,
    game_type: String,
    max_players: usize,
    depth: MilliBigBlind,
    min_frequency: f64,
    max_calls: usize,
    out_dir: String,
}

impl Crawler {
    pub async fn new(config: Config) -> Result<Self> {
        if config.depth < 1_000 || config.depth % 1000 != 0 {
            return Err("crawler: invalid depth".into());
        }
        if config.max_players < Game::MIN_PLAYERS || config.max_players > Game::MAX_PLAYERS {
            return Err("crawler: invalid max players".into());
        }
        if config.min_frequency.is_nan()
            || config.min_frequency < 0.0001
            || config.min_frequency > 1.0
        {
            return Err("crawler: invalid min frequency".into());
        }

        let crawler = Self {
            gw_client_id: config.gw_client_id,
            bearer_token: config.bearer_token,
            refresh_token: config.refresh_token,
            game_type: config.game_type,
            max_players: config.max_players,
            depth: config.depth,
            min_frequency: config.min_frequency,
            max_calls: config.max_calls,
            out_dir: config.out_dir,
        };
        Ok(crawler)
    }

    async fn read_queue(&self) -> Result<Vec<Vec<PreFlopAction>>> {
        match fs::read_to_string(self.queue_path()).await {
            Ok(queue_raw) => {
                let queue: Vec<Vec<PreFlopAction>> = serde_json::from_str(&queue_raw)?;
                Ok(queue)
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                eprintln!("Creating new queue file...");

                let queue: Vec<Vec<PreFlopAction>> = vec![vec![]];
                self.write_queue(&queue).await?;
                Ok(queue)
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn write_queue(&self, queue: &[Vec<PreFlopAction>]) -> Result<()> {
        fs::write(self.queue_path(), serde_json::to_string_pretty(&queue)?).await?;
        Ok(())
    }

    fn queue_path(&self) -> Box<Path> {
        PathBuf::from(&self.out_dir)
            .join("queue.json")
            .into_boxed_path()
    }

    async fn queue_pop_and_push_next(
        &self,
        pop: &[PreFlopAction],
        mut next: Vec<Vec<PreFlopAction>>,
    ) -> Result<()> {
        let queue = self.read_queue().await?;
        if queue.first().is_none() || queue.first().is_some_and(|popped| popped != pop) {
            return Err("queue: unexpected item at front of the queue".into());
        }

        next.extend(queue.into_iter().skip(1));
        self.write_queue(&next).await?;

        Ok(())
    }

    pub async fn crawl(mut self) -> Result<()> {
        loop {
            let queue = self.read_queue().await?;
            let Some(next_pre_flop_actions) = queue.first() else {
                eprintln!("Queue is empty, nothing to do. Exiting....");
                return Ok(());
            };

            self.get_and_parse_range(next_pre_flop_actions.clone())
                .await?;

            sleep(Duration::from_secs(rand::thread_rng().gen_range(1..=3))).await;
        }
    }

    async fn get_and_parse_range(&mut self, pre_flop_actions: Vec<PreFlopAction>) -> Result<()> {
        eprintln!("Processing {pre_flop_actions:?}...");

        let raw_range = self.get_raw_range(&pre_flop_actions).await?;
        let entry = self.parse_raw_range(pre_flop_actions, raw_range)?;

        self.store_range(entry).await?;
        Ok(())
    }

    async fn get_raw_range(&mut self, pre_flop_actions: &[PreFlopAction]) -> Result<RawRangeData> {
        match self.get_raw_range_inner(pre_flop_actions).await {
            Ok(range) => Ok(range),
            Err(err) => {
                let err: Box<reqwest::Error> = err.downcast()?;
                if err
                    .status()
                    .is_some_and(|status| status == StatusCode::UNAUTHORIZED)
                {
                    self.renew_bearer_token().await?;
                    self.get_raw_range_inner(pre_flop_actions).await
                } else {
                    Err(err.into())
                }
            }
        }
    }

    async fn get_raw_range_inner(
        &self,
        pre_flop_actions: &[PreFlopAction],
    ) -> Result<RawRangeData> {
        let url = self.build_range_url(pre_flop_actions)?;
        let headers = self.build_headers()?;
        let client = Client::builder().default_headers(headers).build()?;
        let response = client.get(url).send().await?.error_for_status()?;
        let body: RawRangeData = response.json().await?;
        Ok(body)
    }

    fn build_range_url(&self, pre_flop_actions: &[PreFlopAction]) -> Result<Url> {
        let pre_flop_actions_formatted = self.url_encode_pre_flop_actions(pre_flop_actions)?;
        let depth = self.depth / 1000;
        let params: &[(&str, &str)] = &[
            ("gametype", &self.game_type),
            ("depth", &depth.to_string()),
            ("stacks", ""),
            ("preflop_actions", &pre_flop_actions_formatted),
            ("flop_actions", ""),
            ("turn_actions", ""),
            ("river_actions", ""),
            ("board", ""),
        ];
        let url = Url::parse_with_params(
            "https://api.gtowizard.com/v4/solutions/spot-solution/",
            params,
        )?;
        Ok(url)
    }

    fn build_headers(&self) -> Result<HeaderMap<HeaderValue>> {
        let mut headers = HeaderMap::new();

        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (X11; Linux x86_64; rv:139.0) Gecko/20100101 Firefox/139.0",
            ),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.5"));
        headers.insert(
            ACCEPT_ENCODING,
            HeaderValue::from_static("gzip, deflate, br, zstd"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            REFERER,
            HeaderValue::from_static("https://app.gtowizard.com/"),
        );
        headers.insert("GWCLIENTID", self.gw_client_id.parse()?);
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://app.gtowizard.com"),
        );
        headers.insert("DNT", HeaderValue::from_static("1"));
        headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
        headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
        headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-site"));
        headers.insert(
            "Authorization",
            format!("Bearer {}", self.bearer_token).parse()?,
        );
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
        headers.insert("Priority", HeaderValue::from_static("u=0"));

        Ok(headers)
    }

    fn url_encode_pre_flop_actions(&self, pre_flop_actions: &[PreFlopAction]) -> Result<String> {
        let mut pre_flop_actions_formatted = String::new();

        for (index, action) in pre_flop_actions.iter().copied().enumerate() {
            match action {
                PreFlopAction::Post { .. } => {
                    return Err("pre flop actions: additional post unsupported".into());
                }
                PreFlopAction::Straddle { .. } => {
                    return Err("pre flop actions: straddle unsupported".into());
                }
                PreFlopAction::Fold => pre_flop_actions_formatted.write_char('F')?,
                PreFlopAction::Check => pre_flop_actions_formatted.write_char('X')?,
                PreFlopAction::Call => pre_flop_actions_formatted.write_char('C')?,
                PreFlopAction::Raise(to) => {
                    let raise_formatted = &self.format_raise_in_url(to)?;
                    pre_flop_actions_formatted.write_str(&raise_formatted)?
                }
            }

            if index < pre_flop_actions.len() - 1 {
                pre_flop_actions_formatted.write_char('-')?;
            }
        }

        Ok(pre_flop_actions_formatted)
    }

    fn format_raise_in_url(&self, to: MilliBigBlind) -> Result<String> {
        if to < 2_000 {
            return Err("pre flop actions: raise too small".into());
        }

        if to == self.depth {
            return Ok("RAI".to_string());
        }

        let full_blinds = to / 1_000;
        let blind_fraction = to % 1_000;
        let blind_fraction = blind_fraction.to_string();
        let blind_fraction = blind_fraction.trim_end_matches('0');

        if blind_fraction.is_empty() {
            return Ok(format!("R{full_blinds}"));
        } else {
            return Ok(format!("R{full_blinds}.{blind_fraction}"));
        }
    }

    async fn renew_bearer_token(&mut self) -> Result<()> {
        eprintln!("Renewing bearer token...");

        let url = "https://api.gtowizard.com/v1/token/refresh/";
        let headers = self.build_headers()?;
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        let body = json!({
            "refresh": self.refresh_token,
        });

        let response = client
            .post(url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        let body = response.text().await?;
        let new_token: TokenRefresh = serde_json::from_str(&body)?;
        self.bearer_token = new_token.access;

        eprintln!("Updated bearer token to: {}", self.bearer_token);
        Ok(())
    }

    fn parse_raw_range(
        &self,
        pre_flop_actions: Vec<PreFlopAction>,
        raw_range: RawRangeData,
    ) -> Result<RangeConfigEntry> {
        let total_range = self.parse_raw_total_range(&pre_flop_actions, &raw_range)?;

        let mut actions: Vec<RangeAction> = Vec::new();

        for action_solution in raw_range.action_solutions {
            let action = match action_solution.action.action_type.as_str() {
                "FOLD" => PreFlopAction::Fold,
                "CHECK" => PreFlopAction::Check,
                "CALL" => PreFlopAction::Call,
                "RAISE" => {
                    let Some(raise_amount) = action_solution.action.bet_size else {
                        return Err("parse raw range: bet_size missing with type raise".into());
                    };
                    let raise_amount: f64 = raise_amount.parse()?;
                    let raise_amount = milli_big_blind_from_f64(raise_amount)?;
                    PreFlopAction::Raise(raise_amount)
                }
                _ => return Err("parse raw range: unknown strategy type".into()),
            };

            if action_solution.strategy.len() != PreFlopRangeTable::COUNT {
                return Err("parse raw range: strategy array has bad length".into());
            }

            let mut ev = PreFlopRangeTableWith::default();
            for (current_ev, hand) in action_solution
                .evs
                .into_iter()
                .zip(HANDS_ORDERED_BY_GTO_WIZARD_API)
            {
                let current_ev = milli_big_blind_from_f64(current_ev)?;
                ev[RangeEntry::from_str(hand).unwrap()] = current_ev;
            }

            let range = convert_frequency_array_to_range(&action_solution.strategy)?;
            actions.push(RangeAction::new(action, &total_range, range, Some(ev)));
        }

        eprintln!("Parsed range, validating...");

        let entry = RangeConfigEntry::new(
            pre_flop_actions,
            total_range,
            actions,
            self.max_players,
            self.depth,
            self.small_blind(),
            true,
        )?;

        eprintln!(
            "Validated range: {:?}",
            entry
                .actions()
                .iter()
                .map(|action| (action.action(), entry.frequency(action.action()) * 100.0))
                .collect::<Vec<_>>()
        );

        Ok(entry)
    }

    fn parse_raw_total_range(
        &self,
        pre_flop_actions: &[PreFlopAction],
        raw_range: &RawRangeData,
    ) -> Result<PreFlopRangeTableWith<u16>> {
        let current_game = RangeConfigEntry::build_game(
            self.max_players,
            self.depth,
            self.small_blind(),
            &pre_flop_actions,
        )?;

        let Some(current_player) = current_game.current_player() else {
            return Err("parse raw range: internal error while computing current player".into());
        };
        let player_position = Game::position_name(
            current_game.player_count(),
            current_game.button_index(),
            current_player,
        )
        .unwrap()
        .0;

        let current_player = raw_range
            .players_info
            .iter()
            .filter(|player_info| player_info.player.position == player_position)
            .next();
        let Some(current_player) = current_player else {
            return Err("parse raw range: current player not found".into());
        };

        let total_range = convert_frequency_array_to_range(&current_player.range)?;
        Ok(total_range)
    }

    async fn store_range(&self, entry: RangeConfigEntry) -> Result<()> {
        eprintln!("Storing progress...");

        let range_path = PathBuf::from(&self.out_dir).join(format!(
            "range_{}.json",
            self.url_encode_pre_flop_actions(entry.previous_actions())?
        ));
        fs::write(range_path, serde_json::to_string_pretty(&entry)?).await?;

        let possible_next_actions = entry.possible_next_actions(
            self.max_players,
            self.depth,
            self.small_blind(),
            self.min_frequency,
        )?;

        let next_actions = possible_next_actions
            .into_iter()
            .filter_map(|action| {
                let mut actions = Vec::from(entry.previous_actions());
                actions.push(action);

                if actions
                    .iter()
                    .filter(|action| **action == PreFlopAction::Call)
                    .count()
                    > self.max_calls
                {
                    None
                } else {
                    Some(actions)
                }
            })
            .collect();

        self.queue_pop_and_push_next(entry.previous_actions(), next_actions)
            .await?;
        Ok(())
    }

    fn small_blind(&self) -> MilliBigBlind {
        // Assume small blind is always half the big blind.
        500
    }
}

#[derive(Debug, Deserialize)]
struct RawRangeData {
    action_solutions: Vec<ActionSolution>,
    players_info: Vec<PlayerInfo>,
}

#[derive(Debug, Deserialize)]
struct ActionSolution {
    action: Action,
    strategy: Vec<f64>,
    evs: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct Action {
    #[serde(rename = "type")]
    action_type: String,
    #[serde(rename = "betsize")]
    bet_size: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlayerInfo {
    player: Player,
    range: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct Player {
    position: String,
}

#[derive(Debug, Deserialize)]
struct TokenRefresh {
    access: String,
}

const HANDS_ORDERED_BY_GTO_WIZARD_API: &[&str] = &[
    "22", "32o", "32s", "33", "42o", "42s", "43o", "43s", "44", "52o", "52s", "53o", "53s", "54o",
    "54s", "55", "62o", "62s", "63o", "63s", "64o", "64s", "65o", "65s", "66", "72o", "72s", "73o",
    "73s", "74o", "74s", "75o", "75s", "76o", "76s", "77", "82o", "82s", "83o", "83s", "84o",
    "84s", "85o", "85s", "86o", "86s", "87o", "87s", "88", "92o", "92s", "93o", "93s", "94o",
    "94s", "95o", "95s", "96o", "96s", "97o", "97s", "98o", "98s", "99", "A2o", "A2s", "A3o",
    "A3s", "A4o", "A4s", "A5o", "A5s", "A6o", "A6s", "A7o", "A7s", "A8o", "A8s", "A9o", "A9s",
    "AA", "AJo", "AJs", "AKo", "AKs", "AQo", "AQs", "ATo", "ATs", "J2o", "J2s", "J3o", "J3s",
    "J4o", "J4s", "J5o", "J5s", "J6o", "J6s", "J7o", "J7s", "J8o", "J8s", "J9o", "J9s", "JJ",
    "JTo", "JTs", "K2o", "K2s", "K3o", "K3s", "K4o", "K4s", "K5o", "K5s", "K6o", "K6s", "K7o",
    "K7s", "K8o", "K8s", "K9o", "K9s", "KJo", "KJs", "KK", "KQo", "KQs", "KTo", "KTs", "Q2o",
    "Q2s", "Q3o", "Q3s", "Q4o", "Q4s", "Q5o", "Q5s", "Q6o", "Q6s", "Q7o", "Q7s", "Q8o", "Q8s",
    "Q9o", "Q9s", "QJo", "QJs", "QQ", "QTo", "QTs", "T2o", "T2s", "T3o", "T3s", "T4o", "T4s",
    "T5o", "T5s", "T6o", "T6s", "T7o", "T7s", "T8o", "T8s", "T9o", "T9s", "TT",
];

fn convert_frequency_array_to_range(frequencies: &[f64]) -> Result<PreFlopRangeTableWith<u16>> {
    if frequencies.len() != PreFlopRangeTable::COUNT {
        return Err("parse raw range: bad frequency array length".into());
    }

    let mut range = PreFlopRangeTableWith::default();

    for (mut strategy, hand) in frequencies
        .iter()
        .copied()
        .zip(HANDS_ORDERED_BY_GTO_WIZARD_API)
    {
        // Sometimes the GTO Wizard frequency is a litter bigger than 1.0.
        if strategy > 1.0 && strategy <= 1.01 {
            strategy = 1.0;
        }

        let frequency = convert_frequency(strategy)?;
        range[RangeEntry::from_str(hand).unwrap()] = frequency;
    }

    Ok(range)
}

fn convert_frequency(frequency: f64) -> Result<u16> {
    if frequency.is_nan() || frequency < 0.0 || frequency > 1.0 {
        return Err("parse raw range: frequency not in range from zero to one".into());
    }

    let frequency = frequency * 10_000.0;
    Ok(frequency.trunc() as u16)
}
