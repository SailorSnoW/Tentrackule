use std::error::Error;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RiotMatchError {
    #[error("The request account puuid is not part of the match")]
    PuuidNotInMatch,
    #[error("an error occured while fetching the cached league: {0}.")]
    CantRetrieveCachedLeague(Box<dyn Error>),
    #[error("No {0} league found from the Riot API for puuid: {1}")]
    NoApiLeagueFound(String, String),
    #[error("An error occured during an API operation: {0}")]
    RiotApiError(Box<dyn Error>),
}
