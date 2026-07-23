use sqlx::SqlitePool;

use super::models::{Guild, Player, RankInfo, TrackedPlayer};
use crate::error::AppError;

const PLAYER_COLUMN_NAMES: [&str; 12] = [
    "id",
    "puuid",
    "game_name",
    "tag_line",
    "region",
    "last_match_id",
    "last_rank_solo_tier",
    "last_rank_solo_rank",
    "last_rank_solo_lp",
    "last_rank_flex_tier",
    "last_rank_flex_rank",
    "last_rank_flex_lp",
];

/// Rows kept per `(player, queue)` in `match_results`. The card only reads the
/// last 5; the margin keeps the table bounded without touching recent history.
const MATCH_HISTORY_KEEP: u32 = 20;

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

    /// Every player tracked by at least one alert-enabled guild, each carrying
    /// the unix-seconds timestamp of their most recent activity. Activity is the
    /// newest recorded match result, falling back to when the player was first
    /// added (so a freshly-tracked player reads as "active" and is polled fast
    /// until their first game, rather than looking dormant). Drives the poller's
    /// adaptive cadence without any extra Riot call.
    pub async fn get_tracked_players_with_activity(&self) -> Result<Vec<TrackedPlayer>, AppError> {
        let columns = player_columns(Some("p"));
        let players = sqlx::query_as::<_, TrackedPlayer>(&format!(
            r#"
            SELECT DISTINCT {columns},
                COALESCE(
                    (SELECT MAX(mr.created_at) FROM match_results mr WHERE mr.player_id = p.id),
                    p.created_at
                ) AS last_activity_secs
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

    // === Match-result history (streak) ===

    /// Record one processed match's outcome. Idempotent on `(player_id,
    /// match_id)` so a re-poll of the same match never double-counts. Callers
    /// skip remakes (they are not a real win/loss). The history is pruned to
    /// the newest [`MATCH_HISTORY_KEEP`] rows per player+queue.
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

        // Keep the history bounded: only the newest rows feed the streak bar.
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

    /// The most recent `limit` outcomes for a player in one queue, ordered
    /// oldest→newest (so the last element is the latest game) for the streak bar.
    pub async fn get_recent_results(
        &self,
        player_id: i64,
        queue_id: i32,
        limit: u32,
    ) -> Result<Vec<bool>, AppError> {
        // `rowid` breaks ties within the same `created_at` second (insertion order).
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
        // Query yields newest→oldest; reverse so the bar reads old→new (L→R).
        Ok(rows.into_iter().rev().map(|w| w != 0).collect())
    }

    // === Guild operations ===

    /// Make sure the guild row exists (no-op when it already does).
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

    // === Guild-Player relations ===

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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// A migrated, single-connection in-memory database. `sqlite::memory:` gives
    /// each connection its own database, so the pool is pinned to one connection
    /// to keep every query on the same in-memory schema.
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

    /// The activity timestamp falls back to `players.created_at` when no game is
    /// recorded, then tracks the newest `match_results.created_at` once games are.
    /// Also exercises the `#[sqlx(flatten)]` mapping of the joined row.
    #[tokio::test]
    async fn activity_falls_back_to_created_at_then_tracks_matches() {
        let repo = memory_repo().await;

        let player = repo
            .get_or_create_player("puuid-1", "Alice", "EUW", "EUW1")
            .await
            .unwrap();
        repo.set_guild_alert_channel(1, 100).await.unwrap();
        repo.add_player_to_guild(1, player.id, 7).await.unwrap();

        // No recorded games yet → activity is the player's own creation time.
        let tracked = repo.get_tracked_players_with_activity().await.unwrap();
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].player.id, player.id);
        assert_eq!(tracked[0].player.puuid, "puuid-1");
        let fallback = tracked[0].last_activity_secs;
        assert!(
            fallback > 0,
            "fallback activity should be players.created_at"
        );

        // Record two games; the newest created_at must win the COALESCE/MAX.
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

    /// A player is only returned once their guild has an alert channel — the
    /// poller relies on this set to prune its in-memory schedule.
    #[tokio::test]
    async fn only_alert_enabled_guilds_yield_tracked_players() {
        let repo = memory_repo().await;
        let player = repo
            .get_or_create_player("puuid-2", "Bob", "NA", "NA1")
            .await
            .unwrap();
        repo.add_player_to_guild(2, player.id, 7).await.unwrap();

        // Guild has no alert channel → excluded.
        assert!(
            repo.get_tracked_players_with_activity()
                .await
                .unwrap()
                .is_empty()
        );

        // Set the channel → now tracked.
        repo.set_guild_alert_channel(2, 200).await.unwrap();
        assert_eq!(
            repo.get_tracked_players_with_activity()
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
