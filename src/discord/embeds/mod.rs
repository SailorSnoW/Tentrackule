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
        .title(focused_participant.to_title_win_string(
            match_data.calculate_league_points_difference(focused_participant.win),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riot::types::{InfoDto, LeagueEntryDto, MatchDto, ParticipantDto};

    fn sample_participant(puuid: &str) -> ParticipantDto {
        ParticipantDto {
            puuid: puuid.to_string(),
            champion_name: "Ahri".into(),
            team_position: "MID".into(),
            win: true,
            kills: 10,
            deaths: 2,
            assists: 5,
            profile_icon: 1,
            riot_id_game_name: "Player".into(),
            riot_id_tagline: "EUW".into(),
        }
    }

    fn sample_match(puuid: &str) -> MatchDto {
        MatchDto {
            info: InfoDto {
                participants: vec![sample_participant(puuid)],
                queue_id: 420,
                game_duration: 1800,
                game_creation: 0,
            },
        }
    }

    #[test]
    fn returns_error_when_puuid_missing() {
        let match_data = sample_match("p1");
        let dto = MatchDtoWithLeagueInfo::new(match_data, None, None);
        let err = dto.into_embed("other").unwrap_err();
        assert!(err.to_string().contains("isn't part of the match"));
    }

    #[test]
    fn creates_embed_with_league() {
        let match_data = sample_match("p1");
        let league = Some(LeagueEntryDto {
            queue_type: "RANKED_SOLO_5x5".into(),
            tier: "GOLD".into(),
            rank: "IV".into(),
            league_points: 50,
        });
        let dto = MatchDtoWithLeagueInfo::new(match_data, league, Some(40));

        // We simply assert embed creation succeeds. CreateEmbed's internal fields
        // are private so we cannot inspect them directly.
        assert!(dto.into_embed("p1").is_ok());
    }

    #[tokio::test]
    #[ignore]
    async fn send_embed_to_dev_guild() {
        use poise::serenity_prelude::{ChannelId, Http};

        dotenv::dotenv().ok();
        let token = std::env::var("DISCORD_BOT_TOKEN").expect("token missing");
        let channel_id: u64 = std::env::var("TEST_CHANNEL_ID")
            .expect("channel id missing")
            .parse()
            .expect("invalid id");

        let http = Http::new(&token);

        let match_data = sample_match("p1");
        let league = Some(LeagueEntryDto {
            queue_type: "RANKED_SOLO_5x5".into(),
            tier: "GOLD".into(),
            rank: "IV".into(),
            league_points: 50,
        });
        let dto = MatchDtoWithLeagueInfo::new(match_data, league, Some(40));
        let embed = dto.into_embed("p1").unwrap();

        ChannelId::new(channel_id)
            .send_message(
                &http,
                poise::serenity_prelude::CreateMessage::new().embed(embed),
            )
            .await
            .unwrap();
    }
}
