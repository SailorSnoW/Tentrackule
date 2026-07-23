use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{ChannelId, CreateAttachment, CreateEmbed, CreateMessage, Http};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{Span, debug, error, info, instrument, warn};

use crate::db::{Player, RankInfo, Repository};
use crate::discord::image_gen::{ImageGenerator, MasteryInfo, MatchImageContext};
use crate::error::AppError;
use crate::riot::{Platform, RiotClient};

#[derive(Debug, thiserror::Error)]
enum PollerError {
    #[error(transparent)]
    App(#[from] AppError),
    #[error("Player {player_puuid} not found in match {match_id}")]
    PlayerNotFoundInMatch {
        player_puuid: String,
        match_id: String,
    },
}

pub async fn start_polling(
    db: Repository,
    riot: RiotClient,
    http: Arc<Http>,
    image_gen: Arc<ImageGenerator>,
    interval_secs: u64,
) {
    let mut interval = interval(Duration::from_secs(interval_secs));
    // If a cycle overruns the interval, space the next tick out instead of
    // firing a burst of catch-up ticks back-to-back (the default Burst).
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    info!(interval_secs, "🔄 Match poller started");

    loop {
        interval.tick().await;

        if let Err(e) = poll_players(&db, &riot, &http, &image_gen).await {
            error!(error = ?e, "🔄 ❌ Polling cycle failed");
        }
    }
}

#[instrument(skip_all, fields(player_count))]
async fn poll_players(
    db: &Repository,
    riot: &RiotClient,
    http: &Http,
    image_gen: &ImageGenerator,
) -> Result<(), PollerError> {
    let players = db.get_all_tracked_players().await?;

    if players.is_empty() {
        debug!("🔄 No players tracked, skipping poll cycle");
        return Ok(());
    }

    Span::current().record("player_count", players.len());
    info!(
        count = players.len(),
        "🔄 Polling {} player(s)",
        players.len()
    );

    for player in players {
        if let Err(e) = check_player_match(db, riot, http, image_gen, &player).await {
            warn!(
                error = ?e,
                player_id = player.id,
                riot_id = %player.riot_id(),
                "🔄 ⚠️ Failed to check player match"
            );
        }
    }

    Ok(())
}

#[instrument(
    skip(db, riot, http, image_gen, player),
    fields(
        player_id = player.id,
        riot_id = %player.riot_id(),
        region = %player.region
    )
)]
async fn check_player_match(
    db: &Repository,
    riot: &RiotClient,
    http: &Http,
    image_gen: &ImageGenerator,
    player: &Player,
) -> Result<(), PollerError> {
    let platform: Platform = player.region.parse()?;
    let region = platform.to_region();

    // Pull the last few IDs (same 1-request cost as pulling one) so a burst of
    // games between polls — after a restart or a slow cycle — is caught up in
    // order, instead of only the newest being announced and the rest lost.
    let match_ids = riot.get_match_ids(region, &player.puuid, 5).await?;

    // Oldest→newest, so the streak history and `last_match_id` advance in the
    // real order the games were played.
    let to_process = matches_to_process(&match_ids, player.last_match_id.as_deref());
    if to_process.is_empty() {
        debug!("🔄 No new matches");
        return Ok(());
    }

    for match_id in to_process {
        if let Err(e) = process_match(db, riot, http, image_gen, player, platform, match_id).await {
            // Stop at the first failure instead of skipping past an unprocessed
            // match: `last_match_id` stays put and the next cycle retries from
            // here. Any newer caught-up matches wait for that retry.
            warn!(
                error = ?e,
                match_id = %match_id,
                "🔄 ⚠️ Failed to process match; will retry next cycle"
            );
            break;
        }
    }

    Ok(())
}

/// Not-yet-seen matches from Riot's newest-first id list, returned in the order
/// they should be processed (oldest→newest). With no known boundary — a
/// brand-new player, or an init that never set `last_match_id` — only the newest
/// is returned, so a back-catalogue is never replayed as if it just happened.
fn matches_to_process<'a>(match_ids: &'a [String], last_seen: Option<&str>) -> Vec<&'a String> {
    let newest_first: Vec<&String> = match last_seen {
        // `last_seen` is (or once was) the newest id, so it's never past the head
        // of the list; everything ahead of it is new. If it has fallen off the
        // window entirely, all of them are new — the intended post-downtime
        // catch-up.
        Some(last) => match_ids
            .iter()
            .take_while(|id| id.as_str() != last)
            .collect(),
        None => match_ids.first().into_iter().collect(),
    };
    // Riot gives newest-first; process oldest-first.
    newest_first.into_iter().rev().collect()
}

