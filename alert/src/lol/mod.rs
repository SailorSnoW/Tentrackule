//! Helpers to build Discord embeds for League of Legends matches.

use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use tentrackule_types::{
    lol_match::{Match, MatchParticipant, MatchRanked},
    QueueType,
};

use crate::{Alert, AlertCreationError, TryIntoAlert};

impl TryIntoAlert for Match {
    fn try_into_alert(&self, puuid_focus: &str) -> Result<Alert, AlertCreationError> {
        let focused_participant =
            self.participant(puuid_focus)
                .ok_or_else(|| AlertCreationError::PuuidNotInMatch {
                    puuid: puuid_focus.to_string(),
                })?;

        match self.queue_type() {
            QueueType::NormalDraft => Ok(draft_normal_alert(focused_participant, self)),
            QueueType::Aram => Ok(aram_alert(focused_participant, self)),
            _ => Err(AlertCreationError::UnsupportedQueueType {
                queue_id: self.queue_id,
            }),
        }
    }
}

impl TryIntoAlert for MatchRanked {
    fn try_into_alert(&self, puuid_focus: &str) -> Result<Alert, AlertCreationError> {
        let focused_participant = self.base.participant(puuid_focus).ok_or_else(|| {
            AlertCreationError::PuuidNotInMatch {
                puuid: puuid_focus.to_string(),
            }
        })?;

        match self.base.queue_type() {
            QueueType::SoloDuo => Ok(flex_ranked_alert(focused_participant, self)),
            QueueType::Flex => Ok(solo_duo_ranked_alert(focused_participant, self)),
            _ => Err(AlertCreationError::UnsupportedQueueType {
                queue_id: self.base.queue_id,
            }),
        }
    }
}

/// Shared alert base.
fn base(
    focused_participant: &MatchParticipant,
    match_data: &Match,
    with_role_field: bool,
) -> CreateEmbed {
    let footer = CreateEmbedFooter::new(format!(
        "Duration: {}",
        match_data.to_formatted_match_duration()
    ));
    let mut fields = Vec::new();

    let embed = CreateEmbed::new()
        .title(focused_participant.to_title_win_string())
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
    focused_participant: &MatchParticipant,
    match_data: &MatchRanked,
) -> CreateEmbed {
    let author = CreateEmbedAuthor::new("[LoL] Solo/Duo Queue")
        .icon_url(focused_participant.to_profile_icon_picture_url());
    ranked_alert(focused_participant, match_data).author(author)
}

fn flex_ranked_alert(
    focused_participant: &MatchParticipant,
    match_data: &MatchRanked,
) -> CreateEmbed {
    let author = CreateEmbedAuthor::new("[LoL] Flex Queue")
        .icon_url(focused_participant.to_profile_icon_picture_url());
    ranked_alert(focused_participant, match_data).author(author)
}

fn ranked_alert(focused_participant: &MatchParticipant, match_data: &MatchRanked) -> CreateEmbed {
    let mut embed = base(focused_participant, &match_data.base, true).description(format!(
        "**{}** just {} a ranked game !",
        focused_participant.riot_id_game_name,
        focused_participant.to_formatted_win_string(),
    ));

    // Rank informations
    embed = embed.fields(vec![(
        "Rank",
        format!(
            "{} {} ({} LPs)",
            match_data.current_league.clone().tier,
            match_data.current_league.clone().rank,
            match_data.current_league.clone().league_points
        ),
        false,
    )]);

    embed
}

fn draft_normal_alert(focused_participant: &MatchParticipant, match_data: &Match) -> CreateEmbed {
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

fn aram_alert(focused_participant: &MatchParticipant, match_data: &Match) -> CreateEmbed {
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
