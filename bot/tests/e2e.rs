use poise::serenity_prelude::{ChannelId, CreateMessage, Http};
use serde_json::json;
use tentrackule_alert::TryIntoAlert;
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

#[tokio::test]
#[ignore = "Discord E2E"]
async fn send_sample_alerts() {
    dotenv::dotenv().ok();
    let token = match std::env::var("DISCORD_BOT_TOKEN") {
        Ok(t) => t,
        Err(_) => return,
    };
    let channel_id: u64 = match std::env::var("TEST_CHANNEL_ID") {
        Ok(c) => c.parse().unwrap(),
        Err(_) => return,
    };
    let http = Http::new(&token);
    let channel = ChannelId::new(channel_id);

    for (queue, ranked) in [(420, true), (400, false), (450, false)] {
        let match_info = setup_match(queue, ranked);
        let embed = match_info.try_into_alert("abc").unwrap();
        channel
            .send_message(&http, CreateMessage::new().embed(embed))
            .await
            .unwrap();
    }
}
