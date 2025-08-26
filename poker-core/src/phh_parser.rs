use std::iter;

use bigdecimal::BigDecimal;
use indexmap::IndexMap;
use serde::Deserialize;
use toml::value::Datetime;

use crate::{game::Game, result::Result};

#[derive(Debug, Deserialize)]
struct Entry {
    variant: String,
    antes: Vec<BigDecimal>,
    blinds_or_straddles: Vec<BigDecimal>,
    min_bet: BigDecimal,
    /// Can be inf, which we don't parse in this case.
    starting_stacks: Vec<BigDecimal>,
    actions: Vec<String>,

    author: Option<String>,
    event: Option<String>,
    url: Option<String>,
    venue: Option<String>,
    address: Option<String>,
    city: Option<String>,
    region: Option<String>,
    postal_code: Option<String>,
    country: Option<String>,
    time: Option<Datetime>,    // either in time zone, location or utc
    time_zone: Option<String>, // iana.org/time-zones
    time_zone_abbreviation: Option<String>,
    day: Option<u8>,
    month: Option<u8>,
    year: Option<u32>,
    hand: Option<String>,
    seats: Option<Vec<u8>>,
    seat_count: Option<u8>,
    table: Option<String>,
    players: Option<Vec<String>>,
    finishing_stacks: Option<Vec<BigDecimal>>,
    winnings: Option<Vec<BigDecimal>>,
    currency: Option<String>, // ISO 4127
    currency_symbol: Option<String>,
    ante_trimming_status: Option<bool>,
}

pub fn parse_phhs_str(phhs: &str) -> Result<impl Iterator<Item = Result<Game>>> {
    // TODO: Accept integers as strings.
    let entries: IndexMap<String, Entry> = toml::from_str(phhs)?;

    // TODO
    dbg!(entries);
    Ok(iter::empty())
}
