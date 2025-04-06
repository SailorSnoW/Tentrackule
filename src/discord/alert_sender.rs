use std::collections::HashMap;

use log::error;
use log::info;
use log::warn;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::db::DbRequest;
use crate::riot::types::MatchDtoWithLeagueInfo;

use super::*;

pub type AlertSenderRx = mpsc::Receiver<AlertSenderMessage>;
pub type AlertSenderTx = mpsc::Sender<AlertSenderMessage>;

pub struct AlertSender {
    ctx: serenity::Context,
    receiver: AlertSenderRx,
    db_sender: mpsc::Sender<DbRequest>,
}

impl AlertSender {
    pub fn new(
        receiver: AlertSenderRx,
        ctx: serenity::Context,
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
        info!("✉️ Starting the Alert Sender...");

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
                error!(
                    "✉️ Alert message couldn't be created, cancelling dispatch.\n reason: {}",
                    reason
                );
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
                            error!("✉️ Something went wrong while sending alert message: {}", e)
                        }
                    }
                }
                None => {
                    warn!(
                        "✉️ No alert channel set for guild {}, ignoring dispatch.",
                        guild.0
                    );
                    break;
                }
            }
        }
    }

    async fn get_guilds_for_account(&self, puuid: String) -> HashMap<GuildId, Option<ChannelId>> {
        let (tx, rx) = oneshot::channel();
        self.db_sender
            .send(DbRequest::GetGuildsForAccount {
                puuid,
                respond_to: tx,
            })
            .await
            .unwrap();

        rx.await.unwrap().unwrap()
    }
}

#[derive(Debug)]
pub enum AlertSenderMessage {
    DispatchNewAlert {
        puuid: String,
        match_data: MatchDtoWithLeagueInfo,
    },
}
