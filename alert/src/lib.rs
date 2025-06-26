//! Utilities to convert match data into Discord alerts.
//!
//! This crate exposes types used to build [`CreateEmbed`] structures that can be
//! sent by the Discord bot when a tracked game finishes.

use poise::serenity_prelude::CreateEmbed;
use thiserror::Error;

pub mod lol;

/// Errors that can occur while creating an alert message.
#[derive(Error, Debug)]
pub enum AlertCreationError {
    #[error("The specified PUUID focus {puuid:?} isn't part of the match (likely unexpected !).")]
    PuuidNotInMatch { puuid: String },
    #[error("Tried to convert an unsupported queue ID into an Alert: {queue_id}.")]
    UnsupportedQueueType { queue_id: u16 },
}

/// Convenience alias for a fully built embed representing the alert.
pub type Alert = CreateEmbed;

/// Types implementing this trait can produce an [`Alert`] for a given player.
pub trait TryIntoAlert {
    /// Convert the value into an [`Alert`].
    fn try_into_alert(&self, puuid_focus: &str) -> Result<Alert, AlertCreationError>;
}
