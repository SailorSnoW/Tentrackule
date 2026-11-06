use sqlx::SqlitePool;

use super::models::{Guild, Player, RankInfo, TrackedPlayer};
use crate::error::AppError;

/// Expands to the `Player` column list, each name prefixed by `$p` (use `""`
/// unqualified, `"p."` when the query aliases `players` as `p`).
macro_rules! player_columns {
    ($p:literal) => {
        concat!(
            $p,
            "id, ",
            $p,
            "puuid, ",
            $p,
            "game_name, ",
            $p,
            "tag_line, ",
            $p,
            "region, ",
            $p,
            "last_match_id, ",
            $p,
            "last_rank_solo_tier, ",
            $p,
            "last_rank_solo_rank, ",
            $p,
            "last_rank_solo_lp, ",
            $p,
            "last_rank_flex_tier, ",
            $p,
            "last_rank_flex_rank, ",
            $p,
            "last_rank_flex_lp"
        )
    };
}

const MATCH_HISTORY_KEEP: u32 = 20;

#[derive(Clone, Debug)]
pub struct Repository {
    pool: SqlitePool,
}

impl Repository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get_or_create_player(
        &self,
        puuid: &str,
        game_name: &str,
        tag_line: &str,
        region: &str,
        region_from_riot: bool,
    ) -> Result<Player, AppError> {
        let player = sqlx::query_as::<_, Player>(concat!(
            "INSERT INTO players (puuid, game_name, tag_line, region)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(puuid) DO UPDATE SET
                 game_name = excluded.game_name,
                 tag_line = excluded.tag_line,
                 region = CASE WHEN ? THEN excluded.region ELSE players.region END
             RETURNING ",
            player_columns!("")
        ))
        .bind(puuid)
        .bind(game_name)
        .bind(tag_line)
        .bind(region)
        .bind(region_from_riot)
        .fetch_one(&self.pool)
        .await?;
        Ok(player)
    }

    pub async fn get_player_by_riot_id(
        &self,
        game_name: &str,
        tag_line: &str,
    ) -> Result<Option<Player>, AppError> {
        let player = sqlx::query_as::<_, Player>(concat!(
            "SELECT ",
            player_columns!(""),
            " FROM players WHERE LOWER(game_name) = LOWER(?) AND LOWER(tag_line) = LOWER(?)"
        ))
        .bind(game_name)
        .bind(tag_line)
        .fetch_optional(&self.pool)
        .await?;
        Ok(player)
    }

    pub async fn get_tracked_players_with_activity(&self) -> Result<Vec<TrackedPlayer>, AppError> {
        let players = sqlx::query_as::<_, TrackedPlayer>(concat!(
            "SELECT DISTINCT ",
            player_columns!("p."),
            ",
             COALESCE(
                 (SELECT MAX(mr.created_at) FROM match_results mr WHERE mr.player_id = p.id),
                 p.created_at
             ) AS last_activity_secs
             FROM players p
             INNER JOIN guild_players gp ON p.id = gp.player_id
             INNER JOIN guilds g ON gp.guild_id = g.id
             WHERE g.alert_channel_id IS NOT NULL"
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

    pub async fn record_match_result(
        &self,
        player_id: i64,
        match_id: &str,
        won: bool,
        queue_id: i32,
    ) -> Result<(), AppError> {
        sqlx::query(
            "INSERT OR IGNORE INTO match_results (player_id, match_id, won, queue_id) VALUES (?, ?, ?, ?)",
        )
        .bind(player_id)
        .bind(match_id)
        .bind(won as i32)
        .bind(queue_id)
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            DELETE FROM match_results
            WHERE player_id = ? AND queue_id = ? AND rowid NOT IN (
                SELECT rowid FROM match_results
                WHERE player_id = ? AND queue_id = ?
                ORDER BY created_at DESC, rowid DESC
                LIMIT ?
            )
            "#,
        )
        .bind(player_id)
        .bind(queue_id)
        .bind(player_id)
        .bind(queue_id)
        .bind(MATCH_HISTORY_KEEP as i64)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_recent_results(
        &self,
        player_id: i64,
        queue_id: i32,
        limit: u32,
    ) -> Result<Vec<bool>, AppError> {
        let rows = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT won FROM match_results
            WHERE player_id = ? AND queue_id = ?
            ORDER BY created_at DESC, rowid DESC
            LIMIT ?
            "#,
        )
        .bind(player_id)
        .bind(queue_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().rev().map(|w| w != 0).collect())
    }

    async fn ensure_guild(&self, guild_id: u64) -> Result<(), AppError> {
        sqlx::query("INSERT OR IGNORE INTO guilds (id) VALUES (?)")
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_guild_alert_channel(
        &self,
        guild_id: u64,
        channel_id: u64,
    ) -> Result<(), AppError> {
        self.ensure_guild(guild_id).await?;

        sqlx::query("UPDATE guilds SET alert_channel_id = ? WHERE id = ?")
            .bind(channel_id as i64)
            .bind(guild_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_player_to_guild(
        &self,
        guild_id: u64,
        player_id: i64,
        added_by: u64,
    ) -> Result<(), AppError> {
        self.ensure_guild(guild_id).await?;

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
        let players = sqlx::query_as::<_, Player>(concat!(
            "SELECT ",
            player_columns!("p."),
            " FROM players p
             INNER JOIN guild_players gp ON p.id = gp.player_id
             WHERE gp.guild_id = ?
             ORDER BY p.game_name ASC"
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

    pub async fn count_guild_players(&self, guild_id: u64) -> Result<i64, AppError> {
        let count =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM guild_players WHERE guild_id = ?")
                .bind(guild_id as i64)
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn memory_repo() -> Repository {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open in-memory sqlite");
        crate::db::run_migrations(&pool).await.expect("migrations");
        Repository::new(pool)
    }

    #[tokio::test]
    async fn activity_falls_back_to_created_at_then_tracks_matches() {
        let repo = memory_repo().await;

        let player = repo
            .get_or_create_player("puuid-1", "Alice", "EUW", "EUW1", true)
            .await
            .unwrap();
        repo.set_guild_alert_channel(1, 100).await.unwrap();
        repo.add_player_to_guild(1, player.id, 7).await.unwrap();

        let tracked = repo.get_tracked_players_with_activity().await.unwrap();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].player.id, player.id);
        assert_eq!(tracked[0].player.puuid, "puuid-1");
        let fallback = tracked[0].last_activity_secs;
        assert!(
            fallback > 0,
            "fallback activity should be players.created_at"
        );

        for (mid, ts) in [("EUW1_1", fallback + 10_000), ("EUW1_2", fallback + 5_000)] {
            sqlx::query(
                "INSERT INTO match_results (player_id, match_id, won, queue_id, created_at) \
                 VALUES (?, ?, 1, 420, ?)",
            )
            .bind(player.id)
            .bind(mid)
            .bind(ts)
            .execute(&repo.pool)
            .await
            .unwrap();
        }

        let tracked = repo.get_tracked_players_with_activity().await.unwrap();
        assert_eq!(tracked[0].last_activity_secs, fallback + 10_000);
    }

    #[tokio::test]
    async fn only_alert_enabled_guilds_yield_tracked_players() {
        let repo = memory_repo().await;
        let player = repo
            .get_or_create_player("puuid-2", "Bob", "NA", "NA1", true)
            .await
            .unwrap();
        repo.add_player_to_guild(2, player.id, 7).await.unwrap();

        assert!(
            repo.get_tracked_players_with_activity()
                .await
                .unwrap()
                .is_empty()
        );

        repo.set_guild_alert_channel(2, 200).await.unwrap();
        assert_eq!(
            repo.get_tracked_players_with_activity()
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn counts_players_per_guild() {
        let repo = memory_repo().await;
        assert_eq!(repo.count_guild_players(1).await.unwrap(), 0);

        for (i, puuid) in ["p-a", "p-b", "p-c"].into_iter().enumerate() {
            let player = repo
                .get_or_create_player(puuid, "Name", "TAG", "euw1", true)
                .await
                .unwrap();
            let guild = if i < 2 { 1 } else { 2 };
            repo.add_player_to_guild(guild, player.id, 7).await.unwrap();
        }

        assert_eq!(repo.count_guild_players(1).await.unwrap(), 2);
        assert_eq!(repo.count_guild_players(2).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn only_riot_resolved_regions_overwrite_a_stored_player() {
        let repo = memory_repo().await;
        let player = repo
            .get_or_create_player("puuid-3", "Carol", "EUW", "euw1", true)
            .await
            .unwrap();
        assert_eq!(player.region, "euw1");

        let hinted = repo
            .get_or_create_player("puuid-3", "Carol", "EUW", "na1", false)
            .await
            .unwrap();
        assert_eq!(hinted.id, player.id);
        assert_eq!(
            hinted.region, "euw1",
            "a user-supplied region must not overwrite the stored one"
        );

        let resolved = repo
            .get_or_create_player("puuid-3", "Carol", "EUW", "eun1", true)
            .await
            .unwrap();
        assert_eq!(resolved.region, "eun1");

        let fresh = repo
            .get_or_create_player("puuid-4", "Dave", "NA", "na1", false)
            .await
            .unwrap();
        assert_eq!(
            fresh.region, "na1",
            "a new player falls back to the requested region"
        );
    }
}
