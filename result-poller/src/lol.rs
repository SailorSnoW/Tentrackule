use tentrackule_riot_api::api::lol::LolApiClient;
use tentrackule_shared::lol_match::Match;
use tentrackule_shared::traits::CachedAccountSource;

use crate::{MatchCreationTime, ResultPoller};

pub type LolResultPoller = ResultPoller<LolApiClient, Match>;

impl_result_poller_traits!(LolResultPoller, puuid, last_match_id, set_last_match_id);

impl MatchCreationTime for Match {
    fn game_creation(&self) -> u128 {
        self.game_creation
    }
}
