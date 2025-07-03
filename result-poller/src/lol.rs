use async_trait::async_trait;
use tentrackule_alert::{AlertDispatch, alert_dispatcher::DiscordAlertDispatcher};
use tentrackule_db::SharedDatabase;
use tentrackule_riot_api::api::lol::LolApiClient;
use tentrackule_shared::traits::CachedLeagueSource;
use tentrackule_shared::{
    Account,
    lol_match::{Match, QueueType},
};
use tracing::{debug, error};

use crate::{MatchCreationTime, OnNewMatch, ResultPoller, ResultPollerError};

pub type LolResultPoller = ResultPoller<LolApiClient, Match>;

#[async_trait]
impl OnNewMatch<LolApiClient, Match> for LolResultPoller {
    fn alert_dispatcher(&self) -> &DiscordAlertDispatcher<SharedDatabase> {
        &self.alert_dispatcher
    }

    async fn process_new_match(
        &self,
        match_data: Match,
        account: &Account,
    ) -> Result<(), ResultPollerError> {
        match match_data.queue_type() {
            QueueType::SoloDuo | QueueType::Flex => {
                let match_ranked = match match_data
                    .try_into_match_ranked::<LolApiClient, SharedDatabase>(
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
                debug!(
                    "updating league to {:?} for {}#{}",
                    match_ranked.current_league, account.game_name, account.tag_line
                );
                self.cache
                    .set_league_for(account.puuid.clone(), match_ranked.current_league.clone())
                    .await
                    .map_err(ResultPollerError::CacheError)?;

                debug!(
                    "dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_dispatcher
                    .dispatch_alert(&account.puuid, match_ranked)
                    .await;

                Ok(())
            }
            QueueType::NormalDraft | QueueType::Aram => {
                debug!(
                    "dispatching alert for {}#{}",
                    account.game_name, account.tag_line
                );
                self.alert_dispatcher
                    .dispatch_alert(&account.puuid, match_data)
                    .await;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl MatchCreationTime for Match {
    fn game_creation(&self) -> u128 {
        self.game_creation
    }
}
