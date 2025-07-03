use async_trait::async_trait;
use poise::serenity_prelude::{ChannelId, GuildId};
use std::fmt::Debug;
use std::{collections::HashMap, error::Error as ErrorT};

use crate::{Account, League, UnifiedQueueType};

pub type CachedSourceError = Box<dyn ErrorT + Send + Sync>;

pub trait QueueKind: ToString + Send + Sync {
    fn to_unified(&self) -> UnifiedQueueType;
}

#[async_trait]
pub trait CachedLeagueSource {
    async fn get_league_for(
        &self,
        puuid: String,
        queue_type: &dyn QueueKind,
    ) -> Result<Option<League>, CachedSourceError>;

    async fn set_league_for(&self, puuid: String, league: League) -> Result<(), CachedSourceError>;
}

#[async_trait]
pub trait CachedSettingSource {
    async fn set_alert_channel(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<(), CachedSourceError>;
    async fn get_alert_channel(
        &self,
        guild_id: GuildId,
    ) -> Result<Option<ChannelId>, CachedSourceError>;

    async fn set_queue_alert_enabled(
        &self,
        guild_id: GuildId,
        queue_type: &dyn QueueKind,
        enabled: bool,
    ) -> Result<(), CachedSourceError>;

    async fn is_queue_alert_enabled(
        &self,
        guild_id: GuildId,
        queue_type: &dyn QueueKind,
    ) -> Result<bool, CachedSourceError>;
}

/// Super-trait to specify the required API to handle caching tracked accounts/guilds/settings...
pub trait CacheFull: CachedAccountSource + CachedAccountGuildSource + CachedSettingSource {}

#[async_trait]
pub trait CachedAccountSource: Send + Sync + Debug {
    async fn insert_account(
        &self,
        account: Account,
        guild_id: GuildId,
    ) -> Result<(), CachedSourceError>;
    async fn remove_account(
        &self,
        puuid: String,
        guild_id: GuildId,
    ) -> Result<(), CachedSourceError>;

    async fn set_last_match_id(
        &self,
        puuid: String,
        match_id: String,
    ) -> Result<(), CachedSourceError>;

    /// Get all accounts from the cache.
    async fn get_all_accounts(&self) -> Result<Vec<Account>, CachedSourceError>;
}

#[async_trait]
pub trait CachedAccountGuildSource {
    async fn get_guilds_for(
        &self,
        puuid: String,
    ) -> Result<HashMap<GuildId, Option<ChannelId>>, CachedSourceError>;

    async fn get_accounts_for(&self, guild_id: GuildId) -> Result<Vec<Account>, CachedSourceError>;
}

pub mod api {
    use bytes::Bytes;

    use crate::{Region, lol_match::Match};

    use super::*;

    pub type ApiError = Box<dyn ErrorT + Send + Sync + 'static>;

    /// A league identified with a queue type string.
    pub trait LeagueQueueType {
        fn queue_type(&self) -> String;
    }

    /// A league having actual points data.
    pub trait LeaguePoints {
        fn league_points(&self) -> u16;
    }

    /// A league having rank and tier data.
    pub trait LeagueRank {
        fn rank(&self) -> String;
        fn tier(&self) -> String;
    }

    /// Trait implemented by structures capable of performing raw HTTP requests to the riot API.
    #[async_trait]
    pub trait ApiRequest: Send + Sync + Debug {
        async fn request(&self, path: String) -> Result<Bytes, ApiError>;
    }

    /// Riot Account-V1 API as described in the official documentation.
    #[async_trait]
    pub trait AccountApi: ApiRequest {
        fn route(&self) -> &'static str;

        async fn get_account_by_riot_id(
            &self,
            game_name: String,
            tag_line: String,
        ) -> Result<Account, ApiError>;
    }

    pub trait LolApiFull: LeagueApi + MatchApi<Match> + AccountApi {}

    #[async_trait]
    pub trait LeagueApi: ApiRequest {
        async fn get_leagues(&self, puuid: String, region: Region)
        -> Result<Vec<League>, ApiError>;
    }

    #[async_trait]
    pub trait MatchApi<T>: ApiRequest {
        async fn get_last_match_id(
            &self,
            puuid: String,
            region: Region,
        ) -> Result<Option<String>, ApiError>;

        async fn get_match(&self, match_id: String, region: Region) -> Result<T, ApiError>;
    }
}
