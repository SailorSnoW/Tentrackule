//! Helpers to build Discord embeds for League of Legends matches.

use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use tentrackule_shared::{
    lol_match::{Match, MatchParticipant, MatchRanked},
    traits::api::{LeaguePoints, LeagueRank},
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
            QueueType::Flex => Ok(flex_ranked_alert(focused_participant, self)),
            QueueType::SoloDuo => Ok(solo_duo_ranked_alert(focused_participant, self)),
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
    let mut embed = base(focused_participant, &match_data.base, true)
        .description(format!(
            "**{}** just {} a ranked game !",
            focused_participant.riot_id_game_name,
            focused_participant.to_formatted_win_string(),
        ))
        .title(format!(
            "{} ({:+} LPs)",
            focused_participant.to_title_win_string(),
            match_data.calculate_league_points_difference(focused_participant.win)
        ));

    // Rank informations
    embed = embed.fields(vec![(
        "Rank",
        format!(
            "{} {} ({} LPs)",
            match_data.current_league.clone().tier(),
            match_data.current_league.clone().rank(),
            match_data.current_league.clone().league_points()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tentrackule_shared::{
        init_ddragon_version,
        lol_match::{Match, MatchParticipant, MatchRanked},
        League,
    };

    fn sample_participant(puuid: &str, win: bool, role: &str) -> MatchParticipant {
        MatchParticipant {
            puuid: puuid.to_string(),
            champion_name: "Ahri".to_string(),
            team_position: role.to_string(),
            win,
            kills: 1,
            deaths: 2,
            assists: 3,
            profile_icon: 1,
            riot_id_game_name: "Tester".to_string(),
            riot_id_tagline: "EUW".to_string(),
        }
    }

    fn league(queue: &str) -> League {
        League {
            queue_type: queue.to_string(),
            points: 10,
            wins: 1,
            losses: 1,
            rank: "I".to_string(),
            tier: "Bronze".to_string(),
        }
    }

    #[test]
    fn normal_draft_alert_contains_role() {
        init_ddragon_version();
        let p = sample_participant("p1", true, "JUNGLE");
        let m = Match {
            participants: vec![p.clone()],
            queue_id: 400,
            game_duration: 90,
            game_creation: 0,
        };
        let embed = m.try_into_alert("p1").unwrap();
        let data: Value = serde_json::to_value(&embed).unwrap();
        assert_eq!(data["author"]["name"], "[LoL] Normal Draft");
        // Role field should be present
        assert!(data["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "Role"));
    }

    #[test]
    fn aram_alert_has_no_role() {
        init_ddragon_version();
        let p = sample_participant("p1", false, "UTILITY");
        let m = Match {
            participants: vec![p.clone()],
            queue_id: 450,
            game_duration: 60,
            game_creation: 0,
        };
        let embed = m.try_into_alert("p1").unwrap();
        let data: Value = serde_json::to_value(&embed).unwrap();
        assert_eq!(data["author"]["name"], "[LoL] ARAM");
        assert!(data["fields"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["name"] != "Role"));
    }

    #[test]
    fn unsupported_queue_returns_error() {
        init_ddragon_version();
        let p = sample_participant("p1", true, "TOP");
        let m = Match {
            participants: vec![p.clone()],
            queue_id: 999,
            game_duration: 60,
            game_creation: 0,
        };
        match m.try_into_alert("p1").unwrap_err() {
            AlertCreationError::UnsupportedQueueType { queue_id } => assert_eq!(queue_id, 999),
            _ => panic!("unexpected error"),
        }
    }

    #[test]
    fn puuid_not_in_match_error() {
        init_ddragon_version();
        let p = sample_participant("p1", true, "TOP");
        let m = Match {
            participants: vec![p.clone()],
            queue_id: 400,
            game_duration: 60,
            game_creation: 0,
        };
        match m.try_into_alert("other").unwrap_err() {
            AlertCreationError::PuuidNotInMatch { puuid } => assert_eq!(puuid, "other"),
            _ => panic!("unexpected error"),
        }
    }

    #[test]
    fn ranked_solo_duo_alert() {
        init_ddragon_version();
        let p = sample_participant("p1", true, "MIDDLE");
        let base = Match {
            participants: vec![p.clone()],
            queue_id: 420,
            game_duration: 120,
            game_creation: 0,
        };
        let ranked = MatchRanked {
            base,
            current_league: league("RANKED_SOLO_5x5"),
            cached_league: league("RANKED_SOLO_5x5"),
        };
        let embed = ranked.try_into_alert("p1").unwrap();
        let data: Value = serde_json::to_value(&embed).unwrap();
        assert_eq!(data["author"]["name"], "[LoL] Solo/Duo Queue");
    }

    #[test]
    fn ranked_flex_alert() {
        init_ddragon_version();
        let p = sample_participant("p1", true, "TOP");
        let base = Match {
            participants: vec![p.clone()],
            queue_id: 440,
            game_duration: 120,
            game_creation: 0,
        };
        let ranked = MatchRanked {
            base,
            current_league: league("RANKED_FLEX_SR"),
            cached_league: league("RANKED_FLEX_SR"),
        };
        let embed = ranked.try_into_alert("p1").unwrap();
        let data: Value = serde_json::to_value(&embed).unwrap();
        assert_eq!(data["author"]["name"], "[LoL] Flex Queue");
    }

    #[test]
    fn ranked_unsupported_queue() {
        init_ddragon_version();
        let p = sample_participant("p1", true, "MID");
        let base = Match {
            participants: vec![p.clone()],
            queue_id: 999,
            game_duration: 120,
            game_creation: 0,
        };
        let ranked = MatchRanked {
            base,
            current_league: league("UNHANDLED"),
            cached_league: league("UNHANDLED"),
        };
        match ranked.try_into_alert("p1").unwrap_err() {
            AlertCreationError::UnsupportedQueueType { queue_id } => assert_eq!(queue_id, 999),
            _ => panic!("unexpected error"),
        }
    }
}