/// Process one newly-seen match: fetch its detail, announce the card to every
/// tracking guild, and advance `last_match_id` (and the rank, for ranked
/// queues). Split out so `check_player_match` can replay a backlog in order.
#[instrument(
    skip(db, riot, http, image_gen, player, platform),
    fields(player_id = player.id, riot_id = %player.riot_id())
)]
async fn process_match(
    db: &Repository,
    riot: &RiotClient,
    http: &Http,
    image_gen: &ImageGenerator,
    player: &Player,
    platform: Platform,
    match_id: &str,
) -> Result<(), PollerError> {
    let region = platform.to_region();

    // Get match details
    let match_data = riot.get_match(region, match_id).await?;

    // Skip unsupported game modes
    if !match_data.info.is_supported() {
        debug!(
            queue_id = match_data.info.queue_id,
            match_id, "🔄 Skipping unsupported queue"
        );
        // Still update last_match_id so we don't check this match again
        db.update_player_last_match(player.id, match_id).await?;
        return Ok(());
    }

    info!(
        match_id,
        queue = match_data.info.queue_name(),
        "🔄 ✅ New match detected"
    );

    // Find the player's participant data
    let participant = match_data
        .info
        .participants
        .iter()
        .find(|p| p.puuid == player.puuid)
        .ok_or_else(|| PollerError::PlayerNotFoundInMatch {
            player_puuid: player.puuid.clone(),
            match_id: match_id.to_string(),
        })?;

    let is_ranked = match_data.info.is_ranked();

    // Only ranked cards (420/440) show a rank delta; other queues show mastery
    // instead. So off-ranked we skip the league-v4 call entirely — and don't
    // persist a rank we wouldn't display. `old_rank` is the stored rank from the
    // previous ranked result, naturally `None` off-ranked.
    let old_rank = if match_data.info.is_solo_queue() {
        player.solo_rank_info()
    } else if match_data.info.is_flex_queue() {
        player.flex_rank_info()
    } else {
        None
    };

    let (new_solo_rank, new_flex_rank) = if is_ranked {
        fetch_rank_info(riot, platform, &player.puuid).await?
    } else {
        (None, None)
    };

    let new_rank = if match_data.info.is_solo_queue() {
        new_solo_rank.as_ref()
    } else if match_data.info.is_flex_queue() {
        new_flex_rank.as_ref()
    } else {
        None
    };

    // Record this match's outcome for the streak history (Phase 4). Remakes
    // aren't a real result, so they're skipped; the insert is idempotent, so a
    // re-poll before `last_match_id` advances won't double-count.
    let queue_id = match_data.info.queue_id;
    if !match_data.info.game_ended_in_early_surrender
        && let Err(e) = db
            .record_match_result(player.id, match_id, participant.win, queue_id)
            .await
    {
        warn!(error = ?e, "🔄 ⚠️ Failed to record match result");
    }

    // Ranked cards show a recent-form bar (last 5 in this queue, including this
    // game); non-ranked cards show champion mastery instead. Only compute the
    // one the card will actually display, so a normal game doesn't hit the
    // mastery endpoint needlessly.
    let (streak, mastery) = if is_ranked {
        let streak = db
            .get_recent_results(player.id, queue_id, 5)
            .await
            .unwrap_or_default();
        (streak, None)
    } else {
        let mastery = fetch_mastery(
            riot,
            image_gen,
            platform,
            &player.puuid,
            &participant.champion_name,
        )
        .await;
        (Vec::new(), mastery)
    };

    // Build image
    let ctx = MatchImageContext {
        player,
        participant,
        match_info: &match_data.info,
        old_rank: old_rank.as_ref(),
        new_rank,
        streak,
        mastery,
    };

    // Render the match card. A renderer outage must never block the
    // announcement, so on failure we fall back to a text embed instead of
    // bailing out (which would also stall `last_match_id` and retry forever).
    let image_data: Option<Arc<[u8]>> = match image_gen.generate_match_image(&ctx).await {
        Ok(data) => Some(data.into()),
        Err(e) => {
            warn!(
                error = ?e,
                match_id,
                "🖼️ ⚠️ Card render failed; falling back to a text embed"
            );
            None
        }
    };
    let fallback_embed = image_data.is_none().then(|| build_fallback_embed(&ctx));

    // Get all guilds tracking this player
    let guilds = db.get_guilds_tracking_player(player.id).await?;

    // Send the card (or text fallback) to all guilds
    for guild in guilds {
        if let Some(channel_id) = guild.alert_channel_id {
            let channel = ChannelId::new(channel_id as u64);
            let message = match (&image_data, &fallback_embed) {
                (Some(bytes), _) => {
                    let attachment =
                        CreateAttachment::bytes(bytes.as_ref(), ImageGenerator::ATTACHMENT_FILENAME);
                    CreateMessage::new().add_file(attachment)
                }
                (None, Some(embed)) => CreateMessage::new().embed(embed.clone()),
                // Unreachable: `fallback_embed` is always built when there is no
                // image, but skip defensively rather than send an empty message.
                (None, None) => continue,
            };

            if let Err(e) = channel.send_message(http, message).await {
                error!(
                    error = ?e,
                    guild_id = guild.id,
                    channel_id,
                    "🎮 ❌ Failed to send alert message"
                );
            } else {
                debug!(guild_id = guild.id, channel_id, "🎮 ✅ Alert sent");
            }
        }
    }

    // Advance the cursor; only persist the rank for ranked queues, since off-
    // ranked we never fetched one (and the stored rank still reflects the last
    // ranked result, which is what the next ranked delta compares against).
    db.update_player_last_match(player.id, match_id).await?;
    if is_ranked {
        db.update_player_rank(player.id, new_solo_rank.as_ref(), new_flex_rank.as_ref())
            .await?;
    }

    Ok(())
}

