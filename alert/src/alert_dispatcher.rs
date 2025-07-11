use std::{collections::HashMap, sync::Arc};

use super::TryIntoAlert;
use async_trait::async_trait;
use message_sender::MessageSender;
use poise::serenity_prelude::{ChannelId, CreateMessage, GuildId, Http};
use tentrackule_shared::traits::{CachedAccountGuildSource, CachedSettingSource};
use tracing::{error, warn};

use super::*;

/// Abstraction for dispatching alert messages to Discord.
#[async_trait]
pub trait AlertDispatch {
    async fn dispatch_alert<T>(&self, puuid: &str, match_data: T)
    where
        T: TryIntoAlert + QueueTyped + Send + Sync;
}

/// An AlertDispatcher which use a discord Http client to send alerts.
pub type DiscordAlertDispatcher<Cache> = AlertDispatcher<Arc<Http>, Cache>;

/// Implementation of [`AlertDispatch`] using a [`MessageSender`] and the database.
pub struct AlertDispatcher<S, C> {
    sender: S,
    db: C,
}

impl<S, C> AlertDispatcher<S, C>
where
    C: CachedAccountGuildSource,
{
    /// Create a new dispatcher using the given message sender and database handle.
    pub fn new(sender: S, db: C) -> Self {
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
    C: CachedAccountGuildSource + CachedSettingSource + Send + Sync,
{
    async fn dispatch_alert<T>(&self, puuid: &str, match_data: T)
    where
        T: TryIntoAlert + QueueTyped + Send + Sync,
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

        let queue_type = match_data.queue_type();

        for guild in guilds {
            let maybe_channel_id = guild.1;

            // Enabled or disabled alert for the match queue type check
            let enabled = match self.db.is_queue_alert_enabled(guild.0, queue_type).await {
                Ok(v) => v,
                Err(e) => {
                    error!("DB error while checking queue alert setting: {}", e);
                    true
                }
            };

            if !enabled {
                continue;
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use poise::serenity_prelude::{self as serenity};
    use std::sync::{Arc, Mutex};
    use tentrackule_shared::{Account, QueueType, traits::CachedSourceError};

    struct DummySender {
        pub sent: Arc<Mutex<Vec<(ChannelId, String)>>>,
        pub fail: bool,
    }

    #[async_trait]
    impl MessageSender for DummySender {
        async fn send_message(
            &self,
            channel_id: ChannelId,
            msg: CreateMessage,
        ) -> serenity::Result<()> {
            if self.fail {
                return Err(serenity::Error::Other("fail"));
            }
            let data = serde_json::to_string(&msg).unwrap();
            self.sent.lock().unwrap().push((channel_id, data));
            Ok(())
        }
    }

    struct DummyCache {
        pub guilds: HashMap<GuildId, Option<ChannelId>>,
    }

    #[async_trait]
    impl CachedSettingSource for DummyCache {
        async fn set_alert_channel(
            &self,
            _guild_id: GuildId,
            _channel_id: ChannelId,
        ) -> Result<(), CachedSourceError> {
            Ok(())
        }

        async fn get_alert_channel(
            &self,
            _guild_id: GuildId,
        ) -> Result<Option<ChannelId>, CachedSourceError> {
            Ok(None)
        }

        async fn set_queue_alert_enabled(
            &self,
            _guild_id: GuildId,
            _queue_type: QueueType,
            _enabled: bool,
        ) -> Result<(), CachedSourceError> {
            Ok(())
        }

        async fn is_queue_alert_enabled(
            &self,
            _guild_id: GuildId,
            _queue_type: QueueType,
        ) -> Result<bool, CachedSourceError> {
            Ok(true)
        }
    }

    #[async_trait]
    impl CachedAccountGuildSource for DummyCache {
        async fn get_guilds_for(
            &self,
            _puuid: String,
        ) -> Result<HashMap<GuildId, Option<ChannelId>>, CachedSourceError> {
            Ok(self.guilds.clone())
        }

        async fn get_accounts_for(
            &self,
            _guild_id: GuildId,
        ) -> Result<Vec<Account>, CachedSourceError> {
            Ok(Vec::new())
        }
    }

    struct DummyCacheWithQueues {
        pub guilds: HashMap<GuildId, Option<ChannelId>>,
        pub enabled: HashMap<(GuildId, QueueType), bool>,
    }

    #[async_trait]
    impl CachedAccountGuildSource for DummyCacheWithQueues {
        async fn get_guilds_for(
            &self,
            _puuid: String,
        ) -> Result<HashMap<GuildId, Option<ChannelId>>, CachedSourceError> {
            Ok(self.guilds.clone())
        }

        async fn get_accounts_for(
            &self,
            _guild_id: GuildId,
        ) -> Result<Vec<Account>, CachedSourceError> {
            Ok(Vec::new())
        }
    }

    #[async_trait]
    impl CachedSettingSource for DummyCacheWithQueues {
        async fn set_alert_channel(
            &self,
            _guild_id: GuildId,
            _channel_id: ChannelId,
        ) -> Result<(), CachedSourceError> {
            Ok(())
        }

        async fn get_alert_channel(
            &self,
            _guild_id: GuildId,
        ) -> Result<Option<ChannelId>, CachedSourceError> {
            Ok(None)
        }

        async fn set_queue_alert_enabled(
            &self,
            _guild_id: GuildId,
            _queue_type: QueueType,
            _enabled: bool,
        ) -> Result<(), CachedSourceError> {
            Ok(())
        }

        async fn is_queue_alert_enabled(
            &self,
            guild_id: GuildId,
            queue_type: QueueType,
        ) -> Result<bool, CachedSourceError> {
            Ok(*self.enabled.get(&(guild_id, queue_type)).unwrap_or(&true))
        }
    }

    #[tokio::test]
    async fn dispatch_skips_disabled_queue() {
        let sender = DummySender {
            sent: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        };
        let guilds = [(GuildId::new(1), Some(ChannelId::new(10)))]
            .into_iter()
            .collect();
        let enabled = [((GuildId::new(1), QueueType::NormalDraft), false)]
            .into_iter()
            .collect();
        let cache = DummyCacheWithQueues { guilds, enabled };
        let dispatcher = AlertDispatcher::new(sender, cache);

        dispatcher.dispatch_alert("p", DummyAlert).await;

        assert!(dispatcher.sender.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dispatch_mixed_queue_settings() {
        let sender = DummySender {
            sent: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        };
        let guilds = [
            (GuildId::new(1), Some(ChannelId::new(10))),
            (GuildId::new(2), Some(ChannelId::new(20))),
        ]
        .into_iter()
        .collect();
        let enabled = [
            ((GuildId::new(1), QueueType::NormalDraft), true),
            ((GuildId::new(2), QueueType::NormalDraft), false),
        ]
        .into_iter()
        .collect();
        let cache = DummyCacheWithQueues { guilds, enabled };
        let dispatcher = AlertDispatcher::new(sender, cache);

        dispatcher.dispatch_alert("p", DummyAlert).await;

        let msgs = dispatcher.sender.sent.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, ChannelId::new(10));
    }

    struct DummyAlert;
    impl TryIntoAlert for DummyAlert {
        fn try_into_alert(&self, _: &str) -> Result<Alert, AlertCreationError> {
            Ok(CreateEmbed::new().description("test"))
        }
    }
    impl QueueTyped for DummyAlert {
        fn queue_type(&self) -> QueueType {
            QueueType::NormalDraft
        }
    }

    struct FailingAlert;
    impl TryIntoAlert for FailingAlert {
        fn try_into_alert(&self, _: &str) -> Result<Alert, AlertCreationError> {
            Err(AlertCreationError::PuuidNotInMatch { puuid: "x".into() })
        }
    }
    impl QueueTyped for FailingAlert {
        fn queue_type(&self) -> QueueType {
            QueueType::NormalDraft
        }
    }

    #[tokio::test]
    async fn dispatch_sends_to_available_channels() {
        let sender = DummySender {
            sent: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        };
        let guilds = [
            (GuildId::new(1), Some(ChannelId::new(10))),
            (GuildId::new(2), None),
        ]
        .into_iter()
        .collect();
        let cache = DummyCache { guilds };
        let dispatcher = AlertDispatcher::new(sender, cache);

        dispatcher.dispatch_alert("p", DummyAlert).await;

        let msgs = dispatcher.sender.sent.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, ChannelId::new(10));
    }

    #[tokio::test]
    async fn dispatch_alert_creation_error() {
        let sender = DummySender {
            sent: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        };
        let cache = DummyCache {
            guilds: HashMap::new(),
        };
        let dispatcher = AlertDispatcher::new(sender, cache);

        dispatcher.dispatch_alert("p", FailingAlert).await;

        assert!(dispatcher.sender.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn dispatch_sender_error() {
        let sender = DummySender {
            sent: Arc::new(Mutex::new(Vec::new())),
            fail: true,
        };
        let guilds = [(GuildId::new(1), Some(ChannelId::new(10)))]
            .into_iter()
            .collect();
        let cache = DummyCache { guilds };
        let dispatcher = AlertDispatcher::new(sender, cache);

        dispatcher.dispatch_alert("p", DummyAlert).await;

        // Should record no messages due to failure
        assert!(dispatcher.sender.sent.lock().unwrap().is_empty());
    }
}
