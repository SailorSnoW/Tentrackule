use std::{collections::HashMap, env, sync::Arc};

use async_trait::async_trait;
use dotenv::dotenv;
use poise::serenity_prelude::{ChannelId, CreateEmbed, GuildId, Http};
use tentrackule_alert::{
    Alert, AlertCreationError, AlertDispatch, QueueTyped, TryIntoAlert,
    alert_dispatcher::AlertDispatcher,
};
use tentrackule_shared::{
    Account, League, QueueType, init_ddragon_version,
    lol_match::{Match, MatchParticipant, MatchRanked},
    traits::{CachedAccountGuildSource, CachedSettingSource, CachedSourceError},
};

struct DummyAlert;

impl TryIntoAlert for DummyAlert {
    fn try_into_alert(&self, _puuid_focus: &str) -> Result<Alert, AlertCreationError> {
        Ok(CreateEmbed::new()
            .title("E2E Alert")
            .description("This is a test"))
    }
}

impl QueueTyped for DummyAlert {
    fn queue_type(&self) -> QueueType {
        QueueType::NormalDraft
    }
}

struct TestCache {
    channel: ChannelId,
}

#[async_trait]
impl CachedSettingSource for TestCache {
    async fn set_alert_channel(
        &self,
        _guild_id: GuildId,
        _channel_id: ChannelId,
    ) -> Result<(), CachedSourceError> {
        Ok(())
    }

    async fn get_alert_channel(
        &self,
        _guild_id: GuildId,
    ) -> Result<Option<ChannelId>, CachedSourceError> {
        Ok(Some(self.channel))
    }

    async fn set_queue_alert_enabled(
        &self,
        _guild_id: GuildId,
        _queue_type: QueueType,
        _enabled: bool,
    ) -> Result<(), CachedSourceError> {
        Ok(())
    }

    async fn is_queue_alert_enabled(
        &self,
        _guild_id: GuildId,
        _queue_type: QueueType,
    ) -> Result<bool, CachedSourceError> {
        Ok(true)
    }
}

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

fn sample_league(queue: &str, lp: u16) -> League {
    League {
        queue_type: queue.to_string(),
        league_points: lp,
        wins: 1,
        losses: 1,
        rank: "I".to_string(),
        tier: "Bronze".to_string(),
    }
}

#[async_trait]
impl CachedAccountGuildSource for TestCache {
    async fn get_guilds_for(
        &self,
        _puuid: String,
    ) -> Result<HashMap<GuildId, Option<ChannelId>>, CachedSourceError> {
        Ok([(GuildId::new(1), Some(self.channel))]
            .into_iter()
            .collect())
    }

