use std::{
    fmt::Write,
    io::ErrorKind,
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use rand::Rng;
use serde::Deserialize;
use thirtyfour::prelude::*;

use poker_core::{
    game::{milli_big_blind_from_f64, Game, MilliBigBlind},
    range::{
        PreFlopAction, PreFlopRangeTable, PreFlopRangeTableWith, RangeAction, RangeConfigEntry,
        RangeEntry,
    },
    result::Result,
};
use tokio::{
    fs,
    io::{stdin, AsyncBufReadExt, BufReader},
    time::sleep,
};
use url::Url;

pub struct Crawler {
    driver: WebDriver,
    game_type: String,
    max_players: usize,
    depth: MilliBigBlind,
    rake: String,
    out_dir: String,
}

impl Crawler {
    pub async fn new(
        server_url: String,
        game_type: String,
        max_players: usize,
        depth: MilliBigBlind,
        rake: String,
        out_dir: String,
    ) -> Result<Self> {
        if depth < 1_000 || depth % 1000 != 0 {
            return Err("crawler: invalid depth".into());
        }
        if max_players < Game::MIN_PLAYERS || max_players > Game::MAX_PLAYERS {
            return Err("crawler: invalid max players".into());
        }

        let caps = DesiredCapabilities::chrome();
        eprintln!("Connecting to chromedriver...");
        let driver = WebDriver::new(server_url, caps).await?;

        let crawler = Self {
            driver,
            game_type,
            max_players,
            depth,
            rake,
            out_dir,
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

    pub async fn crawl(&self) -> Result<()> {
        let result = self.crawl_inner().await.inspect_err(|err| {
            eprintln!("An error occurred while crawling: {err}");
        });
        eprintln!("Press any key to exit the browser...");
        wait_for_newline_from_stdin().await?;
        result
    }

    async fn crawl_inner(&self) -> Result<()> {
        eprintln!("Navigating to GTO Wizard...");
        self.driver.goto("https://app.gtowizard.com").await?;
        eprintln!(
            "Please login (auth via Google does not work) and then press enter to continue..."
        );
        wait_for_newline_from_stdin().await?;

        loop {
            let queue = self.read_queue().await?;
            let Some(next_pre_flop_actions) = queue.first() else {
                eprintln!("Queue is empty, nothing to do. Exiting....");
                return Ok(());
            };

            self.get_and_parse_range(next_pre_flop_actions.clone())
                .await?;
            sleep(Duration::from_secs(rand::thread_rng().gen_range(2..=10))).await;
        }
    }

    async fn get_and_parse_range(&self, pre_flop_actions: Vec<PreFlopAction>) -> Result<()> {
        eprintln!("Processing {pre_flop_actions:?}...");

        let url = self.build_url(&pre_flop_actions)?;
        self.driver.goto(url.to_string()).await?;
        sleep(Duration::from_secs(2)).await;

        // Wait for range table.
        self.driver
            .query(By::Css("[data-tst='range_table_hero']"))
            .and_clickable()
            .first()
            .await?;

        const MAX_ROUNDS: usize = 20;
        for round in 1..=MAX_ROUNDS {
            let vue_table_data_script = r#"
                const table = document.querySelector("[data-tst='range_table_hero']");
                const tableGroups = table.__vueParentComponent.props.tableGroups;
                return tableGroups.map(g => ({
                    id: g.id,
                    data: g.combos[0].solution.map(s => ({
                        kind: s.type,
                        raise_amount: s.raise_amount,
                        ev: s.ev,
                        strategy: s.strategy,
                    })),
                }));
            "#;
            let table = self.driver.execute(vue_table_data_script, []).await?;

            let actions = match self.parse_table_data(table) {
                Ok(actions) => actions,
                Err(err) => {
                    if round != MAX_ROUNDS {
                        if round % 5 == 0 {
                            eprintln!("round {round} of parsing js data: {err}");
                        }

                        sleep(Duration::from_secs(1)).await;
                        continue;
                    } else {
                        return Err(err);
                    }
                }
            };

            let entry = RangeConfigEntry {
                previous_actions: pre_flop_actions,
                actions,
            };
            self.store_range(entry).await?;
            return Ok(());
        }

        Err("crawler: timeout while parsing range table".into())
    }

    fn build_url(&self, pre_flop_actions: &[PreFlopAction]) -> Result<Url> {
        let pre_flop_actions_formatted = self.url_encode_pre_flop_actions(pre_flop_actions)?;
        let depth = self.depth / 1000;
        let params: &[(&str, &str)] = &[
            ("gametype", &self.game_type),
            ("depth", &depth.to_string()),
            ("soltab", "strategy"),
            ("solution_type", "gwiz"),
            ("gmfs_solution_tab", "ai_sols"),
            ("gmfft_sort_key", "0"),
            ("gmfft_sort_order", "desc"),
            ("history_spot", &pre_flop_actions.len().to_string()),
            ("gmff_rake", &self.rake),
            ("preflop_actions", &pre_flop_actions_formatted),
        ];
        let url = Url::parse_with_params("https://app.gtowizard.com/solutions", params)?;
        Ok(url)
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

    fn parse_table_data(&self, table: ScriptRet) -> Result<Vec<RangeAction>> {
        let js_range: Vec<JsRangeEntry> = table.convert()?;

        let mut actions: Vec<RangeAction> = Vec::new();
        for entry in js_range {
            let id = RangeEntry::from_str(&entry.id)?;

            if entry.data.is_empty() {
                return Err("parse js: strategy and ev data is empty".into());
            }

            for strategy in entry.data {
                let action = match strategy.kind.as_str() {
                    "FOLD" => PreFlopAction::Fold,
                    "CHECK" => PreFlopAction::Check,
                    "CALL" => PreFlopAction::Call,
                    "RAISE" => {
                        let Some(raise_amount) = strategy.raise_amount else {
                            return Err("parse js: raise_amount missing with type raise".into());
                        };
                        let raise_amount = milli_big_blind_from_f64(raise_amount)?;
                        PreFlopAction::Raise(raise_amount)
                    }
                    _ => return Err("parse js: unknown strategy type".into()),
                };

                if strategy.strategy.is_nan() || strategy.strategy < 0.0 || strategy.strategy > 1.0
                {
                    return Err("parse js: strategy not in range from zero to one".into());
                }
                let frequency = strategy.strategy * 10_000.0;
                let frequency = frequency.trunc() as u16;

                let ev = milli_big_blind_from_f64(strategy.ev)?;

                let matching_action = {
                    let matching_action = actions
                        .iter_mut()
                        .filter(|current_action| current_action.action == action)
                        .next();

                    match matching_action {
                        Some(action) => action,
                        None => {
                            actions.push(RangeAction {
                                action,
                                range: PreFlopRangeTableWith::default(),
                                ev: Some(PreFlopRangeTableWith::default()),
                            });
                            actions.last_mut().unwrap()
                        }
                    }
                };

                matching_action.range[id] = frequency;
                matching_action.ev.as_mut().unwrap()[id] = ev;
            }
        }

        self.round_range_frequencies(&mut actions)?;
        Ok(actions)
    }

    fn round_range_frequencies(&self, actions: &mut Vec<RangeAction>) -> Result<()> {
        if !actions
            .iter()
            .any(|action| action.action == PreFlopAction::Fold)
        {
            return Ok(());
        }

        for entry in PreFlopRangeTable::entries() {
            let total_frequencies = actions
                .iter()
                .map(|action| action.range[entry])
                .fold(Some(0u16), |acc, n| acc.and_then(|acc| acc.checked_add(n)));

            let Some(total_frequencies) = total_frequencies else {
                return Err("parse js: total frequencies of range entry overflowed".into());
            };

            if total_frequencies < 10_000 {
                let max_frequency_action = actions
                    .iter_mut()
                    .max_by_key(|action| action.range[entry])
                    .unwrap();

                max_frequency_action.range[entry] += 10_000 - total_frequencies;
            }
        }

        Ok(())
    }

    async fn store_range(&self, entry: RangeConfigEntry) -> Result<()> {
        eprintln!("Done processing, storing progress...");

        let range_path = PathBuf::from(&self.out_dir).join(format!(
            "range_{}.json",
            self.url_encode_pre_flop_actions(&entry.previous_actions)?
        ));
        fs::write(range_path, serde_json::to_string_pretty(&entry)?).await?;

        // Assume small blind is always half the big blind.
        let possible_next_actions = entry.validate(self.max_players, self.depth, 500)?;

        let mut next_actions: Vec<_> = possible_next_actions
            .into_iter()
            .map(|action| {
                let mut actions = entry.previous_actions.clone();
                actions.push(action);
                actions
            })
            .collect();
        next_actions.reverse();

        self.queue_pop_and_push_next(&entry.previous_actions, next_actions)
            .await?;

        Ok(())
    }
}

async fn wait_for_newline_from_stdin() -> Result<()> {
    let mut buf = String::new();
    BufReader::new(stdin()).read_line(&mut buf).await?;
    Ok(())
}

#[derive(Deserialize)]
struct JsRangeEntry {
    id: String,
    data: Vec<JsRangeStrategyEntry>,
}

#[derive(Deserialize)]
struct JsRangeStrategyEntry {
    kind: String,
    raise_amount: Option<f64>,
    strategy: f64,
    ev: f64,
}
