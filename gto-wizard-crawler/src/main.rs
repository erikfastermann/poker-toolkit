mod crawler;

use poker_core::{game::MilliBigBlind, result::Result};

use crate::crawler::Crawler;

#[tokio::main]
async fn main() -> Result<()> {
    unsafe { poker_core::init::init() };

    let args: Vec<_> = std::env::args().collect();
    let Ok([_, server_url, game_type, max_players_raw, depth_raw, rake, out_dir]) =
        <[_; 7]>::try_from(args)
    else {
        // Example:
        // cargo run -- http://localhost:46565 Cash6m50zGGGeneral 6 100000 GG+NL50 ./out
        return Err(
            "USAGE: ./gto-wizard-crawler <chrome driver server url> <game type> \
            <max players> <depth> <rake> <out dir>"
                .into(),
        );
    };

    let depth: MilliBigBlind = depth_raw.parse()?;
    let max_players: usize = max_players_raw.parse()?;

    let crawler = Crawler::new(server_url, game_type, max_players, depth, rake, out_dir).await?;
    crawler.crawl().await?;
    // We don't quit the webdriver afterwards.
    Ok(())
}
