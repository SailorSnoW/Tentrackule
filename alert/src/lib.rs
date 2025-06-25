use poise::serenity_prelude::CreateEmbed;
use thiserror::Error;

pub mod lol;

#[derive(Error, Debug)]
pub enum AlertCreationError {
    #[error("The specified PUUID focus {puuid:?} isn't part of the match (likely unexpected !).")]
    PuuidNotInMatch { puuid: String },
    #[error("Tried to convert an unsupported queue ID into an Alert: {queue_id}.")]
    UnsupportedQueueType { queue_id: u16 },
}

pub type Alert = CreateEmbed;

pub trait TryIntoAlert {
    fn try_into_alert(&self, puuid_focus: &str) -> Result<Alert, AlertCreationError>;
}