    async fn get_accounts_for(
        &self,
        _guild_id: GuildId,
    ) -> Result<Vec<Account>, CachedSourceError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
#[ignore = "Requires Discord credentials"]
async fn dispatch_alert_to_discord() {
    dotenv().ok();
    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN not set");
    let channel_id: u64 = env::var("TEST_CHANNEL_ID")
        .expect("TEST_CHANNEL_ID not set")
        .parse()
        .expect("invalid channel id");
    let http = Arc::new(Http::new(&token));
    let cache = TestCache {
        channel: ChannelId::new(channel_id),
    };

    let dispatcher = AlertDispatcher::new(http, cache);

    dispatcher.dispatch_alert("puuid", DummyAlert).await;
}

#[tokio::test]
#[ignore = "Requires Discord credentials"]
async fn dispatch_lol_match_alert() {
    dotenv().ok();
    init_ddragon_version();
    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN not set");
    let channel_id: u64 = env::var("TEST_CHANNEL_ID")
        .expect("TEST_CHANNEL_ID not set")
        .parse()
        .expect("invalid channel id");

    let http = Arc::new(Http::new(&token));
    let cache = TestCache {
        channel: ChannelId::new(channel_id),
    };

    let dispatcher = AlertDispatcher::new(http, cache);

    let p = sample_participant("p1", true, "MIDDLE");
    let m = Match {
        participants: vec![p.clone()],
        queue_id: 400,
        game_duration: 600,
        game_creation: 0,
    };

    dispatcher.dispatch_alert("p1", m).await;
}

#[tokio::test]
#[ignore = "Requires Discord credentials"]
async fn dispatch_lol_ranked_alert() {
    dotenv().ok();
    init_ddragon_version();
    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN not set");
    let channel_id: u64 = env::var("TEST_CHANNEL_ID")
        .expect("TEST_CHANNEL_ID not set")
        .parse()
        .expect("invalid channel id");

    let http = Arc::new(Http::new(&token));
    let cache = TestCache {
        channel: ChannelId::new(channel_id),
    };

    let dispatcher = AlertDispatcher::new(http, cache);

    let p = sample_participant("p1", true, "TOP");
    let base = Match {
        participants: vec![p.clone()],
        queue_id: 420,
        game_duration: 600,
        game_creation: 0,
    };
    let ranked = MatchRanked {
        base,
        current_league: sample_league("RANKED_SOLO_5x5", 40),
        cached_league: Some(sample_league("RANKED_SOLO_5x5", 20)),
    };

    dispatcher.dispatch_alert("p1", ranked).await;
}

#[tokio::test]
#[ignore = "Requires Discord credentials"]
async fn dispatch_lol_flex_ranked_alert() {
    dotenv().ok();
    init_ddragon_version();
    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN not set");
    let channel_id: u64 = env::var("TEST_CHANNEL_ID")
        .expect("TEST_CHANNEL_ID not set")
        .parse()
        .expect("invalid channel id");

    let http = Arc::new(Http::new(&token));
    let cache = TestCache {
        channel: ChannelId::new(channel_id),
    };

    let dispatcher = AlertDispatcher::new(http, cache);

    let p = sample_participant("p1", false, "JUNGLE");
    let base = Match {
        participants: vec![p.clone()],
        queue_id: 440,
        game_duration: 600,
        game_creation: 0,
    };
    let ranked = MatchRanked {
        base,
        current_league: sample_league("RANKED_FLEX_SR", 40),
        cached_league: Some(sample_league("RANKED_FLEX_SR", 20)),
    };

    dispatcher.dispatch_alert("p1", ranked).await;
}

#[tokio::test]
#[ignore = "Requires Discord credentials"]
async fn dispatch_lol_aram_alert() {
    dotenv().ok();
    init_ddragon_version();
    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN not set");
    let channel_id: u64 = env::var("TEST_CHANNEL_ID")
        .expect("TEST_CHANNEL_ID not set")
        .parse()
        .expect("invalid channel id");

    let http = Arc::new(Http::new(&token));
    let cache = TestCache {
        channel: ChannelId::new(channel_id),
    };

    let dispatcher = AlertDispatcher::new(http, cache);

    let p = sample_participant("p1", true, "UTILITY");
    let m = Match {
        participants: vec![p.clone()],
        queue_id: 450,
        game_duration: 600,
        game_creation: 0,
    };

    dispatcher.dispatch_alert("p1", m).await;
}

#[tokio::test]
#[ignore = "Requires Discord credentials"]
async fn dispatch_remake_game_alert() {
    dotenv().ok();
    init_ddragon_version();
    let token = env::var("DISCORD_BOT_TOKEN").expect("DISCORD_BOT_TOKEN not set");
    let channel_id: u64 = env::var("TEST_CHANNEL_ID")
        .expect("TEST_CHANNEL_ID not set")
        .parse()
        .expect("invalid channel id");

    let http = Arc::new(Http::new(&token));
    let cache = TestCache {
        channel: ChannelId::new(channel_id),
    };

    let dispatcher = AlertDispatcher::new(http, cache);

    let p = sample_participant("p1", true, "UTILITY");
    let m = Match {
        participants: vec![p.clone()],
        queue_id: 450,
        game_duration: 80,
        game_creation: 0,
    };

    dispatcher.dispatch_alert("p1", m).await;
}
