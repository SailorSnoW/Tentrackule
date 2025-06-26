use std::collections::HashMap;
use std::sync::Arc;

use message_sender::MessageSender;
use tentrackule_alert::TryIntoAlert;
use tentrackule_db::DatabaseExt;
use tracing::{error, warn};

use super::*;

/// Abstraction for dispatching alert messages to Discord.
#[async_trait]
pub trait AlertDispatch {
    async fn dispatch_alert(&self, puuid: &str, match_data: Box<dyn TryIntoAlert + Send + Sync>);
}

/// Implementation of [`AlertDispatch`] using a [`MessageSender`] and the database.
pub struct AlertDispatcher {
    sender: Arc<dyn MessageSender + Send + Sync>,
    db: SharedDatabase,
}

impl AlertDispatcher {
    /// Create a new dispatcher using the given message sender and database handle.
    pub fn new(sender: Arc<dyn MessageSender + Send + Sync>, db: SharedDatabase) -> Self {
        Self { sender, db }
    }

    /// Retrieve the list of guilds tracking the specified player along with
    /// their configured alert channel, if any.
    async fn get_guilds_for_account(&self, puuid: String) -> HashMap<GuildId, Option<ChannelId>> {
        match self.db.run(|db| db.get_guilds_for_puuid(puuid)).await {
            Ok(x) => x,
            Err(e) => {
                error!(
                    "❌ [ALERT] DB error while getting guilds for account: {}",
                    e
                );
                HashMap::new()
            }
        }
    }
}

#[async_trait]
impl AlertDispatch for AlertDispatcher {
    async fn dispatch_alert(&self, puuid: &str, match_data: Box<dyn TryIntoAlert + Send + Sync>) {
        let alert = match match_data.try_into_alert(puuid) {
            Ok(alert) => alert,
            Err(reason) => {
                error!("⚠️ [ALERT] failed to build alert: {}", reason);
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
                        error!("❌ [ALERT] failed to send message: {}", e)
                    }
                }
                None => {
                    warn!(
                        "⚠️ [ALERT] guild {} has no alert channel, skipping",
                        guild.0
                    );
                    continue;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poise::serenity_prelude::{CreateEmbed, GuildId};
    use rusqlite::Connection;
    use tentrackule_alert::{Alert, AlertCreationError};
    use tentrackule_db::Database;
    use tentrackule_riot_api::api::client::AccountDto;
    use tentrackule_riot_api::types::Region;
    use tokio::sync::Mutex as TokioMutex;

    struct DummyAlert;

    impl TryIntoAlert for DummyAlert {
        fn try_into_alert(&self, _puuid: &str) -> Result<Alert, AlertCreationError> {
            Ok(CreateEmbed::new().title("dummy"))
        }
    }

    #[derive(Default)]
    struct MockSender {
        sent: TokioMutex<Vec<ChannelId>>,
    }

    #[async_trait::async_trait]
    impl MessageSender for MockSender {
        async fn send_message(
            &self,
            channel_id: ChannelId,
            _msg: CreateMessage,
        ) -> serenity::Result<()> {
            self.sent.lock().await.push(channel_id);
            Ok(())
        }
    }

    fn setup_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        Database::from_connection(conn)
    }

    #[tokio::test]
    async fn dispatch_alert_sends_to_configured_channels() {
        let mut db = setup_db();
        db.track_new_account(
            AccountDto {
                puuid: "puuid".into(),
                game_name: Some("gm".into()),
                tag_line: Some("tag".into()),
            },
            Region::Euw,
            GuildId::new(1),
        )
        .unwrap();
        db.set_alert_channel(GuildId::new(1), ChannelId::new(42))
            .unwrap();
        db.track_new_account(
            AccountDto {
                puuid: "puuid".into(),
                game_name: Some("gm".into()),
                tag_line: Some("tag".into()),
            },
            Region::Euw,
            GuildId::new(2),
        )
        .unwrap();

        let shared: SharedDatabase = Arc::new(TokioMutex::new(db));
        let mock = Arc::new(MockSender::default());
        let sender =
            AlertDispatcher::new(mock.clone() as Arc<dyn MessageSender + Send + Sync>, shared);

        sender.dispatch_alert("puuid", Box::new(DummyAlert)).await;

        let sent = mock.sent.lock().await;
        assert_eq!(&*sent, &[ChannelId::new(42)]);
    }
}
