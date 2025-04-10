use std::env;

use client::RiotClient;
use log::info;
use tokio::sync::{mpsc, oneshot};
use types::{AccountDto, LeagueEntryDto, MatchDto, Region, RiotApiResponse};

mod client;
pub mod result_poller;
pub mod types;

pub type RiotApiRx = mpsc::Receiver<ApiRequest>;
pub type RiotApiTx = mpsc::Sender<ApiRequest>;

pub struct RiotApiHandler {
    client: RiotClient,
    sender: RiotApiTx,
    receiver: RiotApiRx,
}

impl RiotApiHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        let key = env::var("RIOT_API_KEY")
            .expect("A Riot API Key must be set in environment to create the API Client.");

        Self {
            client: RiotClient::new(key),
            sender: tx,
            receiver: rx,
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    pub fn sender_cloned(&self) -> RiotApiTx {
        self.sender.clone()
    }

    async fn run(mut self) {
        info!("📡 Starting Riot API Handler...");

        while let Some(request) = self.receiver.recv().await {
            // Ensure we do not enforce the RIOT API rate limits before doing any request
            self.client.limiter.until_ready().await;

            match request {
                ApiRequest::PuuidByAccountId {
                    game_name,
                    tag_line,
                    respond_to,
                } => {
                    let account_data = self
                        .client
                        .get_account_by_riot_id(game_name, tag_line)
                        .await;
                    let _ = respond_to.send(account_data);
                }
                ApiRequest::LastMatch {
                    puuid,
                    region,
                    respond_to,
                } => {
                    let _ = respond_to.send(self.client.get_last_match_id(puuid, region).await);
                }
                ApiRequest::LastMatchData {
                    match_id,
                    region,
                    respond_to,
                } => {
                    let _ = respond_to.send(self.client.get_match(match_id, region).await);
                }
                ApiRequest::Leagues {
                    puuid,
                    region,
                    respond_to,
                } => {
                    let _ = respond_to.send(self.client.get_leagues(puuid, region).await);
                }
            }
        }
    }
}

/// MPSC Messages to communicate with the Riot API Task.
#[derive(Debug)]
pub enum ApiRequest {
    PuuidByAccountId {
        game_name: String,
        tag_line: String,
        respond_to: oneshot::Sender<RiotApiResponse<AccountDto>>,
    },
    LastMatch {
        puuid: String,
        region: Region,
        respond_to: oneshot::Sender<RiotApiResponse<Option<String>>>,
    },
    LastMatchData {
        match_id: String,
        region: Region,
        respond_to: oneshot::Sender<RiotApiResponse<MatchDto>>,
    },
    Leagues {
        puuid: String,
        region: Region,
        respond_to: oneshot::Sender<RiotApiResponse<Vec<LeagueEntryDto>>>,
    },
}
