use std::{env, sync::Arc};

use dotenv::dotenv;
use result_poller::ResultPoller;
use tentrackule_bot::{AlertDispatcher, DiscordBot};
use tentrackule_db::Database;
use tentrackule_riot_api::api::{init_ddragon_version, LolApiClient};
use tracing::{error, info};

mod logging;
mod result_poller;

#[tokio::main]
async fn main() {
    logging::init();
    dotenv().ok();
    init_ddragon_version();

    info!("🚀 [MAIN] Tentrackule starting");

    let db = Database::new_shared_from_env();

    let lol_api: Arc<LolApiClient> = LolApiClient::new(get_api_key_from_env()).into();
    let bot = DiscordBot::new(db.clone(), lol_api.clone()).await;
    let alert_dispatcher: Arc<AlertDispatcher> =
        AlertDispatcher::new(bot.client.http.clone(), db.clone()).into();
    let result_poller = ResultPoller::new(lol_api.clone(), db, alert_dispatcher);

    lol_api.start_metrics_logging();

    tokio::select! {
        res = bot.start() => {
            match res {
                Ok(Ok(())) => info!("The discord bot task exited gracefully."),
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
                Ok(()) => info!("The result poller exited gracefully."),
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
