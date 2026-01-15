use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Riot API error: {status} - {message}")]
    RiotApi { status: u16, message: String },

    #[error("Discord error: {0}")]
    Discord(Box<serenity::Error>),

    #[error("HTTP request error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Player not found: {game_name}#{tag_line}")]
    PlayerNotFound { game_name: String, tag_line: String },

    #[error("Invalid region: {0}")]
    InvalidRegion(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Player not tracked in this server")]
    PlayerNotTracked,

    #[error("Image generation error: {message}")]
    ImageGeneration { message: String },
}

impl From<serenity::Error> for AppError {
    fn from(err: serenity::Error) -> Self {
        AppError::Discord(Box::new(err))
    }
}
