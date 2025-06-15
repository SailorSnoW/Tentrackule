use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tracing::{error, info, warn};

use crate::db::DbRequest;
use crate::riot::types::MatchDtoWithLeagueInfo;

use super::*;

pub type AlertSenderRx = mpsc::Receiver<AlertSenderMessage>;
pub type AlertSenderTx = mpsc::Sender<AlertSenderMessage>;

pub struct AlertSender {
    ctx: Arc<serenity::Http>,
    receiver: AlertSenderRx,
    db_sender: mpsc::Sender<DbRequest>,
}

impl AlertSender {
    pub fn new(
        receiver: AlertSenderRx,
        ctx: Arc<serenity::Http>,
        db_sender: mpsc::Sender<DbRequest>,
    ) -> Self {
        Self {
            receiver,
            ctx,
            db_sender,
        }
    }

    pub fn start(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(&mut self) {
        info!("📢 [ALERT] sender started");

        while let Some(request) = self.receiver.recv().await {
            match request {
                AlertSenderMessage::DispatchNewAlert { puuid, match_data } => {
                    self.dispatch_alert(&puuid, match_data).await;
                }
            }
        }
    }

    async fn dispatch_alert(&self, puuid: &str, match_data: MatchDtoWithLeagueInfo) {
        let alert = match match_data.into_embed(puuid) {
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
                    let maybe_msg = channel_id
                        .send_message(&self.ctx, CreateMessage::new().embed(alert.clone()))
                        .await;
                    match maybe_msg {
                        Ok(msg) => {
                            let _ = msg.react(&self.ctx, '🎉').await;
                            let _ = msg.react(&self.ctx, '😂').await;
                            let _ = msg.react(&self.ctx, '😭').await;
                            let _ = msg.react(&self.ctx, '😱').await;
                        }
                        Err(e) => {
                            error!("❌ [ALERT] failed to send message: {}", e)
                        }
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
        let (tx, rx) = oneshot::channel();

        if let Err(e) = self
            .db_sender
            .send(DbRequest::GetGuildsForAccount {
                puuid,
                respond_to: tx,
            })
            .await
        {
            error!("❌ [ALERT] failed to send DB request: {}", e);
            return HashMap::new();
        }

        match rx.await {
            Ok(Ok(guilds)) => guilds,
            Ok(Err(e)) => {
                error!(
                    "❌ [ALERT] DB error while getting guilds for account: {}",
                    e
                );
                HashMap::new()
            }
            Err(e) => {
                error!(
                    "❌ [ALERT] DB request cancelled while getting guilds for account: {}",
                    e
                );
                HashMap::new()
            }
        }
    }
}

#[derive(Debug)]
pub enum AlertSenderMessage {
    DispatchNewAlert {
        puuid: String,
        match_data: MatchDtoWithLeagueInfo,
    },
}
