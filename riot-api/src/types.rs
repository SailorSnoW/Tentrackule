use std::error::Error;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RiotApiError {
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("HTTP status error: {0}")]
    Status(reqwest::StatusCode),

    #[error("Decoding raw response error: {0}")]
    Serde(serde_json::Error),
}

#[derive(Debug, Error)]
pub enum RiotMatchError {
    #[error("The request account puuid is not part of the match")]
    PuuidNotInMatch,
    #[error("an error occured while fetching the cached league.")]
    CantRetrieveCachedLeague(Box<dyn Error>),
    #[error("No cached league is existing for puuid: {0} with queue_id: {1}.")]
    NoCachedLeagueFound(String, u16),
    #[error("No {0} league found from the Riot API for puuid: {1}")]
    NoApiLeagueFound(String, String),
    #[error("An error occured during an API operation: {0}")]
    RiotApiError(RiotApiError),
}

/// A call to Riot API can either result in a success with the success type or fail with a [`RiotApiError`].
pub type RiotApiResponse<T> = Result<T, RiotApiError>;
