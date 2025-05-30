mod crawler;

use crawler::Config;
use poker_core::result::Result;
use tokio::fs;

use crate::crawler::Crawler;

#[tokio::main]
async fn main() -> Result<()> {
    unsafe { poker_core::init::init() };

    let args: Vec<_> = std::env::args().collect();
    let Ok([_, config_path]) = <[_; 2]>::try_from(args) else {
        return Err("USAGE: ./gto-wizard-crawler <config path>".into());
    };

    let config: Config = serde_json::from_str(&fs::read_to_string(config_path).await?)?;

    let crawler = Crawler::new(config).await?;
    crawler.crawl().await?;

    Ok(())
}
