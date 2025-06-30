//! Entry point of the Tentrackule application.
//!
//! Initializes the various components and starts both the Discord bot and the
//! result poller.

use std::{env, sync::Arc};

use dotenv::dotenv;
use result_poller::ResultPoller;
use tentrackule_bot::{DiscordAlertDispatcher, DiscordBot};
use tentrackule_db::SharedDatabase;
use tentrackule_riot_api::api::LolApiClient;
use tentrackule_types::init_ddragon_version;
use tracing::{error, info};

mod logging;
mod result_poller;

#[tokio::main]
async fn main() {
    dotenv().ok();
    logging::init();
    init_ddragon_version();

    info!("🚀 Tentrackule starting");

    let db = SharedDatabase::new_from_env().unwrap();
    db.init().await;

    let lol_api: Arc<LolApiClient> = LolApiClient::new(get_api_key_from_env()).into();
    let bot = DiscordBot::new(Arc::new(db.clone()), lol_api.clone()).await;
    let alert_dispatcher: DiscordAlertDispatcher<SharedDatabase> =
        DiscordAlertDispatcher::new(Arc::new(bot.client().http.clone()), db.clone());
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
