use async_trait::async_trait;
use tentrackule_db::SharedDatabase;
use tentrackule_riot_api::api::lol::LolApiClient;
use tentrackule_shared::traits::{CachedAccountSource, CachedSourceError};
use tentrackule_shared::{Account, lol_match::Match};

use crate::{MatchCreationTime, ResultPoller, WithLastMatchId, WithPuuid};

pub type LolResultPoller = ResultPoller<LolApiClient, Match>;

impl WithPuuid for LolResultPoller {
    fn puuid_of(account: &Account) -> Option<String> {
        account.puuid.clone()
    }
}

#[async_trait]
impl WithLastMatchId for LolResultPoller {
    fn cache(&self) -> SharedDatabase {
        self.cache.clone()
    }

    fn last_match_id(account: &Account) -> Option<String> {
        account.last_match_id.clone()
    }

    async fn set_last_match_id(
        &self,
        account: &Account,
        match_id: String,
    ) -> Result<(), CachedSourceError> {
        self.cache().set_last_match_id(account.id, match_id).await
    }
}

impl MatchCreationTime for Match {
    fn game_creation(&self) -> u128 {
        self.game_creation
    }
}
