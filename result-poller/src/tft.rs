use async_trait::async_trait;
use tentrackule_alert::{AlertDispatch, alert_dispatcher::DiscordAlertDispatcher};
use tentrackule_db::SharedDatabase;
use tentrackule_riot_api::api::tft::TftApiClient;
use tentrackule_shared::{
    Account,
    tft_match::{Match, QueueType},
};

use crate::{MatchCreationTime, OnNewMatch, ResultPoller, ResultPollerError, WithPuuid};

pub type TftResultPoller = ResultPoller<TftApiClient, Match>;

impl WithPuuid for TftResultPoller {
    fn puuid_of(account: &Account) -> String {
        account.puuid_tft.clone().unwrap_or_default()
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
            QueueType::Unhandled => Ok(()),
        }
    }
}

impl MatchCreationTime for Match {
    fn game_creation(&self) -> u128 {
        self.info.game_creation
    }
}
