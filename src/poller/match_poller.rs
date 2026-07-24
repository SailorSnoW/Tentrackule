use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures::stream::{self, StreamExt};
use poise::serenity_prelude::{ChannelId, CreateAttachment, CreateEmbed, CreateMessage, Http};
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{Span, debug, error, info, instrument, warn};

use crate::db::{Guild, Player, RankInfo, Repository, TrackedPlayer};
use crate::discord::image_gen::{ImageGenerator, MasteryInfo, MatchImageContext};
use crate::error::AppError;
use crate::metrics::Metrics;
use crate::riot::{InfoDto, Platform, RiotClient};

const ACTIVE_WINDOW: Duration = Duration::from_secs(2 * 60 * 60);
const DORMANT_WINDOW: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const IDLE_CADENCE: Duration = Duration::from_secs(5 * 60);
const DORMANT_CADENCE: Duration = Duration::from_secs(15 * 60);

const POLL_CONCURRENCY: usize = 5;

fn cadence_for(activity_age: Duration, active_cadence: Duration) -> Duration {
    if activity_age < ACTIVE_WINDOW {
        active_cadence
    } else if activity_age < DORMANT_WINDOW {
        IDLE_CADENCE.max(active_cadence)
    } else {
        DORMANT_CADENCE.max(active_cadence)
    }
}

fn poll_due(since_last_poll: Duration, activity_age: Duration, active_cadence: Duration) -> bool {
    since_last_poll >= cadence_for(activity_age, active_cadence)
}

#[derive(Default)]
struct PollSchedule {
    last_polled: HashMap<i64, Instant>,
}

impl PollSchedule {
    fn is_due(
        &self,
        player_id: i64,
        activity_age: Duration,
        active_cadence: Duration,
        now: Instant,
    ) -> bool {
        match self.last_polled.get(&player_id) {
            Some(&last) => poll_due(
                now.saturating_duration_since(last),
                activity_age,
                active_cadence,
            ),
            None => true,
        }
    }

    fn mark_polled(&mut self, player_id: i64, now: Instant) {
        self.last_polled.insert(player_id, now);
    }

    fn retain_tracked(&mut self, tracked: &HashSet<i64>) {
        self.last_polled.retain(|id, _| tracked.contains(id));
    }
}

fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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
    metrics: Arc<Metrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = interval(Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let active_cadence = Duration::from_secs(interval_secs);
    let mut schedule = PollSchedule::default();

    info!(interval_secs, "🔄 Match poller started");

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                metrics.record_poll_cycle();
                if let Err(e) =
                    poll_players(&db, &riot, &http, &image_gen, active_cadence, &mut schedule).await
                {
                    error!(error = ?e, "🔄 ❌ Polling cycle failed");
                }
            }
            _ = shutdown.changed() => {
                info!("🔄 🛑 Shutdown requested; poller stopping");
                break;
            }
        }
    }
}

