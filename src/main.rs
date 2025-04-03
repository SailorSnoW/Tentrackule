use db::DatabaseHandler;
use discord::DiscordBot;
use dotenv::dotenv;
use log::info;
use riot::RiotApiHandler;

mod db;
mod discord;
mod riot;

#[tokio::main]
async fn main() {
    env_logger::builder()
        .format_timestamp(None)
        .filter_module("Tentrackule", log::LevelFilter::Debug)
        .init();
    dotenv().ok();

    info!("🐙 Starting...");

    let db = DatabaseHandler::new();
    let api = RiotApiHandler::new();
    let bot = DiscordBot::new(db.sender_cloned(), api.sender_cloned());

    db.start();
    api.start();
    bot.start().await.unwrap();
}
