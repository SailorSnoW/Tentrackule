use super::{
    api::{
        types::{LeagueEntryDto, MatchDto},
        LolApi,
    },
    types::{LeaguePoints, Region},
};
use crate::{
    db::{types::Account, DbRequest},
    discord::AlertSender,
    riot::{api::types::MatchDtoWithLeagueInfo, types::QueueType},
};
use dotenv::dotenv;
use futures::{stream, StreamExt};
use poise::serenity_prelude::Timestamp;
use std::{env, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

/// Poller responsible for automatically fetching new results of tracked player from Riot servers, parsing results data and sending it to the discord receiver when alerting is needed.
pub struct ResultPoller {
    lol_api: Arc<LolApi>,
    db_sender: mpsc::Sender<DbRequest>,
    alert_sender: AlertSender,
    start_time: u64,
    poll_interval: Duration,
}

impl ResultPoller {
    pub fn new(
        lol_api: Arc<LolApi>,
        db_sender: mpsc::Sender<DbRequest>,
        alert_sender: AlertSender,
    ) -> Self {
        dotenv().ok();

        let poll_interval_u64 = env::var("POLL_INTERVAL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        let poll_interval = Duration::from_secs(poll_interval_u64);

        Self {
            lol_api,
            db_sender,
            alert_sender,
            start_time: Timestamp::now().timestamp_millis() as u64,
            poll_interval,
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(&self) {
        info!("📡 [POLL] poller started");

        let mut interval = tokio::time::interval(self.poll_interval);

        loop {
            interval.tick().await;
            self.poll_once().await;
        }
    }

    async fn poll_once(&self) {
        info!("🔄 [POLL] starting fetch cycle");

        let accounts = self.get_all_accounts().await;
        stream::iter(accounts)
            .for_each_concurrent(10, |account| async move {
                self.process_account(account).await;
            })
            .await;
    }

    async fn get_all_accounts(&self) -> Vec<Account> {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .db_sender
            .send(DbRequest::GetAllAccounts { respond_to: tx })
            .await
        {
            error!("Failed to send DB request: {}", e);
            return Vec::new();
        }
        match rx.await {
            Ok(Ok(accounts)) => accounts,
            Ok(Err(e)) => {
                error!("Database error while fetching accounts: {}", e);
                Vec::new()
            }
            Err(e) => {
                error!("DB channel error: {}", e);
                Vec::new()
            }
        }
    }

    async fn process_account(&self, account: Account) {
        debug!(
            "🔍 [POLL] checking {}#{}",
            account.game_name, account.tag_line
        );
        let Some(new_match_id) = self
            .fetch_new_match_id(account.puuid.clone(), account.region.clone())
            .await
        else {
            return;
        };

        if new_match_id == account.last_match_id {
            debug!(
                "⏭️ [POLL] {}#{} no new result",
                account.game_name, account.tag_line
            );
            return;
        }

        debug!(
            "💾 [POLL] {}#{} caching match {}",
            account.game_name, account.tag_line, new_match_id
        );
        self.store_new_match_id(account.puuid.clone(), new_match_id.clone())
            .await;

        let Some(match_data) = self
            .fetch_match_data(new_match_id, account.region.clone())
            .await
        else {
            return;
        };

        if self.start_time > match_data.info.game_creation {
            debug!(
                "🗑️ [POLL] {}#{} old match ignored",
                account.game_name, account.tag_line
            );
            return;
        }

        self.dispatch_alert_if_needed(account, match_data).await;
    }

    async fn dispatch_alert_if_needed(&self, account: Account, match_data: MatchDto) {
        match match_data.queue_type() {
            QueueType::SoloDuo => {
                let cached_league_points = account.cached_league_points;
                let league = self
                    .get_ranked_solo_duo_league(account.puuid.clone(), account.region)
                    .await;

                if let Some(x) = &league {
                    debug!(
                        "⬆️ [POLL] updating league points to {} for {}#{}",
                        x.league_points, account.game_name, account.tag_line
                    );
                    self.update_league_points(
                        account.puuid.clone(),
                        QueueType::SoloDuo,
                        x.league_points,
                    )
                    .await;
                } else {
                    warn!("⚠️ [POLL] league data missing");
                }

                debug!(
                    "📢 [POLL] dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_sender
                    .dispatch_alert(
                        &account.puuid,
                        MatchDtoWithLeagueInfo::new(match_data, league, cached_league_points),
                    )
                    .await;
            }
            QueueType::NormalDraft | QueueType::Aram => {
                debug!(
                    "📢 [POLL] dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_sender
                    .dispatch_alert(
                        &account.puuid,
                        MatchDtoWithLeagueInfo::new(match_data, None, None),
                    )
                    .await;
            }
            QueueType::Unhandled => {
                debug!(
                    "❌ [POLL] {}#{} unsupported queue ID: {}",
                    account.game_name, account.tag_line, match_data.info.queue_id
                );
            }
        }
    }

    async fn fetch_new_match_id(&self, puuid: String, region: Region) -> Option<String> {
        let request = self.lol_api.match_v5.get_last_match_id(puuid, region).await;
        match request {
            Ok(maybe_id) => maybe_id,
            Err(e) => {
                error!("Riot API error while fetching last match id: {:?}", e);
                None
            }
        }
    }

    async fn store_new_match_id(&self, puuid: String, match_id: String) {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .db_sender
            .send(DbRequest::SetLastMatchId {
                puuid,
                match_id,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send DB request: {}", e);
            return;
        }
        let _ = rx.await;
    }

    async fn update_league_points(
        &self,
        puuid: String,
        queue_type: QueueType,
        league_points: LeaguePoints,
    ) {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self
            .db_sender
            .send(DbRequest::UpdateLeaguePoints {
                puuid,
                queue_type,
                league_points,
                respond_to: tx,
            })
            .await
        {
            error!("Failed to send DB request: {}", e);
            return;
        }
        let _ = rx.await;
    }

    async fn fetch_match_data(&self, match_id: String, region: Region) -> Option<MatchDto> {
        let request = self.lol_api.match_v5.get_match(match_id, region).await;
        match request {
            Ok(m) => Some(m),
            Err(e) => {
                error!("Riot API error while fetching last match: {:?}", e);
                None
            }
        }
    }

    async fn get_ranked_solo_duo_league(
        &self,
        puuid: String,
        region: Region,
    ) -> Option<LeagueEntryDto> {
        let request = self.lol_api.league_v4.get_leagues(puuid, region).await;
        let leagues = match request {
            Ok(l) => l,
            Err(e) => {
                error!("Riot API error while fetching last match: {:?}", e);
                None
            }?,
        };

        leagues
            .into_iter()
            .find(|league| league.is_ranked_solo_duo())
    }
}
