use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use thiserror::Error;

use crate::riot::types::{MatchDtoWithLeagueInfo, ParticipantDto, QueueType};

#[derive(Error, Debug)]
pub enum EmbedCreationError {
    #[error("The specified PUUID focus {puuid:?} isn't part of the match.")]
    PuuidNotInMatch { puuid: String },
}

impl MatchDtoWithLeagueInfo {
    pub fn into_embed(self, focused_puuid: &str) -> anyhow::Result<CreateEmbed> {
        let focused_participant = self.match_data.participant_info_of(focused_puuid).ok_or(
            EmbedCreationError::PuuidNotInMatch {
                puuid: focused_puuid.to_string(),
            },
        )?;

        match self.match_data.queue_type() {
            QueueType::SoloDuo => create_solo_duo_alert_msg(focused_participant, &self),
            QueueType::Unhandled => unreachable!(),
        }
    }
}
fn create_base_embed(
    focused_participant: &ParticipantDto,
    match_data: &MatchDtoWithLeagueInfo,
    with_role_field: bool,
) -> anyhow::Result<CreateEmbed> {
    let footer = CreateEmbedFooter::new(format!(
        "Duration: {}",
        match_data.match_data.to_formatted_match_duration()
    ));

    let embed = CreateEmbed::new()
        .title(focused_participant.to_title_win_string())
        .color(focused_participant.to_win_colour())
        .url(focused_participant.to_dpm_profile_url())
        .thumbnail(focused_participant.to_champion_picture_url())
        .footer(footer)
        .fields(vec![
            (
                "K/D/A",
                format!(
                    "{}/{}/{}",
                    focused_participant.kills,
                    focused_participant.deaths,
                    focused_participant.assists
                ),
                true,
            ),
            (
                "Role",
                focused_participant.team_position.clone(),
                with_role_field,
            ),
            ("Champion", focused_participant.champion_name.clone(), true),
        ]);

    Ok(embed)
}

fn create_solo_duo_alert_msg(
    focused_participant: &ParticipantDto,
    match_data: &MatchDtoWithLeagueInfo,
) -> anyhow::Result<CreateEmbed> {
    let author = CreateEmbedAuthor::new("[LoL] Solo/Duo Queue")
        .icon_url(focused_participant.to_profile_icon_picture_url());
    let mut embed = create_base_embed(focused_participant, match_data, true)?
        .author(author)
        .description(format!(
            "**{}** just {} a ranked game !",
            focused_participant.riot_id_game_name,
            focused_participant.to_formatted_win_string(),
        ));

    // Rank informations
    if match_data.league_data.is_some() {
        embed = embed.fields(vec![(
            "Rank",
            format!(
                "{} {} ({} LPs)",
                match_data.league_data.clone().unwrap().tier,
                match_data.league_data.clone().unwrap().rank,
                match_data.league_data.clone().unwrap().league_points
            ),
            false,
        )]);
    }

    Ok(embed)
}
