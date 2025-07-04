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

use tentrackule_alert::alert_dispatcher::DiscordAlertDispatcher;
use tentrackule_shared::{
    Account,
    traits::{
        CachedAccountSource, CachedSourceError,
        api::{ApiError, MatchApi},
    },
};
use tracing::{debug, error, info, warn};

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
    fn puuid_of(account: &Account) -> String;
}

#[async_trait]
pub trait OnNewMatch<Api, Match> {
    fn alert_dispatcher(&self) -> &DiscordAlertDispatcher<SharedDatabase>;

    async fn process_new_match(
        &self,
        match_data: Match,
        account: &Account,
    ) -> Result<(), ResultPollerError>;
}

pub struct ResultPoller<Api, Match> {
    cache: SharedDatabase,
    api: Arc<Api>,
    pub alert_dispatcher: DiscordAlertDispatcher<SharedDatabase>,
    start_time: u128,
    poll_interval: Duration,
    marker: PhantomData<Match>,
}

impl<Api, Match> ResultPoller<Api, Match>
where
    Self: 'static + OnNewMatch<Api, Match> + WithPuuid,
    Api: MatchApi<Match>,
    Match: MatchCreationTime + Send + Sync,
{
    pub fn new(
        api: Arc<Api>,
        cache: SharedDatabase,
        alert_dispatcher: DiscordAlertDispatcher<SharedDatabase>,
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
            marker: Default::default(),
        }
    }

    async fn poll_once(&self) {
        info!("🔄 starting fetch cycle");

        let accounts = match self.cache.get_all_accounts().await {
            Ok(accounts) => accounts,
            Err(e) => {
                error!("Database error while fetching accounts: {}", e);
                Vec::new()
            }
        };
        stream::iter(accounts)
            .for_each_concurrent(10, |account| async move {
                if let Err(e) = self.process_account(&account).await {
                    error!(
                        "Processing account of {}#{} exited with error: {e}",
                        account.game_name, account.tag_line
                    )
                }
            })
            .await;
    }

    async fn process_account(&self, account: &Account) -> Result<(), ResultPollerError>
    where
        Self: WithPuuid,
    {
        debug!("checking {}#{}", account.game_name, account.tag_line);
        let last_match_id = match self
            .api
            .get_last_match_id(Self::puuid_of(account).clone(), account.region)
            .await
            .map_err(ResultPollerError::RiotApiError)?
        {
            Some(id) => id,
            None => {
                warn!("No last match ID found from the API.");
                return Ok(());
            }
        };

        if last_match_id == account.last_match_id {
            debug!("{}#{} no new result", account.game_name, account.tag_line);
            return Ok(());
        }

        debug!(
            "Detected newer match ID on Riot servers, caching new match {}",
            last_match_id
        );
        self.cache
            .set_last_match_id(Self::puuid_of(account).clone(), last_match_id.clone())
            .await
            .map_err(ResultPollerError::CacheError)?;

        let match_data = self
            .api
            .get_match(last_match_id, account.region)
            .await
            .map_err(ResultPollerError::RiotApiError)?;

        if self.start_time > match_data.game_creation() {
            debug!("This is an old match, alerting ignored.",);
            return Ok(());
        }

        self.process_new_match(match_data, account).await
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("Poller started");

            let mut interval = tokio::time::interval(self.poll_interval);

            loop {
                interval.tick().await;
                self.poll_once().await
            }
        })
    }
}
