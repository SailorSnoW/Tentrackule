//! Helpers to build Discord embeds for League of Legends matches.

use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use tentrackule_riot_api::{
    api::types::{MatchDtoWithLeagueInfo, ParticipantDto},
    types::QueueType,
};

use crate::{Alert, AlertCreationError, TryIntoAlert};

impl TryIntoAlert for MatchDtoWithLeagueInfo {
    fn try_into_alert(&self, puuid_focus: &str) -> Result<Alert, AlertCreationError> {
        let focused_participant = self
            .match_data
            .participant_info_of(puuid_focus)
            .ok_or_else(|| AlertCreationError::PuuidNotInMatch {
                puuid: puuid_focus.to_string(),
            })?;

        match self.match_data.queue_type() {
            QueueType::SoloDuo => Ok(solo_duo_ranked_alert(focused_participant, self)),
            QueueType::NormalDraft => Ok(draft_normal_alert(focused_participant, self)),
            QueueType::Aram => Ok(aram_alert(focused_participant, self)),
            QueueType::Unhandled => Err(AlertCreationError::UnsupportedQueueType {
                queue_id: self.match_data.info.queue_id,
            }),
        }
    }
}

/// Shared alert base.
fn base(
    focused_participant: &ParticipantDto,
    match_data: &MatchDtoWithLeagueInfo,
    with_role_field: bool,
) -> CreateEmbed {
    let footer = CreateEmbedFooter::new(format!(
        "Duration: {}",
        match_data.match_data.to_formatted_match_duration()
    ));
    let mut fields = Vec::new();

    let embed = CreateEmbed::new()
        .title(focused_participant.to_title_win_string(
            match_data.calculate_league_points_difference(focused_participant.win),
        ))
        .color(focused_participant.to_win_colour())
        .url(focused_participant.to_dpm_profile_url())
        .thumbnail(focused_participant.to_champion_picture_url())
        .footer(footer);

    fields.push((
        "K/D/A",
        format!(
            "{}/{}/{}",
            focused_participant.kills, focused_participant.deaths, focused_participant.assists
        ),
        true,
    ));
    if with_role_field {
        fields.push(("Role", focused_participant.to_normalized_role(), true));
    }
    fields.push(("Champion", focused_participant.champion_name.clone(), true));

    embed.fields(fields)
}

fn solo_duo_ranked_alert(
    focused_participant: &ParticipantDto,
    match_data: &MatchDtoWithLeagueInfo,
) -> CreateEmbed {
    let author = CreateEmbedAuthor::new("[LoL] Solo/Duo Queue")
        .icon_url(focused_participant.to_profile_icon_picture_url());
    let mut embed = base(focused_participant, match_data, true)
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

    embed
}

fn draft_normal_alert(
    focused_participant: &ParticipantDto,
    match_data: &MatchDtoWithLeagueInfo,
) -> CreateEmbed {
    let author = CreateEmbedAuthor::new("[LoL] Normal Draft")
        .icon_url(focused_participant.to_profile_icon_picture_url());
    base(focused_participant, match_data, true)
        .author(author)
        .description(format!(
            "**{}** just {} a normal game !",
            focused_participant.riot_id_game_name,
            focused_participant.to_formatted_win_string(),
        ))
}

