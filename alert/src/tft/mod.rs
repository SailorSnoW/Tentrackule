use poise::serenity_prelude::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};
use tentrackule_shared::tft_match::{Match, Participant, QueueType, UnitsFilter};

use crate::{Alert, AlertCreationError, TryIntoAlert};

impl TryIntoAlert for Match {
    fn try_into_alert(&self, puuid_focus: &str) -> Result<Alert, AlertCreationError> {
        let focused_participant =
            self.participant(puuid_focus)
                .ok_or_else(|| AlertCreationError::PuuidNotInMatch {
                    puuid: puuid_focus.to_string(),
                })?;

        match self.queue_type() {
            QueueType::Normal => Ok(normal_alert(self, focused_participant)),

            _ => Err(AlertCreationError::UnsupportedQueueType {
                queue_id: self.info.queue_id,
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
