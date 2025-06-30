use async_trait::async_trait;
use poise::serenity_prelude::{ChannelId, GuildId};
use std::fmt::Debug;
use std::{collections::HashMap, error::Error as ErrorT};

use crate::{Account, CachedLeague, QueueType};

pub type CachedSourceError = Box<dyn ErrorT + Send + Sync>;

#[async_trait]
pub trait CachedLeagueSource {
    async fn get_league_for(
        &self,
        puuid: String,
        queue_type: QueueType,
    ) -> Result<Option<CachedLeague>, CachedSourceError>;

    async fn set_league_for(
        &self,
        puuid: String,
        queue_type: QueueType,
        league: CachedLeague,
    ) -> Result<(), CachedSourceError>;
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
}

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
