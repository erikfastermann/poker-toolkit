use poker_core::cards::Cards;
use poker_core::equity::Equity;
use poker_core::range::RangeTable;
use poker_core::result::Result;
use poker_gui::gui::gui;

const INVALID_COMMAND_ERROR: &'static str = "Invalid command. See README for usage.";

fn main() -> Result<()> {
    unsafe { poker_core::init::init() };

    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|cmd| cmd == "enumerate") {
        enumerate(&args[2..])
    } else if args.get(1).is_some_and(|cmd| cmd == "simulate") {
        simulate(&args[2..])
    } else if args.get(1).is_some_and(|cmd| cmd == "gui") {
        if args.len() != 2 {
            return Err(INVALID_COMMAND_ERROR.into());
        }
        gui().map_err(|err| format!("{err}"))?;
        Ok(())
    } else {
        Err(INVALID_COMMAND_ERROR.into())
    }
}

fn enumerate(args: &[String]) -> Result<()> {
    let [community_cards_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let community_cards = Cards::from_str(community_cards_raw)?;
    let ranges = args[1..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Box::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) = Equity::enumerate(community_cards, &ranges) else {
        return Err("enumerate failed: invalid input or expected sample to large".into());
    };
    print_equities(&equities);
    Ok(())
}

fn simulate(args: &[String]) -> Result<()> {
    let [rounds_raw, community_cards_raw, ..] = args else {
        return Err(INVALID_COMMAND_ERROR.into());
    };
    let rounds: u64 = rounds_raw.parse()?;
    let community_cards = Cards::from_str(community_cards_raw)?;
    let ranges = args[2..]
        .iter()
        .map(|raw_range| RangeTable::parse(&raw_range))
        .map(|r| r.map(Box::new))
        .collect::<Result<Vec<_>>>()?;
    let Some(equities) = Equity::simulate(community_cards, &ranges, rounds) else {
        return Err("simulate failed: invalid input".into());
    };
    print_equities(&equities);
    Ok(())
}

fn print_equities(equities: &[Equity]) {
    assert!(equities.len() >= 2);
    for (i, equity) in equities.iter().enumerate() {
        println!("player {}: {}", i + 1, equity);
    }
}
