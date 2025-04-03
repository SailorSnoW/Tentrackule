use std::collections::HashMap;

use log::debug;
use log::info;
use log::warn;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::riot::types::LeagueEntryDto;
use crate::riot::types::ParticipantDto;
use crate::{db::DbRequest, riot::types::MatchDto};

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

    pub fn spawn(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(&mut self) {
        info!("✉️ Starting the Alert Sender...");

        while let Some(request) = self.receiver.recv().await {
            match request {
                AlertSenderMessage::AlertNewMatchResult {
                    puuid,
                    match_data,
                    league_data,
                } => {
                    self.alert_new_match_result(puuid, &match_data, &league_data)
                        .await;
                }
            }
        }
    }

    async fn alert_new_match_result(
        &self,
        puuid: String,
        match_data: &MatchDto,
        league_data: &Option<LeagueEntryDto>,
    ) {
        let puuid_game_data = match_data.get_participant_info_of(puuid.clone()).unwrap();
        debug!(
            "✉️ Dispatching new alert result of player: {}#{}",
            puuid_game_data.riot_id_game_name, puuid_game_data.riot_id_tagline
        );

        // First, we get all the guilds where the player is tracked with channel ID where to send
        // the alert.
        let guilds = self.get_guilds_for_account(puuid).await;

        for guild in guilds {
            let maybe_channel_id = guild.1;
            match maybe_channel_id {
                Some(channel_id) => {
                    let maybe_msg = channel_id
                        .send_message(
                            &self.ctx,
                            create_result_alert_msg(match_data, puuid_game_data, league_data),
                        )
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
    AlertNewMatchResult {
        puuid: String,
        match_data: MatchDto,
        league_data: Option<LeagueEntryDto>,
    },
}

fn create_result_alert_msg(
    match_data: &MatchDto,
    participant_game_data: &ParticipantDto,
    league_data: &Option<LeagueEntryDto>,
) -> CreateMessage {
    let author = CreateEmbedAuthor::new("Solo/Duo Queue")
        .icon_url(participant_game_data.to_profile_icon_picture_url());
    let footer = CreateEmbedFooter::new(format!(
        "Duration: {}",
        match_data.to_formatted_match_duration()
    ));
    let mut embed = CreateEmbed::new()
        .author(author)
        .title(participant_game_data.to_title_win_string())
        .color(participant_game_data.to_win_colour())
        .url(participant_game_data.to_dpm_profile_url())
        .thumbnail(participant_game_data.to_champion_picture_url())
        .description(format!(
            "**{}** just {} a ranked game !",
            participant_game_data.riot_id_game_name,
            participant_game_data.to_formatted_win_string(),
        ))
        .footer(footer)
        .timestamp(Timestamp::now());
    // Rank informations
    if league_data.is_some() {
        embed = embed.fields(vec![(
            "Rank",
            format!(
                "{} {} ({} LPs)",
                league_data.clone().unwrap().tier,
                league_data.clone().unwrap().rank,
                league_data.clone().unwrap().league_points
            ),
            false,
        )])
    }
    // Game informations
    embed = embed.fields(vec![
        (
            "K/D/A",
            format!(
                "{}/{}/{}",
                participant_game_data.kills,
                participant_game_data.deaths,
                participant_game_data.assists
            ),
            true,
        ),
        ("Role", participant_game_data.team_position.clone(), true),
        (
            "Champion",
            participant_game_data.champion_name.clone(),
            true,
        ),
    ]);

    CreateMessage::new().embed(embed)
}
