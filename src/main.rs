//! Entry point of the Tentrackule application.
//!
//! Initializes the various components and starts both the Discord bot and the
//! result poller.

use std::{env, sync::Arc};

use dotenv::dotenv;
use tentrackule_alert::alert_dispatcher::DiscordAlertDispatcher;
use tentrackule_bot::{ApiClients, DiscordBot};
use tentrackule_db::SharedDatabase;
use tentrackule_result_poller::{lol::LolResultPoller, tft::TftResultPoller};
use tentrackule_riot_api::api::{lol::LolApiClient, tft::TftApiClient};
use tentrackule_shared::init_ddragon_version;
use tracing::{error, info};

mod logging;

#[tokio::main]
async fn main() {
    dotenv().ok();
    logging::init();
    init_ddragon_version();

    info!("🚀 Tentrackule starting");

    let db = SharedDatabase::new_from_env().unwrap();
    db.init().await;

    let lol_client: Arc<LolApiClient> = LolApiClient::new(get_lol_api_key_from_env()).into();
    let tft_client = Arc::new(TftApiClient::new(get_tft_api_key_from_env()));
    let api_clients = ApiClients {
        lol: Some(lol_client.clone()),
        tft: Some(tft_client.clone()),
    };

    let bot = DiscordBot::new(Arc::new(db.clone()), api_clients).await;
    let alert_dispatcher: DiscordAlertDispatcher<SharedDatabase> =
        DiscordAlertDispatcher::new(bot.client().http.clone(), db.clone());

    let lol_result_poller = LolResultPoller::new(
        lol_client.clone(),
        db.clone(),
        alert_dispatcher.clone(),
        "LoL",
    );
    let tft_result_poller = TftResultPoller::new(tft_client.clone(), db, alert_dispatcher, "TFT");

    lol_client.start_metrics_logging();
    tft_client.start_metrics_logging();

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
        res = lol_result_poller.start() => {
            match res {
                Ok(()) => info!("The LoL result poller exited gracefully."),
                Err(e) => {
                    error!("The LoL result poller crashed: {:?}", e);
                    return;
                },
            }
        },
        res = tft_result_poller.start() => {
            match res {
                Ok(()) => info!("The TFT result poller exited gracefully."),
                Err(e) => {
                    error!("The TFT result poller crashed: {:?}", e);
                    return;
                },
            }
        },
    }
}

fn get_lol_api_key_from_env() -> String {
    env::var("LOL_API_KEY")
        .expect("A LoL Riot API Key must be set in environment to create the API Client.")
}

fn get_tft_api_key_from_env() -> String {
    env::var("TFT_API_KEY")
        .expect("A TFT Riot API Key must be set in environment to create the API Client.")
}
