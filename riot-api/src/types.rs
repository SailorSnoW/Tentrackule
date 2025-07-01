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