/// Best-effort champion-mastery lookup for the non-ranked footer block. Resolves
/// the champion's numeric id from the DDragon tables, then queries mastery; any
/// miss (unknown champion, 404 "never played", API error) simply omits the
/// block rather than failing the card.
async fn fetch_mastery(
    riot: &RiotClient,
    image_gen: &ImageGenerator,
    platform: Platform,
    puuid: &str,
    champion_name: &str,
) -> Option<MasteryInfo> {
    let champion_id = image_gen.champion_id(champion_name)?;
    match riot
        .get_champion_mastery(platform, puuid, champion_id)
        .await
    {
        Ok(m) => Some(MasteryInfo {
            level: m.champion_level,
            points: m.champion_points,
        }),
        Err(e) => {
            debug!(error = ?e, champion = champion_name, "🔄 No champion mastery available");
            None
        }
    }
}

/// A compact text embed used when card rendering is unavailable, so a tracked
/// match is still announced instead of being silently dropped.
fn build_fallback_embed(ctx: &MatchImageContext<'_>) -> CreateEmbed {
    let p = ctx.participant;
    let info = ctx.match_info;

    let (icon, outcome, color) = if info.game_ended_in_early_surrender {
        ("🔄", "Remake", 0x9AA2AD)
    } else if p.win {
        ("🏆", "Victory", 0x3BA55D)
    } else {
        ("💀", "Defeat", 0xE84057)
    };

    let mut embed = CreateEmbed::new()
        .title(format!("{icon} {outcome} — {}", ctx.player.riot_id()))
        .color(color)
        .field("Champion", &p.champion_name, true)
        .field(
            "KDA",
            format!(
                "{}/{}/{} ({:.2})",
                p.kills,
                p.deaths,
                p.assists,
                p.kda_ratio()
            ),
            true,
        )
        .field(
            "Queue",
            format!("{} · {}", info.queue_name(), info.duration_formatted()),
            true,
        );

    if let Some(rank) = ctx.new_rank {
        embed = embed.field(
            "Rank",
            format!("{} · {} LP", rank.display_tier(), rank.lp),
            true,
        );
    }

    embed
}

async fn fetch_rank_info(
    riot: &RiotClient,
    platform: Platform,
    puuid: &str,
) -> Result<(Option<RankInfo>, Option<RankInfo>), PollerError> {
    let entries = riot.get_league_entries_by_puuid(platform, puuid).await?;

    let mut solo_rank = None;
    let mut flex_rank = None;

    for entry in entries {
        let rank_info = RankInfo {
            tier: entry.tier.clone(),
            rank: entry.rank.clone(),
            lp: entry.league_points,
            wins: Some(entry.wins),
            losses: Some(entry.losses),
        };

        if entry.is_solo_queue() {
            solo_rank = Some(rank_info);
        } else if entry.is_flex_queue() {
            flex_rank = Some(rank_info);
        }
    }

    Ok((solo_rank, flex_rank))
}

#[cfg(test)]
mod tests {
    use super::matches_to_process;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Run the selector and materialise the borrowed result as owned ids.
    fn run(list: &[&str], last_seen: Option<&str>) -> Vec<String> {
        let owned = ids(list);
        matches_to_process(&owned, last_seen)
            .into_iter()
            .cloned()
            .collect()
    }

    #[test]
    fn processes_everything_newer_than_last_oldest_first() {
        let got = run(&["m5", "m4", "m3", "m2", "m1"], Some("m2"));
        assert_eq!(got, ids(&["m3", "m4", "m5"]));
    }

    #[test]
    fn empty_when_the_latest_is_already_seen() {
        assert!(run(&["m5", "m4"], Some("m5")).is_empty());
    }

    #[test]
    fn only_the_newest_when_there_is_no_boundary() {
        // Init failed / brand-new player: never replay the back-catalogue.
        assert_eq!(run(&["m5", "m4", "m3"], None), ids(&["m5"]));
    }

    #[test]
    fn full_window_when_last_seen_fell_off_it() {
        // Downtime longer than the window: all five are new, in order.
        assert_eq!(
            run(&["m5", "m4", "m3"], Some("m0")),
            ids(&["m3", "m4", "m5"])
        );
    }

    #[test]
    fn empty_match_list_yields_nothing() {
        assert!(run(&[], Some("m5")).is_empty());
        assert!(run(&[], None).is_empty());
    }
}
