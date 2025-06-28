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
}

/// A call to Riot API can either result in a success with the success type or fail with a [`RiotApiError`].
pub type RiotApiResponse<T> = Result<T, RiotApiError>;
