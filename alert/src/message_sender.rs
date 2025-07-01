//! Abstraction used by the bot to send messages.

use std::sync::Arc;

use async_trait::async_trait;
use poise::serenity_prelude::{self as serenity, ChannelId, CreateMessage};

#[async_trait]
/// A way to send a message (likely containing an Alert for example)
pub trait MessageSender: Send + Sync {
    async fn send_message(&self, channel_id: ChannelId, msg: CreateMessage)
        -> serenity::Result<()>;
}

#[async_trait]
impl MessageSender for Arc<serenity::Http> {
    async fn send_message(
        &self,
        channel_id: ChannelId,
        msg: CreateMessage,
    ) -> serenity::Result<()> {
        channel_id.send_message(self, msg).await.map(|_| ())
    }
}
