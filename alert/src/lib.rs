//! Utilities to convert match data into Discord alerts.
//!
//! This crate exposes types used to build [`CreateEmbed`] structures that can be
//! sent by the Discord bot when a tracked game finishes.

use poise::serenity_prelude::CreateEmbed;
use tentrackule_shared::{QueueType, lol_match};
use thiserror::Error;

pub mod alert_dispatcher;
pub mod lol;
pub mod message_sender;

pub use alert_dispatcher::{AlertDispatch, AlertDispatcher};
pub use message_sender::MessageSender;

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

/// Types that expose the queue type associated with them.
pub trait QueueTyped {
    fn queue_type(&self) -> QueueType;
}

impl QueueTyped for lol_match::Match {
    fn queue_type(&self) -> QueueType {
        tentrackule_shared::lol_match::Match::queue_type(self)
    }
}

impl QueueTyped for lol_match::MatchRanked {
    fn queue_type(&self) -> QueueType {
        self.base.queue_type()
    }
}
