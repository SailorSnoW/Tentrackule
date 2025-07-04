use async_trait::async_trait;
use tentrackule_alert::{AlertDispatch, alert_dispatcher::DiscordAlertDispatcher};
use tentrackule_db::SharedDatabase;
use tentrackule_riot_api::api::tft::TftApiClient;
use tentrackule_shared::traits::{CachedAccountSource, CachedLeagueSource, CachedSourceError};
use tentrackule_shared::{
    Account,
    tft_match::{Match, QueueType},
};
use tracing::{debug, error};

use crate::{
    MatchCreationTime, OnNewMatch, ResultPoller, ResultPollerError, WithLastMatchId, WithPuuid,
};

pub type TftResultPoller = ResultPoller<TftApiClient, Match>;

impl WithPuuid for TftResultPoller {
    fn puuid_of(account: &Account) -> Option<String> {
        account.puuid_tft.clone()
    }
}

#[async_trait]
impl WithLastMatchId for TftResultPoller {
    fn cache(&self) -> SharedDatabase {
        self.cache.clone()
    }

    fn last_match_id(account: &Account) -> Option<String> {
        account.last_match_id_tft.clone()
    }

    async fn set_last_match_id(
        &self,
        account: &Account,
        match_id: String,
    ) -> Result<(), CachedSourceError> {
        self.cache()
            .set_last_match_id_tft(account.id, match_id)
            .await
    }
}

#[async_trait]
impl OnNewMatch<TftApiClient, Match> for TftResultPoller {
    fn alert_dispatcher(&self) -> &DiscordAlertDispatcher<SharedDatabase> {
        &self.alert_dispatcher
    }

    async fn process_new_match(
        &self,
        match_data: Match,
        account: &Account,
    ) -> Result<(), ResultPollerError> {
        match match_data.queue_type() {
            QueueType::Normal => {
                self.alert_dispatcher()
                    .dispatch_alert(account, match_data)
                    .await;
                Ok(())
            }
            QueueType::Ranked => {
                let match_ranked = match match_data
                    .try_into_match_ranked::<TftApiClient, SharedDatabase>(
                        account,
                        self.api.clone(),
                        &self.cache,
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
                self.alert_dispatcher()
                    .dispatch_alert(account, match_ranked)
                    .await;
                Ok(())
            }
            QueueType::Unhandled => Ok(()),
        }
    }
}

impl MatchCreationTime for Match {
    fn game_creation(&self) -> u128 {
        self.info.game_creation
    }
}
