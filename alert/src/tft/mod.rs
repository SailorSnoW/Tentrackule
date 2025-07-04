use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use tentrackule_shared::{
    Account,
    lol_match::MatchRanked,
    tft_match::{Match, Participant, QueueType, UnitsFilter},
    traits::api::{LeaguePoints, LeagueRank},
};

use crate::{Alert, AlertCreationError, QueueTyped, TryIntoAlert};

impl TryIntoAlert for Match {
    fn try_into_alert(&self, account: &Account) -> Result<Alert, AlertCreationError> {
        let focused_participant = self
            .participant(&account.puuid_tft.clone().unwrap_or_default())
            .ok_or_else(|| AlertCreationError::PuuidNotInMatch {
                puuid: account.puuid.clone(),
            })?;

        match self.queue_type() {
            QueueType::Normal => Ok(normal_alert(self, focused_participant)),

            _ => Err(AlertCreationError::UnsupportedQueueType {
                queue_id: self.info.queue_id,
            }),
        }
    }
}

impl TryIntoAlert for MatchRanked<Match> {
    fn try_into_alert(&self, account: &Account) -> Result<Alert, AlertCreationError> {
        let focused_participant = self
            .base
            .participant(&account.puuid_tft.clone().unwrap_or_default())
            .ok_or_else(|| AlertCreationError::PuuidNotInMatch {
                puuid: account.puuid.clone(),
            })?;

        match self.queue_type() {
            QueueType::Ranked => Ok(ranked_alert(self, focused_participant)),

            _ => Err(AlertCreationError::UnsupportedQueueType {
                queue_id: self.base.info.queue_id,
            }),
        }
    }
}

pub fn normal_alert(match_data: &Match, focused_participant: &Participant) -> CreateEmbed {
    let footer = CreateEmbedFooter::new(format!("Set {}", match_data.info.tft_set_number));

    let author = CreateEmbedAuthor::new("[TFT] Normal Game");
    let mut fields = Vec::new();

    let embed = CreateEmbed::new()
        .title(focused_participant.to_place_title_string())
        .description(format!("**{}** just finished at the __{}__ !", focused_participant.riot_id_game_name, focused_participant.to_place_string()))
        .color(focused_participant.to_win_colour())
        .url(match_data.to_trackergg_url())
        .thumbnail("https://ddragon.leagueoflegends.com/cdn/13.24.1/img/tft-tactician/Tooltip_Nimblefoot_Base_Variant4_Tier1.png")
        .footer(footer)
        .author(author);

    if let Some(unit) = focused_participant.units.best_unit() {
        fields.push(("Best Unit", format!("{}", unit), false))
    };

    fields.push((
        "Gold Left",
        format!("{}", focused_participant.gold_left),
        true,
    ));
    fields.push((
        "Rounds Survived",
        format!("{}", focused_participant.last_round),
        true,
    ));
    fields.push((
        "Damage Dealt",
        format!("{}", focused_participant.total_damage_to_players),
        true,
    ));

    embed.fields(fields)
}

pub fn ranked_alert(
    match_data: &MatchRanked<Match>,
    focused_participant: &Participant,
) -> CreateEmbed {
    let author = CreateEmbedAuthor::new("[TFT] Ranked Queue");

    let embed = normal_alert(&match_data.base, focused_participant)
        .author(author)
        .title(format!(
            "{}{}",
            focused_participant.to_place_title_string(),
            match match_data.calculate_league_points_difference(focused_participant.placement < 5) {
                Some(diff) => format!(" ({:+} LPs)", diff),
                None => String::new(),
            }
        ));

    // Rank informations
    embed.fields(vec![(
        "Rank",
        format!(
            "{} {} ({} LPs)",
            match_data.current_league.clone().tier(),
            match_data.current_league.clone().rank(),
            match_data.current_league.clone().league_points()
        ),
        false,
    )])
}
