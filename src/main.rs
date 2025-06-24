use std::sync::Arc;

use db::DatabaseHandler;
use discord::{AlertSender, DiscordBot};
use dotenv::dotenv;
use riot::{
    api::{client::ApiClient, LolApi},
    result_poller::ResultPoller,
};
use tracing::info;

mod db;
mod discord;
mod logging;
mod riot;

#[tokio::main]
async fn main() {
    logging::init();
    dotenv().ok();

    info!("🚀 [MAIN] Tentrackule starting");

    let db = DatabaseHandler::new();

    let lol_api: Arc<LolApi> = LolApi::new(ApiClient::new().into()).into();
    let bot = DiscordBot::new(db.sender_cloned(), lol_api.clone()).await;
    let alert_sender = AlertSender::new(bot.client.http.clone(), db.sender_cloned());
    let result_poller = ResultPoller::new(lol_api.clone(), db.sender_cloned(), alert_sender);

    tokio::select! {
        res = db.start() => {
            match res {
                Ok(()) => unreachable!(),
                Err(e) => panic!("The DatabaseHandler crashed: {:?}", e),
            }
        },
        res = bot.start() => {
            match res {
                Ok(()) => unreachable!(),
                Err(e) => panic!("The discord bot task crashed: {:?}", e),
            }
        },
        res = result_poller.start() => {
            match res {
                Ok(()) => unreachable!(),
                Err(e) => panic!("The result poller crashed: {:?}", e),
            }
        },
    }
}