#[instrument(skip_all, fields(due, tracked))]
async fn poll_players(
    db: &Repository,
    riot: &RiotClient,
    http: &Http,
    image_gen: &ImageGenerator,
    active_cadence: Duration,
    schedule: &mut PollSchedule,
) -> Result<(), PollerError> {
    let players: Vec<TrackedPlayer> = db.get_tracked_players_with_activity().await?;

    let tracked_ids: HashSet<i64> = players.iter().map(|tp| tp.player.id).collect();
    schedule.retain_tracked(&tracked_ids);
    let tracked = tracked_ids.len();

    if players.is_empty() {
        debug!("🔄 No players tracked, skipping poll cycle");
        return Ok(());
    }

    let now = Instant::now();
    let now_unix = unix_now_secs();

    let due: Vec<Player> = players
        .into_iter()
        .filter(|tp| {
            let activity_age =
                Duration::from_secs((now_unix - tp.last_activity_secs).max(0) as u64);
            schedule.is_due(tp.player.id, activity_age, active_cadence, now)
        })
        .map(|tp| tp.player)
        .collect();

    let due_count = due.len();
    Span::current().record("tracked", tracked);
    Span::current().record("due", due_count);

    if due.is_empty() {
        debug!(tracked, "🔄 No players due this cycle");
        return Ok(());
    }

    info!(
        due = due_count,
        tracked, "🔄 Polling {due_count} of {tracked} tracked player(s)"
    );

    for player in &due {
        schedule.mark_polled(player.id, now);
    }

    stream::iter(due)
        .for_each_concurrent(POLL_CONCURRENCY, |player| async move {
            if let Err(e) = check_player_match(db, riot, http, image_gen, &player).await {
                warn!(
                    error = ?e,
                    player_id = player.id,
                    riot_id = %player.riot_id(),
                    "🔄 ⚠️ Failed to check player match"
                );
            }
        })
        .await;

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

    let match_ids = riot.get_match_ids(region, &player.puuid, 5).await?;

    let to_process = matches_to_process(&match_ids, player.last_match_id.as_deref());
    if to_process.is_empty() {
        debug!("🔄 No new matches");
        return Ok(());
    }

    for match_id in to_process {
        if let Err(e) = process_match(db, riot, http, image_gen, player, platform, match_id).await {
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

/// Picks the solo- or flex-queue variant of a value, or `None` for any other queue.
fn for_queue<T>(info: &InfoDto, solo: T, flex: T) -> Option<T> {
    if info.is_solo_queue() {
        Some(solo)
    } else if info.is_flex_queue() {
        Some(flex)
    } else {
        None
    }
}

fn matches_to_process<'a>(match_ids: &'a [String], last_seen: Option<&str>) -> Vec<&'a String> {
    let newest_first: Vec<&String> = match last_seen {
        Some(last) => match_ids
            .iter()
            .take_while(|id| id.as_str() != last)
            .collect(),
        None => match_ids.first().into_iter().collect(),
    };
    newest_first.into_iter().rev().collect()
}

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

    let match_data = riot.get_match(region, match_id).await?;

    if !match_data.info.is_supported() {
        debug!(
            queue_id = match_data.info.queue_id,
            match_id, "🔄 Skipping unsupported queue"
        );
        db.update_player_last_match(player.id, match_id).await?;
        return Ok(());
    }

    info!(
        match_id,
        queue = match_data.info.queue_name(),
        "🔄 ✅ New match detected"
    );

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

    let old_rank = for_queue(
        &match_data.info,
        player.solo_rank_info(),
        player.flex_rank_info(),
    )
    .flatten();

    let (new_solo_rank, new_flex_rank) = if is_ranked {
        fetch_rank_info(riot, platform, &player.puuid).await?
    } else {
        (None, None)
    };

    let new_rank = for_queue(
        &match_data.info,
        new_solo_rank.as_ref(),
        new_flex_rank.as_ref(),
    )
    .flatten();

    let queue_id = match_data.info.queue_id;
    if !match_data.info.game_ended_in_early_surrender
        && let Err(e) = db
            .record_match_result(player.id, match_id, participant.win, queue_id)
            .await
    {
        warn!(error = ?e, "🔄 ⚠️ Failed to record match result");
    }

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

    let ctx = MatchImageContext {
        player,
        participant,
        match_info: &match_data.info,
        old_rank: old_rank.as_ref(),
        new_rank,
        streak,
        mastery,
    };

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

    let guilds = db.get_guilds_tracking_player(player.id).await?;
    announce(
        http,
        &guilds,
        image_data.as_deref(),
        fallback_embed.as_ref(),
    )
    .await;

    db.update_player_last_match(player.id, match_id).await?;
    if is_ranked {
        db.update_player_rank(player.id, new_solo_rank.as_ref(), new_flex_rank.as_ref())
            .await?;
    }

    Ok(())
}

