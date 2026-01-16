use sqlx::SqlitePool;

use super::models::{Guild, Player, RankInfo};
use crate::error::AppError;

const PLAYER_COLUMN_NAMES: [&str; 13] = [
    "id",
    "puuid",
    "game_name",
    "tag_line",
    "region",
    "profile_icon_id",
    "last_match_id",
    "last_rank_solo_tier",
    "last_rank_solo_rank",
    "last_rank_solo_lp",
    "last_rank_flex_tier",
    "last_rank_flex_rank",
    "last_rank_flex_lp",
];

fn player_columns(alias: Option<&str>) -> String {
    let prefix = alias.map(|a| format!("{a}.")).unwrap_or_default();
    PLAYER_COLUMN_NAMES
        .iter()
        .map(|col| format!("{prefix}{col}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone, Debug)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // === Player operations ===

    pub async fn get_or_create_player(
        &self,
        puuid: &str,
        game_name: &str,
        tag_line: &str,
        region: &str,
    ) -> Result<Player, AppError> {
        let columns = player_columns(None);
        let query = format!(
            r#"
            INSERT INTO players (puuid, game_name, tag_line, region)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(puuid) DO UPDATE SET
                game_name = excluded.game_name,
                tag_line = excluded.tag_line,
                region = excluded.region
            RETURNING {columns}
            "#
        );

        let player = sqlx::query_as::<_, Player>(&query)
            .bind(puuid)
            .bind(game_name)
            .bind(tag_line)
            .bind(region)
            .fetch_one(&self.pool)
            .await?;
        Ok(player)
    }

    pub async fn get_player_by_puuid(&self, puuid: &str) -> Result<Option<Player>, AppError> {
        let columns = player_columns(None);
        let player = sqlx::query_as::<_, Player>(&format!(
            "SELECT {columns} FROM players WHERE puuid = ?"
        ))
        .bind(puuid)
        .fetch_optional(&self.pool)
        .await?;
        Ok(player)
    }

    pub async fn get_player_by_riot_id(
        &self,
        game_name: &str,
        tag_line: &str,
    ) -> Result<Option<Player>, AppError> {
        let columns = player_columns(None);
        let player = sqlx::query_as::<_, Player>(&format!(
            "SELECT {columns} FROM players WHERE LOWER(game_name) = LOWER(?) AND LOWER(tag_line) = LOWER(?)"
        ))
        .bind(game_name)
        .bind(tag_line)
        .fetch_optional(&self.pool)
        .await?;
        Ok(player)
    }

    pub async fn get_all_tracked_players(&self) -> Result<Vec<Player>, AppError> {
        let columns = player_columns(Some("p"));
        let players = sqlx::query_as::<_, Player>(&format!(
            r#"
            SELECT DISTINCT {columns}
            FROM players p
            INNER JOIN guild_players gp ON p.id = gp.player_id
            INNER JOIN guilds g ON gp.guild_id = g.id
            WHERE g.alert_channel_id IS NOT NULL
            "#
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(players)
    }

    pub async fn update_player_last_match(
        &self,
        player_id: i64,
        match_id: &str,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE players SET last_match_id = ? WHERE id = ?")
            .bind(match_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_player_profile_icon(
        &self,
        player_id: i64,
        profile_icon_id: i32,
    ) -> Result<(), AppError> {
        sqlx::query("UPDATE players SET profile_icon_id = ? WHERE id = ?")
            .bind(profile_icon_id)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_player_rank(
        &self,
        player_id: i64,
        solo: Option<&RankInfo>,
        flex: Option<&RankInfo>,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            UPDATE players SET
                last_rank_solo_tier = ?,
                last_rank_solo_rank = ?,
                last_rank_solo_lp = ?,
                last_rank_flex_tier = ?,
                last_rank_flex_rank = ?,
                last_rank_flex_lp = ?
            WHERE id = ?
            "#,
        )
        .bind(solo.map(|r| &r.tier))
        .bind(solo.map(|r| &r.rank))
        .bind(solo.map(|r| r.lp))
        .bind(flex.map(|r| &r.tier))
        .bind(flex.map(|r| &r.rank))
        .bind(flex.map(|r| r.lp))
        .bind(player_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // === Guild operations ===

    pub async fn get_or_create_guild(&self, guild_id: u64) -> Result<Guild, AppError> {
        let id = guild_id as i64;

        if let Some(guild) = self.get_guild(guild_id).await? {
            return Ok(guild);
        }

        sqlx::query("INSERT INTO guilds (id) VALUES (?)")
            .bind(id)
            .execute(&self.pool)
            .await?;

        self.get_guild(guild_id)
            .await?
            .ok_or_else(|| AppError::Database(sqlx::Error::RowNotFound))
    }

    pub async fn get_guild(&self, guild_id: u64) -> Result<Option<Guild>, AppError> {
        let guild =
            sqlx::query_as::<_, Guild>("SELECT id, alert_channel_id FROM guilds WHERE id = ?")
                .bind(guild_id as i64)
                .fetch_optional(&self.pool)
                .await?;
        Ok(guild)
    }

    pub async fn set_guild_alert_channel(
        &self,
        guild_id: u64,
        channel_id: u64,
    ) -> Result<(), AppError> {
        self.get_or_create_guild(guild_id).await?;

        sqlx::query("UPDATE guilds SET alert_channel_id = ? WHERE id = ?")
            .bind(channel_id as i64)
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // === Guild-Player relations ===

    pub async fn add_player_to_guild(
        &self,
        guild_id: u64,
        player_id: i64,
        added_by: u64,
    ) -> Result<(), AppError> {
        self.get_or_create_guild(guild_id).await?;

        sqlx::query(
            "INSERT OR IGNORE INTO guild_players (guild_id, player_id, added_by) VALUES (?, ?, ?)",
        )
        .bind(guild_id as i64)
        .bind(player_id)
        .bind(added_by as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_player_from_guild(
        &self,
        guild_id: u64,
        player_id: i64,
    ) -> Result<bool, AppError> {
        let result = sqlx::query("DELETE FROM guild_players WHERE guild_id = ? AND player_id = ?")
            .bind(guild_id as i64)
            .bind(player_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_guild_players(&self, guild_id: u64) -> Result<Vec<Player>, AppError> {
        let columns = player_columns(Some("p"));
        let players = sqlx::query_as::<_, Player>(&format!(
            r#"
            SELECT {columns}
            FROM players p
            INNER JOIN guild_players gp ON p.id = gp.player_id
            WHERE gp.guild_id = ?
            ORDER BY p.game_name ASC
            "#
        ))
        .bind(guild_id as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(players)
    }

    pub async fn get_guilds_tracking_player(&self, player_id: i64) -> Result<Vec<Guild>, AppError> {
        let guilds = sqlx::query_as::<_, Guild>(
            r#"
            SELECT g.id, g.alert_channel_id
            FROM guilds g
            INNER JOIN guild_players gp ON g.id = gp.guild_id
            WHERE gp.player_id = ? AND g.alert_channel_id IS NOT NULL
            "#,
        )
        .bind(player_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(guilds)
    }

    pub async fn is_player_tracked_in_guild(
        &self,
        guild_id: u64,
        player_id: i64,
    ) -> Result<bool, AppError> {
        let exists = sqlx::query_scalar::<_, i32>(
            "SELECT 1 FROM guild_players WHERE guild_id = ? AND player_id = ?",
        )
        .bind(guild_id as i64)
        .bind(player_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }
}
