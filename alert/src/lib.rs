//! Utilities to convert match data into Discord alerts.
//!
//! This crate exposes types used to build [`CreateEmbed`] structures that can be
//! sent by the Discord bot when a tracked game finishes.

use poise::serenity_prelude::CreateEmbed;
use tentrackule_shared::{lol_match, tft_match, traits::QueueKind};
use thiserror::Error;

pub mod alert_dispatcher;
pub mod lol;
pub mod message_sender;
pub mod tft;

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
pub trait QueueTyped<T: ToString + QueueKind + Send + Sync> {
    fn queue_type(&self) -> T;
}

impl QueueTyped<lol_match::QueueType> for lol_match::Match {
    fn queue_type(&self) -> lol_match::QueueType {
        tentrackule_shared::lol_match::Match::queue_type(self)
    }
}

impl QueueTyped<lol_match::QueueType> for lol_match::MatchRanked {
    fn queue_type(&self) -> lol_match::QueueType {
        self.base.queue_type()
    }
}

impl QueueTyped<tft_match::QueueType> for tft_match::Match {
    fn queue_type(&self) -> tft_match::QueueType {
        self.queue_type()
    }
}