/// Posts the alert to every guild watching the player. The card is uploaded once;
/// the remaining guilds re-embed the resulting attachment URL.
async fn announce(
    http: &Http,
    guilds: &[Guild],
    image_data: Option<&[u8]>,
    fallback_embed: Option<&CreateEmbed>,
) {
    let mut reuse_url: Option<String> = None;
    for guild in guilds {
        let Some(channel_id) = guild.alert_channel_id else {
            continue;
        };
        let channel = ChannelId::new(channel_id as u64);

        let message = match (image_data, &reuse_url, fallback_embed) {
            (Some(_), Some(url), _) => CreateMessage::new().embed(CreateEmbed::new().image(url)),
            (Some(bytes), None, _) => CreateMessage::new().add_file(CreateAttachment::bytes(
                bytes,
                ImageGenerator::ATTACHMENT_FILENAME,
            )),
            (None, _, Some(embed)) => CreateMessage::new().embed(embed.clone()),
            (None, _, None) => continue,
        };

        match channel.send_message(http, message).await {
            Ok(sent) => {
                debug!(guild_id = guild.id, channel_id, "🎮 ✅ Alert sent");
                if image_data.is_some() && reuse_url.is_none() {
                    reuse_url = sent.attachments.first().map(|a| a.url.clone());
                }
            }
            Err(e) => {
                error!(
                    error = ?e,
                    guild_id = guild.id,
                    channel_id,
                    "🎮 ❌ Failed to send alert message"
                );
            }
        }
    }
}

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
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    use super::{
        DORMANT_CADENCE, IDLE_CADENCE, PollSchedule, cadence_for, matches_to_process, poll_due,
    };

    const HOUR: Duration = Duration::from_secs(60 * 60);
    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[test]
    fn cadence_tiers_follow_activity_age() {
        let active = Duration::from_secs(60);
        assert_eq!(cadence_for(Duration::ZERO, active), active);
        assert_eq!(cadence_for(HOUR, active), active);
        assert_eq!(cadence_for(3 * HOUR, active), IDLE_CADENCE);
        assert_eq!(cadence_for(8 * DAY, active), DORMANT_CADENCE);
    }

    #[test]
    fn active_cadence_is_a_floor_for_slower_tiers() {
        let active = Duration::from_secs(20 * 60);
        assert_eq!(cadence_for(3 * HOUR, active), active);
        assert_eq!(cadence_for(8 * DAY, active), active);
    }

    #[test]
    fn poll_due_respects_the_tier_cadence() {
        let active = Duration::from_secs(60);
        assert!(!poll_due(Duration::from_secs(30), Duration::ZERO, active));
        assert!(poll_due(active, Duration::ZERO, active));
        assert!(!poll_due(Duration::from_secs(4 * 60), 3 * HOUR, active));
        assert!(poll_due(IDLE_CADENCE, 3 * HOUR, active));
    }

    #[test]
    fn schedule_polls_unseen_players_then_spaces_them() {
        let mut sched = PollSchedule::default();
        let active = Duration::from_secs(60);
        let now = Instant::now();

        assert!(sched.is_due(1, Duration::ZERO, active, now));
        sched.mark_polled(1, now);
        assert!(!sched.is_due(1, Duration::ZERO, active, now));
        assert!(sched.is_due(1, Duration::ZERO, active, now + active));
    }

    #[test]
    fn schedule_forgets_untracked_players() {
        let mut sched = PollSchedule::default();
        let now = Instant::now();
        sched.mark_polled(1, now);
        sched.mark_polled(2, now);

        let tracked: HashSet<i64> = [2].into_iter().collect();
        sched.retain_tracked(&tracked);

        assert!(!sched.last_polled.contains_key(&1));
        assert!(sched.last_polled.contains_key(&2));
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

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
        assert_eq!(run(&["m5", "m4", "m3"], None), ids(&["m5"]));
    }

    #[test]
    fn full_window_when_last_seen_fell_off_it() {
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
