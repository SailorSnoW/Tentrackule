use std::collections::HashMap;
use std::sync::Arc;

use tentrackule_alert::TryIntoAlert;
use tentrackule_db::DatabaseExt;
use tracing::{error, warn};

use super::*;

pub struct AlertSender {
    ctx: Arc<serenity::Http>,
    db: SharedDatabase,
}

impl AlertSender {
    pub fn new(ctx: Arc<serenity::Http>, db: SharedDatabase) -> Self {
        Self { ctx, db }
    }

    pub async fn dispatch_alert(&self, puuid: &str, match_data: impl TryIntoAlert) {
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
                    if let Err(e) = channel_id
                        .send_message(&self.ctx, CreateMessage::new().embed(alert.clone()))
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
