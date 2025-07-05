use tentrackule_riot_api::api::tft::TftApiClient;
use tentrackule_shared::tft_match::Match;
use tentrackule_shared::traits::CachedAccountSource;

use crate::{MatchCreationTime, ResultPoller};

pub type TftResultPoller = ResultPoller<TftApiClient, Match>;

impl_result_poller_traits!(
    TftResultPoller,
    puuid_tft,
    last_match_id_tft,
    set_last_match_id_tft
);

impl MatchCreationTime for Match {
    fn game_creation(&self) -> u128 {
        self.info.game_creation
    }
}