fn aram_alert(
    focused_participant: &ParticipantDto,
    match_data: &MatchDtoWithLeagueInfo,
) -> CreateEmbed {
    let author = CreateEmbedAuthor::new("[LoL] ARAM")
        .icon_url(focused_participant.to_profile_icon_picture_url());
    base(focused_participant, match_data, false)
        .author(author)
        .description(format!(
            "**{}** just {} an ARAM game !",
            focused_participant.riot_id_game_name,
            focused_participant.to_formatted_win_string(),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AlertCreationError;
    use serde_json::{json, Value};
    use tentrackule_riot_api::api::types::{
        LeagueEntryDto, MatchDto, MatchDtoWithLeagueInfo, ParticipantDto,
    };

    fn dummy_participant(puuid: &str) -> ParticipantDto {
        ParticipantDto {
            puuid: puuid.into(),
            champion_name: "Lux".into(),
            team_position: "MIDDLE".into(),
            win: true,
            kills: 5,
            deaths: 2,
            assists: 8,
            profile_icon: 1234,
            riot_id_game_name: "Game".into(),
            riot_id_tagline: "Tag".into(),
        }
    }

    fn dummy_match(queue_id: u16, participant: &ParticipantDto) -> MatchDto {
        let value = json!({
            "info": {
                "participants": [{
                    "puuid": participant.puuid,
                    "championName": participant.champion_name,
                    "teamPosition": participant.team_position,
                    "win": participant.win,
                    "kills": participant.kills,
                    "deaths": participant.deaths,
                    "assists": participant.assists,
                    "profileIcon": participant.profile_icon,
                    "riotIdGameName": participant.riot_id_game_name,
                    "riotIdTagline": participant.riot_id_tagline,
                }],
                "queueId": queue_id,
                "gameDuration": 125,
                "gameCreation": 0
            }
        });
        serde_json::from_value(value).unwrap()
    }

    fn league_entry(lp: u16) -> LeagueEntryDto {
        LeagueEntryDto {
            queue_type: "RANKED_SOLO_5x5".to_string(),
            tier: "GOLD".to_string(),
            rank: "IV".to_string(),
            league_points: lp,
        }
    }

    fn setup_match(queue: u16, with_league: bool) -> MatchDtoWithLeagueInfo {
        let participant = dummy_participant("abc");
        let match_data = dummy_match(queue, &participant);
        let league = with_league.then(|| league_entry(120));
        MatchDtoWithLeagueInfo::new(match_data, league, Some(100))
    }

    #[test]
    fn base_fields_with_and_without_role() {
        let participant = dummy_participant("abc");
        let match_info = MatchDtoWithLeagueInfo::new(
            dummy_match(420, &participant),
            Some(league_entry(120)),
            Some(100),
        );

        let embed_with_role = super::base(&participant, &match_info, true);
        let json: Value = serde_json::to_value(&embed_with_role).unwrap();
        let fields = json.get("fields").unwrap().as_array().unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[1]["name"], "Role");

        let embed_no_role = super::base(&participant, &match_info, false);
        let json: Value = serde_json::to_value(&embed_no_role).unwrap();
        let fields = json.get("fields").unwrap().as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields.iter().all(|f| f["name"] != "Role"));
    }

    #[test]
    fn solo_duo_alert_includes_rank() {
        let match_info = setup_match(420, true);
        let participant = match_info.match_data.info.participants[0].clone();

        let embed = super::solo_duo_ranked_alert(&participant, &match_info);
        let json: Value = serde_json::to_value(&embed).unwrap();
        assert_eq!(json["author"]["name"], "[LoL] Solo/Duo Queue");
        assert!(json["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "Rank"));
        assert!(json["description"]
            .as_str()
            .unwrap()
            .contains("ranked game"));
    }

    #[test]
    fn draft_and_aram_alerts() {
        let draft = setup_match(400, false);
        let part = draft.match_data.info.participants[0].clone();
        let embed_draft = super::draft_normal_alert(&part, &draft);
        let json: Value = serde_json::to_value(&embed_draft).unwrap();
        assert_eq!(json["author"]["name"], "[LoL] Normal Draft");

        let aram = setup_match(450, false);
        let part_aram = aram.match_data.info.participants[0].clone();
        let embed_aram = super::aram_alert(&part_aram, &aram);
        let json: Value = serde_json::to_value(&embed_aram).unwrap();
        assert_eq!(json["author"]["name"], "[LoL] ARAM");
        assert_eq!(json["fields"].as_array().unwrap().len(), 2); // no role
    }

    #[test]
    fn try_into_alert_branches() {
        let match_info = setup_match(420, true);
        let result = match_info.try_into_alert("abc");
        assert!(result.is_ok());

        let match_info = setup_match(400, false);
        assert!(match_info.try_into_alert("abc").is_ok());

        let match_info = setup_match(450, false);
        assert!(match_info.try_into_alert("abc").is_ok());

        let match_info = setup_match(999, false);
        let err = match_info.try_into_alert("abc").unwrap_err();
        assert!(matches!(
            err,
            AlertCreationError::UnsupportedQueueType { queue_id: 999 }
        ));

        let no_puuid = setup_match(420, true);
        assert!(matches!(
            no_puuid.try_into_alert("missing"),
            Err(AlertCreationError::PuuidNotInMatch { .. })
        ));
    }
}
