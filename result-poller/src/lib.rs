use async_trait::async_trait;
use futures::{StreamExt, stream};
use std::{
    env,
    marker::PhantomData,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tentrackule_db::SharedDatabase;
use thiserror::Error;

use tentrackule_alert::{AlertDispatch, TryIntoAlert, alert_dispatcher::DiscordAlertDispatcher};
use tentrackule_shared::{
    Account, QueueTyped,
    lol_match::MatchRanked,
    traits::{
        CachedAccountSource, CachedLeagueSource, CachedSourceError, QueueKind,
        api::{ApiError, LeagueApi, MatchApi},
    },
};
use tracing::{Instrument, debug, error, info, info_span, trace, warn};

#[macro_use]
mod macros;

pub mod lol;
pub mod tft;

#[derive(Debug, Error)]
pub enum ResultPollerError {
    #[error("An error occured during a request to the Riot API: {0}")]
    RiotApiError(ApiError),
    #[error("An error occured during a local cache operation: {0}")]
    CacheError(CachedSourceError),
}

pub trait MatchCreationTime {
    fn game_creation(&self) -> u128;
}

pub trait WithPuuid {
    fn puuid_of(account: &Account) -> Option<String>;
}

#[async_trait]
pub trait WithLastMatchId {
    fn cache(&self) -> SharedDatabase;

    fn last_match_id(account: &Account) -> Option<String>;
    async fn set_last_match_id(
        &self,
        account_id: &Account,
        match_id: String,
    ) -> Result<(), CachedSourceError>;
}

pub struct ResultPoller<Api, Match> {
    cache: SharedDatabase,
    api: Arc<Api>,
    pub alert_dispatcher: DiscordAlertDispatcher<SharedDatabase>,
    start_time: u128,
    poll_interval: Duration,
    name: &'static str,
    marker: PhantomData<Match>,
}

impl<Api, Match> ResultPoller<Api, Match>
where
    Self: 'static + WithPuuid + WithLastMatchId,
    Api: MatchApi<Match> + LeagueApi,
    Match: TryIntoAlert + MatchCreationTime + QueueTyped + Clone + Send + Sync,
    MatchRanked<Match>: TryIntoAlert + QueueTyped,
{
    pub fn new(
        api: Arc<Api>,
        cache: SharedDatabase,
        alert_dispatcher: DiscordAlertDispatcher<SharedDatabase>,
        name: &'static str,
    ) -> Self {
        let poll_interval_u64 = env::var("POLL_INTERVAL_SECONDS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        let poll_interval = Duration::from_secs(poll_interval_u64);

        Self {
            api,
            cache,
            start_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards")
                .as_millis(),
            poll_interval,
            alert_dispatcher,
            name,
            marker: Default::default(),
        }
    }

    async fn poll_once(&self) {
        info!("starting fetch cycle");

        let accounts = match self.cache.get_all_accounts().await {
            Ok(accounts) => accounts,
            Err(e) => {
                error!("Database error while fetching accounts: {}", e);
                Vec::new()
            }
        };
        stream::iter(accounts)
            .for_each_concurrent(10, |account| {
                let span = info_span!(
                    "",
                    user = %format!("{}#{}", account.game_name, account.tag_line)
                );
                async move {
                    if let Err(e) = self.process_account(&account).await {
                        error!("Processing account exited with error: {e}");
                    }
                }
                .instrument(span)
            })
            .await;
    }

    async fn process_account(&self, account: &Account) -> Result<(), ResultPollerError>
    where
        Self: WithPuuid,
    {
        info!("Processing account...");
        let puuid = match Self::puuid_of(account).clone() {
            Some(x) => {
                if x == String::new() {
                    return Ok(());
                } else {
                    x
                }
            }
            None => {
                warn!(
                    poller = self.name,
                    account = %format!("{}#{}", account.game_name, account.tag_line),
                    "Player doesn't have a cached puuid, ignoring."
                );
                return Ok(());
            }
        };

        debug!("Fetching most recent match ID");
        let last_match_id = match self
            .api
            .get_last_match_id(puuid.clone(), account.region)
            .await
            .map_err(ResultPollerError::RiotApiError)?
        {
            Some(id) => {
                debug!("Most recent match ID: {}", id);
                id
            }
            None => {
                warn!("No last match ID found from the API.");
                return Ok(());
            }
        };

        trace!(
            "Comparing fetched match ID {} with cached match ID {}",
            last_match_id,
            Self::last_match_id(account).unwrap_or_default()
        );
        if last_match_id == Self::last_match_id(account).unwrap_or_default() {
            debug!("No new match detected, ignoring.");
            return Ok(());
        }

        debug!(new_match_id = %last_match_id, "Detected newer match ID on Riot servers, caching new match");
        self.set_last_match_id(account, last_match_id.clone())
            .await
            .map_err(ResultPollerError::CacheError)?;

        let match_data = self
            .api
            .get_match(last_match_id, account.region)
            .await
            .map_err(ResultPollerError::RiotApiError)?;

        if self.start_time > match_data.game_creation() {
            debug!("This is an old match, alerting ignored.");
            return Ok(());
        }

        self.process_new_match(match_data, account).await
    }

    async fn process_new_match(
        &self,
        match_data: Match,
        account: &Account,
    ) -> Result<(), ResultPollerError> {
        match match_data.clone().queue_type() {
            // Normal games when we don't need enriched ranked data
            x if !x.is_ranked() => {
                debug!("dispatching alert");
                self.alert_dispatcher
                    .dispatch_alert(account, match_data)
                    .await;
                Ok(())
            }
            // Ranked game where we need enriched ranked data from cached + API leagues
            // data
            x if x.is_ranked() => {
                let match_ranked = match MatchRanked::from_match(
                    &match_data,
                    account,
                    self.cache.clone(),
                    self.api.clone(),
                )
                .await
                {
                    Ok(data) => data,
                    Err(e) => {
                        error!("conversion of match into a ranked match failed: {}", e);
                        return Ok(());
                    }
                };
                debug!(league = ?match_ranked.current_league, "updating league");
                self.cache
                    .set_league_for(account.id, match_ranked.current_league.clone())
                    .await
                    .map_err(ResultPollerError::CacheError)?;

                debug!("dispatching alert");
                self.alert_dispatcher
                    .dispatch_alert(account, match_ranked)
                    .await;

                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let span = info_span!("📡 ", poller = self.name);
        tokio::spawn(
            async move {
                info!("Poller started");

                let mut interval = tokio::time::interval(self.poll_interval);
                interval.tick().await;

                loop {
                    interval.tick().await;
                    self.poll_once().await
                }
            }
            .instrument(span),
        )
    }
}
