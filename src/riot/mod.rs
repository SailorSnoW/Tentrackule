use std::{env, sync::Arc};

use api::{
    client::{AccountDto, ApiClient},
    metrics::RequestMetrics,
    types::{LeagueEntryDto, MatchDto},
    LolApi,
};
use tokio::sync::{mpsc, oneshot};
use tracing::info;
use types::{Region, RiotApiResponse};

pub mod api;
pub mod result_poller;
pub mod types;

pub type LolApiRx = mpsc::Receiver<LolApiRequest>;
pub type LolApiTx = mpsc::Sender<LolApiRequest>;

pub struct LolApiHandler {
    api: LolApi,
    sender: LolApiTx,
    receiver: LolApiRx,
    metrics: Arc<RequestMetrics>,
}

impl LolApiHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        let key = env::var("RIOT_API_KEY")
            .expect("A Riot API Key must be set in environment to create the API Client.");
        let metrics = RequestMetrics::new();
        let api_client = ApiClient::new(key, metrics.clone());

        Self {
            api: LolApi::new(api_client.into()),
            sender: tx,
            receiver: rx,
            metrics,
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let metrics = self.metrics.clone();
        tokio::spawn(async move {
            metrics.log_loop().await;
        });

        tokio::spawn(async move {
            self.run().await;
        })
    }

    pub fn sender_cloned(&self) -> LolApiTx {
        self.sender.clone()
    }

    async fn run(mut self) {
        info!("🛰️ [LoL] API handler started");

        while let Some(request) = self.receiver.recv().await {
            // Ensure we do not enforce the RIOT API rate limits before doing any request
            self.api.client.limiter.until_ready().await;

            match request {
                LolApiRequest::PuuidByAccountId {
                    game_name,
                    tag_line,
                    respond_to,
                } => {
                    let account_data = self
                        .api
                        .client
                        .get_account_by_riot_id(game_name, tag_line)
                        .await;
                    let _ = respond_to.send(account_data);
                }
                LolApiRequest::LastMatch {
                    puuid,
                    region,
                    respond_to,
                } => {
                    let _ =
                        respond_to.send(self.api.match_v5.get_last_match_id(puuid, region).await);
                }
                LolApiRequest::LastMatchData {
                    match_id,
                    region,
                    respond_to,
                } => {
                    let _ = respond_to.send(self.api.match_v5.get_match(match_id, region).await);
                }
                LolApiRequest::Leagues {
                    puuid,
                    region,
                    respond_to,
                } => {
                    let _ = respond_to.send(self.api.league_v4.get_leagues(puuid, region).await);
                }
            }
        }
    }
}

/// MPSC Messages to communicate with the LoL API Task.
#[derive(Debug)]
pub enum LolApiRequest {
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
