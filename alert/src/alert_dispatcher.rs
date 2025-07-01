use std::{collections::HashMap, sync::Arc};

use super::TryIntoAlert;
use async_trait::async_trait;
use message_sender::MessageSender;
use poise::serenity_prelude::{ChannelId, CreateMessage, GuildId, Http};
use tentrackule_shared::traits::CachedAccountGuildSource;
use tracing::{error, warn};

use super::*;

/// Abstraction for dispatching alert messages to Discord.
#[async_trait]
pub trait AlertDispatch {
    async fn dispatch_alert<T>(&self, puuid: &str, match_data: T)
    where
        T: TryIntoAlert + Send + Sync;
}

/// An AlertDispatcher which use a discord Http client to send alerts.
pub type DiscordAlertDispatcher<Cache> = AlertDispatcher<Arc<Http>, Cache>;

/// Implementation of [`AlertDispatch`] using a [`MessageSender`] and the database.
pub struct AlertDispatcher<S, C> {
    sender: Arc<S>,
    db: C,
}

impl<S, C> AlertDispatcher<S, C>
where
    C: CachedAccountGuildSource,
{
    /// Create a new dispatcher using the given message sender and database handle.
    pub fn new(sender: Arc<S>, db: C) -> Self {
        Self { sender, db }
    }

    /// Retrieve the list of guilds tracking the specified player along with
    /// their configured alert channel, if any.
    async fn get_guilds_for_account(&self, puuid: String) -> HashMap<GuildId, Option<ChannelId>> {
        match self.db.get_guilds_for(puuid).await {
            Ok(x) => x,
            Err(e) => {
                error!("DB error while getting guilds for account: {}", e);
                HashMap::new()
            }
        }
    }
}

#[async_trait]
impl<S, C> AlertDispatch for AlertDispatcher<S, C>
where
    S: MessageSender,
    C: CachedAccountGuildSource + Send + Sync,
{
    async fn dispatch_alert<T>(&self, puuid: &str, match_data: T)
    where
        T: TryIntoAlert + Send + Sync,
    {
        let alert = match match_data.try_into_alert(puuid) {
            Ok(alert) => alert,
            Err(reason) => {
                error!("failed to build alert: {}", reason);
                return;
            }
        };

        // First, we get all the guilds where the player is tracked with channel ID where to send
        // the alert.
        let guilds = self.get_guilds_for_account(puuid.to_string()).await;

        for guild in guilds {
            let maybe_channel_id = guild.1;
            match maybe_channel_id {
                Some(channel_id) => {
                    if let Err(e) = self
                        .sender
                        .send_message(channel_id, CreateMessage::new().embed(alert.clone()))
                        .await
                    {
                        error!("failed to send message: {}", e)
                    }
                }
                None => {
                    warn!("guild {} has no alert channel, skipping", guild.0);
                    continue;
                }
            }
        }
    }
}
