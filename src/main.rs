use std::{env, sync::Arc};

use dotenv::dotenv;
use result_poller::ResultPoller;
use tentrackule_bot::{AlertDispatcher, DiscordBot};
use tentrackule_db::Database;
use tentrackule_riot_api::api::{client::ApiClient, LolApi};
use tracing::{error, info};

mod logging;
mod result_poller;

#[tokio::main]
async fn main() {
    logging::init();
    dotenv().ok();

    info!("🚀 [MAIN] Tentrackule starting");

    let db = Database::new_shared_from_env();

    let lol_api: Arc<LolApi> = LolApi::new(ApiClient::new(get_api_key_from_env()).into()).into();
    let bot = DiscordBot::new(db.clone(), lol_api.clone()).await;
    let alert_dispatcher = AlertDispatcher::new(bot.client.http.clone(), db.clone());
    let result_poller = ResultPoller::new(lol_api.clone(), db, alert_dispatcher);

    tokio::select! {
        res = bot.start() => {
            match res {
                Ok(Ok(())) => unreachable!(),
                Ok(Err(e)) => {
                    error!("The discord bot task crashed: {:?}", e);
                    return;
                },
                Err(e) => {
                    error!("The discord bot task panicked: {:?}", e);
                    return;
                },
            }
        },
        res = result_poller.start() => {
            match res {
                Ok(()) => unreachable!(),
                Err(e) => {
                    error!("The result poller crashed: {:?}", e);
                    return;
                },
            }
        },
    }
}

fn get_api_key_from_env() -> String {
    env::var("RIOT_API_KEY")
        .expect("A Riot API Key must be set in environment to create the API Client.")
}
