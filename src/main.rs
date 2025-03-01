#![allow(dead_code)] // TODO

mod card;
mod cards;
mod equity;
mod hand;
mod range;
mod rank;
mod result;
mod suite;

use std::sync::Arc;

use crate::cards::Cards;
use crate::equity::Equity;
use crate::hand::Hand;
use crate::range::RangeTable;
use crate::result::Result;

const INVALID_COMMAND_ERROR: &'static str = "Invalid command. See README for usage.";

fn main() -> Result<()> {
    unsafe { Cards::init() };

    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|cmd| cmd == "enumerate") {
        enumerate(&args[2..])
    } else if args.get(1).is_some_and(|cmd| cmd == "simulate") {
        simulate(&args[2..])
    } else {
        Err(INVALID_COMMAND_ERROR.into())
    }
}

fn enumerate(args: &[String]) -> Result<()> {
    let [community_cards_raw, hero_hand_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let community_cards = Cards::from_str(community_cards_raw)?;
    let hero_hand = Hand::from_str(hero_hand_raw)?;
    let villain_ranges = args[2..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Arc::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) = Equity::enumerate(community_cards, hero_hand, &villain_ranges) else {
        return Err("enumerate failed: invalid input or expected sample to large".into());
    };
    print_equities(&equities);
    Ok(())
}

fn simulate(args: &[String]) -> Result<()> {
    let [rounds_raw, community_cards_raw, hero_hand_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let rounds: u64 = rounds_raw.parse()?;
    let community_cards = Cards::from_str(community_cards_raw)?;
    let hero_hand = Hand::from_str(hero_hand_raw)?;
    let villain_ranges = args[3..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Arc::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) =
        Equity::simulate_ranges(community_cards, hero_hand, &villain_ranges, rounds)
    else {
        return Err("simulate failed: invalid input".into());
    };
    print_equities(&equities);
    Ok(())
}

fn print_equities(equities: &[Equity]) {
    assert!(equities.len() >= 2);
    println!("hero:      {}", equities[0]);
    for (i, equity) in equities[1..].iter().enumerate() {
        println!("villain {}: {}", i + 1, equity);
    }
}
