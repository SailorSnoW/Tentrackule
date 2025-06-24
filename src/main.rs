use std::{env, sync::Arc, time::Duration};

use db::DatabaseHandler;
use discord::{AlertSender, DiscordBot};
use dotenv::dotenv;
use riot::{
    api::{client::ApiClient, LolApi},
    result_poller::ResultPoller,
};
use tokio::sync::mpsc;
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

    let poll_interval = env::var("POLL_INTERVAL_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(60);
    let poll_interval = Duration::from_secs(poll_interval);

    let (tx, rx) = mpsc::channel(100);
    let alert_sender_handler = AlertSender::new(rx, bot.client.http.clone(), db.sender_cloned());
    let result_poller_handle =
        ResultPoller::new(lol_api.clone(), db.sender_cloned(), tx, poll_interval);

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
        // Need to be spawned before the result poller in case the result poller try to send a
        // message to send an alert.
        res = alert_sender_handler.start() => {
            match res {
                Ok(()) => unreachable!(),
                Err(e) => panic!("The alert sender crashed: {:?}", e),
            }
        },
        res = result_poller_handle.start() => {
            match res {
                Ok(()) => unreachable!(),
                Err(e) => panic!("The result poller crashed: {:?}", e),
            }
        },
    }
}
