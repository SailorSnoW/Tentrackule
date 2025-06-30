//! Periodically polls Riot servers for new match results and sends results to an alert dispatcher.

use futures::{stream, StreamExt};
use std::{
    env,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tentrackule_bot::AlertDispatch;
use tentrackule_riot_api::api::{types::MatchDto, LolApiFull};
use tentrackule_types::{
    lol_match::Match,
    traits::{CachedAccountSource, CachedLeagueSource},
    Account, CachedLeague, QueueType, Region,
};
use tracing::{debug, error, info};

/// Poller responsible for automatically fetching new results of tracked player from Riot servers, parsing results data and sending it to the discord receiver when alerting is needed.
pub struct ResultPoller<C, LA, AD> {
    lol_api: Arc<LA>,
    db: C,
    alert_dispatcher: AD,
    start_time: u128,
    poll_interval: Duration,
}

impl<C, LA, AD> ResultPoller<C, LA, AD>
where
    C: CachedAccountSource + CachedLeagueSource + Sync + Send + 'static,
    LA: LolApiFull + Send + Sync + 'static,
    AD: AlertDispatch + Sync + Send + 'static,
{
    pub fn new(lol_api: Arc<LA>, db: C, alert_dispatcher: AD) -> Self {
        let poll_interval_u64 = env::var("POLL_INTERVAL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        let poll_interval = Duration::from_secs(poll_interval_u64);

        Self {
            lol_api,
            db,
            alert_dispatcher,
            start_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis(),
            poll_interval,
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(&self) {
        info!("poller started");

        let mut interval = tokio::time::interval(self.poll_interval);

        loop {
            interval.tick().await;
            self.poll_once().await;
        }
    }

    async fn poll_once(&self) {
        info!("🔄 starting fetch cycle");

        let accounts = self.get_all_accounts().await;
        stream::iter(accounts)
            .for_each_concurrent(10, |account| async move {
                self.process_account(account).await;
            })
            .await;
    }

    async fn get_all_accounts(&self) -> Vec<Account> {
        match self.db.get_all_accounts().await {
            Ok(accounts) => accounts,
            Err(e) => {
                error!("Database error while fetching accounts: {}", e);
                Vec::new()
            }
        }
    }

    async fn process_account(&self, account: Account) {
        debug!("checking {}#{}", account.game_name, account.tag_line);
        let Some(new_match_id) = self
            .fetch_new_match_id(account.puuid.clone(), account.region)
            .await
        else {
            return;
        };

        if new_match_id == account.last_match_id {
            debug!("{}#{} no new result", account.game_name, account.tag_line);
            return;
        }

        debug!(
            "{}#{} caching match {}",
            account.game_name, account.tag_line, new_match_id
        );
        self.store_new_match_id(account.puuid.clone(), new_match_id.clone())
            .await;

        let Some(match_data) = self.fetch_match_data(new_match_id, account.region).await else {
            return;
        };

        if self.start_time > match_data.info.game_creation {
            debug!(
                "{}#{} old match ignored",
                account.game_name, account.tag_line
            );
            return;
        }

        self.dispatch_alert_if_needed(account, match_data.into())
            .await;
    }

    async fn dispatch_alert_if_needed(&self, account: Account, match_data: Match) {
        match match_data.queue_type() {
            QueueType::SoloDuo => {
                let match_ranked = match match_data
                    .try_into_match_ranked(&account, self.lol_api.clone(), &self.db)
                    .await
                {
                    Ok(data) => data,
                    Err(e) => {
                        error!("conversion of match into a ranked match failed: {}", e);
                        return;
                    }
                };

                debug!(
                    "updating league to {} for {}#{}",
                    match_ranked.current_league.league_points, account.game_name, account.tag_line
                );
                self.set_cached_league_for(
                    account.puuid.clone(),
                    QueueType::SoloDuo,
                    match_ranked.current_league.clone().into(),
                )
                .await;

                debug!(
                    "dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_dispatcher
                    .dispatch_alert(&account.puuid, match_ranked)
                    .await;
            }
            QueueType::Flex => {
                let match_ranked = match match_data
                    .try_into_match_ranked(&account, self.lol_api.clone(), &self.db)
                    .await
                {
                    Ok(data) => data,
                    Err(e) => {
                        error!("conversion of match into a ranked match failed: {}", e);
                        return;
                    }
                };

                debug!(
                    "updating league to {} for {}#{}",
                    match_ranked.current_league.league_points, account.game_name, account.tag_line
                );
                self.set_cached_league_for(
                    account.puuid.clone(),
                    QueueType::Flex,
                    match_ranked.current_league.clone().into(),
                )
                .await;

                debug!(
                    "dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_dispatcher
                    .dispatch_alert(&account.puuid, match_ranked)
                    .await;
            }
            QueueType::NormalDraft | QueueType::Aram => {
                debug!(
                    "dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_dispatcher
                    .dispatch_alert(&account.puuid, match_data)
                    .await;
            }
            QueueType::Unhandled => {
                debug!(
                    "{}#{} unsupported queue ID: {}",
                    account.game_name, account.tag_line, match_data.queue_id
                );
            }
        }
    }

    async fn fetch_new_match_id(&self, puuid: String, region: Region) -> Option<String> {
        let request = self.lol_api.get_last_match_id(puuid, region).await;
        match request {
            Ok(maybe_id) => maybe_id,
            Err(e) => {
                error!("Riot API error while fetching last match id: {:?}", e);
                None
            }
        }
    }

    async fn store_new_match_id(&self, puuid: String, match_id: String) {
        if let Err(e) = self.db.set_last_match_id(puuid, match_id).await {
            error!("Failed to send DB request: {}", e);
        }
    }

    async fn set_cached_league_for(
        &self,
        puuid: String,
        queue_type: QueueType,
        league: CachedLeague,
    ) {
        if let Err(e) = self.db.set_league_for(puuid, queue_type, league).await {
            error!("Failed to send DB request: {}", e);
        }
    }

    async fn fetch_match_data(&self, match_id: String, region: Region) -> Option<MatchDto> {
        let request = self.lol_api.get_match(match_id, region).await;
        match request {
            Ok(m) => Some(m),
            Err(e) => {
                error!("Riot API error while fetching last match: {:?}", e);
                None
            }
        }
    }
}
