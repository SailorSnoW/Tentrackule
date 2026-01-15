use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine;
use reqwest::Client;
use tiny_skia::Pixmap;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};
use usvg::fontdb::Database;
use usvg::{Options, Tree};

use crate::db::{Player, RankInfo};
use crate::error::AppError;
use crate::riot::{InfoDto, ParticipantDto};

const SVG_TEMPLATE: &str = include_str!("../../assets/match_template.svg");

// Cache configuration
const CACHE_TTL_HOURS: u64 = 24 * 7; // 7 days
const CACHE_MAX_SIZE_MB: u64 = 100; // 100 MB max
const CACHE_DIR: &str = ".cache/images";

/// Metadata for cached images
#[derive(Debug, Clone)]
struct CacheEntry {
    data_uri: String,
    size_bytes: usize,
    created_at: SystemTime,
}

/// Cache for Data Dragon images with disk persistence, TTL, and size limit
pub struct ImageCache {
    memory_cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    cache_dir: PathBuf,
    ttl: Duration,
    max_size_bytes: u64,
}

impl ImageCache {
    pub async fn new() -> Self {
        let cache_dir = PathBuf::from(CACHE_DIR);

        // Create cache directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&cache_dir).await {
            warn!(error = ?e, "🖼️ ⚠️ Failed to create cache directory");
        }

        let cache = Self {
            memory_cache: Arc::new(RwLock::new(HashMap::new())),
            cache_dir,
            ttl: Duration::from_secs(CACHE_TTL_HOURS * 3600),
            max_size_bytes: CACHE_MAX_SIZE_MB * 1024 * 1024,
        };

        // Load existing cache from disk
        cache.load_from_disk().await;

        cache
    }

    /// Load cached images from disk into memory
    async fn load_from_disk(&self) {
        let mut entries = match fs::read_dir(&self.cache_dir).await {
            Ok(entries) => entries,
            Err(_) => return,
        };

        let mut loaded_count = 0;
        let mut expired_count = 0;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();

            if path.extension().map(|e| e != "png").unwrap_or(true) {
                continue;
            }

            // Check file age for TTL
            if let Ok(metadata) = fs::metadata(&path).await
                && let Ok(modified) = metadata.modified()
            {
                if modified.elapsed().unwrap_or(Duration::MAX) > self.ttl {
                    // Expired, delete it
                    let _ = fs::remove_file(&path).await;
                    expired_count += 1;
                    continue;
                }

                // Load into memory
                if let Ok(bytes) = fs::read(&path).await {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let data_uri = format!("data:image/png;base64,{}", b64);

                    // Extract URL from filename
                    if let Some(url) = self
                        .filename_to_url(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
                    {
                        let entry = CacheEntry {
                            data_uri,
                            size_bytes: bytes.len(),
                            created_at: modified,
                        };

                        let mut cache = self.memory_cache.write().await;
                        cache.insert(url, entry);
                        loaded_count += 1;
                    }
                }
            }
        }

        if loaded_count > 0 || expired_count > 0 {
            info!(
                loaded = loaded_count,
                expired = expired_count,
                "🖼️ Cache loaded from disk"
            );
        }
    }

    /// Convert URL to safe filename
    fn url_to_filename(&self, url: &str) -> String {
        // Create a hash-based filename to avoid path issues
        let hash = url
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        format!("{:016x}", hash)
    }

    /// Convert filename back to URL (for known patterns)
    fn filename_to_url(&self, filename: &str) -> Option<String> {
        // We can't reverse the hash, but we store the URL in memory anyway
        // This is just for initial load - we'll match by hash
        Some(filename.to_string())
    }

    /// Get cache file path for a URL
    fn get_cache_path(&self, url: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}.png", self.url_to_filename(url)))
    }

    /// Calculate total cache size
    async fn total_cache_size(&self) -> u64 {
        let cache = self.memory_cache.read().await;
        cache.values().map(|e| e.size_bytes as u64).sum()
    }

    /// Evict oldest entries until under size limit
    async fn evict_if_needed(&self) {
        let current_size = self.total_cache_size().await;

        if current_size <= self.max_size_bytes {
            return;
        }

        let mut cache = self.memory_cache.write().await;

        // Sort by age and remove oldest
        let mut entries: Vec<_> = cache
            .iter()
            .map(|(k, v)| (k.clone(), v.created_at))
            .collect();
        entries.sort_by(|a, b| a.1.cmp(&b.1));

        let mut freed: u64 = 0;
        let target_free = current_size - (self.max_size_bytes * 80 / 100); // Free to 80% capacity

        for (url, _) in entries {
            if freed >= target_free {
                break;
            }

            if let Some(entry) = cache.remove(&url) {
                freed += entry.size_bytes as u64;

                // Also remove from disk
                let path = self.get_cache_path(&url);
                let _ = fs::remove_file(&path).await;
            }
        }

        if freed > 0 {
            debug!(
                freed_mb = freed / 1024 / 1024,
                "🖼️ Cache eviction completed"
            );
        }
    }

    /// Check if entry is expired
    fn is_expired(&self, entry: &CacheEntry) -> bool {
        entry.created_at.elapsed().unwrap_or(Duration::MAX) > self.ttl
    }

    async fn get_or_fetch(&self, http: &Client, url: &str) -> Option<String> {
        // Check memory cache first
        {
            let cache = self.memory_cache.read().await;
            if let Some(entry) = cache.get(url)
                && !self.is_expired(entry)
            {
                trace!(url, "🖼️ Memory cache hit");
                return Some(entry.data_uri.clone());
            }
        }

        // Check disk cache
        let cache_path = self.get_cache_path(url);
        if cache_path.exists()
            && let Ok(metadata) = fs::metadata(&cache_path).await
            && let Ok(modified) = metadata.modified()
        {
            if modified.elapsed().unwrap_or(Duration::MAX) <= self.ttl {
                // Valid disk cache
                if let Ok(bytes) = fs::read(&cache_path).await {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let data_uri = format!("data:image/png;base64,{}", b64);

                    // Store in memory
                    let entry = CacheEntry {
                        data_uri: data_uri.clone(),
                        size_bytes: bytes.len(),
                        created_at: modified,
                    };

                    let mut cache = self.memory_cache.write().await;
                    cache.insert(url.to_string(), entry);

                    trace!(url, "🖼️ Disk cache hit");
                    return Some(data_uri);
                }
            } else {
                // Expired, remove
                let _ = fs::remove_file(&cache_path).await;
            }
        }

        // Fetch from network
        trace!(url, "🖼️ Fetching image");
        match http.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                match response.bytes().await {
                    Ok(bytes) => {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        let data_uri = format!("data:image/png;base64,{}", b64);

                        // Save to disk
                        if let Err(e) = fs::write(&cache_path, &bytes).await {
                            warn!(error = ?e, "🖼️ ⚠️ Failed to write cache file");
                        }

                        // Store in memory
                        let entry = CacheEntry {
                            data_uri: data_uri.clone(),
                            size_bytes: bytes.len(),
                            created_at: SystemTime::now(),
                        };

                        {
                            let mut cache = self.memory_cache.write().await;
                            cache.insert(url.to_string(), entry);
                        }

                        // Check if eviction needed
                        self.evict_if_needed().await;

                        debug!(url, "🖼️ ✅ Image cached");
                        Some(data_uri)
                    }
                    Err(e) => {
                        warn!(url, error = ?e, "🖼️ ⚠️ Failed to read image bytes");
                        None
                    }
                }
            }
            Ok(response) => {
                warn!(
                    url,
                    status = response.status().as_u16(),
                    "🖼️ ⚠️ Image fetch failed"
                );
                None
            }
            Err(e) => {
                warn!(url, error = ?e, "🖼️ ⚠️ Image request failed");
                None
            }
        }
    }
}

pub struct MatchImageContext<'a> {
    pub player: &'a Player,
    pub participant: &'a ParticipantDto,
    pub match_info: &'a InfoDto,
    pub old_rank: Option<&'a RankInfo>,
    pub new_rank: Option<&'a RankInfo>,
}

pub struct ImageGenerator {
    http: Client,
    cache: ImageCache,
    ddragon_version: String,
    fontdb: Database,
}

impl ImageGenerator {
    pub async fn new(ddragon_version: String) -> Self {
        let http = Client::builder()
            .user_agent("Tentrackule/2.0")
            .build()
            .expect("Failed to build HTTP client");

        // Load system fonts
        let mut fontdb = Database::new();
        fontdb.load_system_fonts();
        let font_count = fontdb.len();
        info!(font_count, "🖼️ Loaded system fonts");

        // Initialize cache (loads from disk)
        let cache = ImageCache::new().await;

        Self {
            http,
            cache,
            ddragon_version,
            fontdb,
        }
    }

    pub async fn generate_match_image(
        &self,
        ctx: &MatchImageContext<'_>,
    ) -> Result<Vec<u8>, AppError> {
        let svg = self.build_svg(ctx).await;
        self.render_svg_to_png(&svg)
    }

    async fn build_svg(&self, ctx: &MatchImageContext<'_>) -> String {
        let participant = ctx.participant;
        let match_info = ctx.match_info;
        let is_win = participant.win;
        let is_remake = match_info.game_ended_in_early_surrender;

        // Result styling based on outcome
        let (banner_gradient, result_glow, result_text) = if is_remake {
            ("url(#remakeGradient)", "", "REMAKE")
        } else if is_win {
            ("url(#victoryGradient)", "url(#victoryGlow)", "VICTORY")
        } else {
            ("url(#defeatGradient)", "url(#defeatGlow)", "DEFEAT")
        };

        // Fetch images
        let champion_image = self
            .fetch_champion_image(&participant.champion_name)
            .await
            .unwrap_or_default();

        let profile_icon = if let Some(icon_id) = ctx.player.profile_icon_id {
            self.fetch_profile_icon(icon_id).await.unwrap_or_default()
        } else {
            String::new()
        };

        // Fetch item images
        let items = participant.items();
        let mut item_images: Vec<Option<String>> = Vec::with_capacity(7);
        for item_id in items {
            if item_id > 0 {
                item_images.push(self.fetch_item_image(item_id).await);
            } else {
                item_images.push(None);
            }
        }

        // Stats
        let cs = participant.cs_total();
        let cs_per_min = format!("{:.1}", participant.cs_per_minute(match_info.game_duration));
        let damage = format_damage(participant.total_damage_dealt_to_champions);
        let vision = participant.vision_score.to_string();
        let role = participant.position_display();
        let gold = participant.gold_formatted();

        // Rank info
        let (rank_display, lp_change, lp_color, lp_x) = Self::format_rank_info(ctx);

        // Build SVG by replacing placeholders
        let mut svg = SVG_TEMPLATE.to_string();

        // Basic replacements
        svg = svg.replace("{{banner_gradient}}", banner_gradient);
        svg = svg.replace("{{result_glow}}", result_glow);
        svg = svg.replace("{{result_text}}", result_text);
        svg = svg.replace("{{champion_image}}", &champion_image);
        svg = svg.replace("{{profile_icon}}", &profile_icon);
        svg = svg.replace(
            "{{player_name}}",
            &format!("{}#{}", ctx.player.game_name, ctx.player.tag_line),
        );
        svg = svg.replace("{{queue_type}}", match_info.queue_name());
        svg = svg.replace("{{duration}}", &match_info.duration_formatted());
        svg = svg.replace("{{champion_name}}", &participant.champion_name);
        svg = svg.replace("{{kills}}", &participant.kills.to_string());
        svg = svg.replace("{{deaths}}", &participant.deaths.to_string());
        svg = svg.replace("{{assists}}", &participant.assists.to_string());
        svg = svg.replace("{{kda_ratio}}", &format!("{:.2}", participant.kda_ratio()));
        svg = svg.replace("{{cs}}", &cs.to_string());
        svg = svg.replace("{{cs_per_min}}", &cs_per_min);
        svg = svg.replace("{{damage}}", &damage);
        svg = svg.replace("{{vision}}", &vision);
        svg = svg.replace("{{role}}", role);
        svg = svg.replace("{{gold}}", &gold);
        svg = svg.replace("{{rank_display}}", &rank_display);
        svg = svg.replace("{{lp_change}}", &lp_change);
        svg = svg.replace("{{lp_color}}", &lp_color);
        svg = svg.replace("{{lp_x}}", &lp_x);
        svg = svg.replace("{{patch}}", match_info.patch_version());

        // Handle conditional item images with mustache-like syntax
        for (i, item_opt) in item_images.iter().enumerate() {
            let tag_open = format!("{{{{#item{}}}}}", i);
            let tag_close = format!("{{{{/item{}}}}}", i);
            let placeholder = format!("{{{{item{}}}}}", i);

            if let Some(data_uri) = item_opt {
                // Item exists - keep the element and replace placeholder
                svg = svg.replace(&tag_open, "");
                svg = svg.replace(&tag_close, "");
                svg = svg.replace(&placeholder, data_uri);
            } else {
                // No item - remove entire conditional block
                if let (Some(start), Some(end)) = (svg.find(&tag_open), svg.find(&tag_close)) {
                    let end_with_tag = end + tag_close.len();
                    svg.replace_range(start..end_with_tag, "");
                }
            }
        }

        // Handle ARAM-specific layout (2 stats) vs normal layout (4 stats)
        let is_aram = match_info.queue_id == 450;
        svg = Self::handle_conditional_block(&svg, "stats_normal", !is_aram);
        svg = Self::handle_conditional_block(&svg, "stats_aram", is_aram);

        svg
    }

    /// Handle mustache-like conditional blocks: {{#name}}content{{/name}}
    fn handle_conditional_block(svg: &str, name: &str, show: bool) -> String {
        let tag_open = format!("{{{{#{}}}}}", name);
        let tag_close = format!("{{{{/{}}}}}", name);

        if show {
            // Keep content, remove tags
            svg.replace(&tag_open, "").replace(&tag_close, "")
        } else {
            // Remove entire block
            let mut result = svg.to_string();
            if let (Some(start), Some(end)) = (result.find(&tag_open), result.find(&tag_close)) {
                let end_with_tag = end + tag_close.len();
                result.replace_range(start..end_with_tag, "");
            }
            result
        }
    }

    fn format_rank_info(ctx: &MatchImageContext<'_>) -> (String, String, String, String) {
        if !ctx.match_info.is_ranked() {
            return (
                String::new(),
                String::new(),
                "transparent".to_string(),
                "0".to_string(),
            );
        }

        let rank_display = ctx
            .new_rank
            .map(|r| format!("{} {} • {} LP", capitalize(&r.tier), r.rank, r.lp))
            .unwrap_or_default();

        let lp_diff = calculate_lp_diff(ctx.old_rank, ctx.new_rank);

        // Calculate approximate x position for LP change based on rank_display length
        let lp_x = 60 + (rank_display.len() as i32 * 9);

        let (lp_change, lp_color) = match lp_diff {
            Some(diff) if diff > 0 => (format!("(+{})", diff), "#4CAF50".to_string()),
            Some(diff) if diff < 0 => (format!("({})", diff), "#E84057".to_string()),
            _ => (String::new(), "transparent".to_string()),
        };

        (rank_display, lp_change, lp_color, lp_x.to_string())
    }

    async fn fetch_champion_image(&self, champion_name: &str) -> Option<String> {
        let url = format!(
            "https://ddragon.leagueoflegends.com/cdn/{}/img/champion/{}.png",
            self.ddragon_version, champion_name
        );
        self.cache.get_or_fetch(&self.http, &url).await
    }

    async fn fetch_profile_icon(&self, icon_id: i32) -> Option<String> {
        let url = format!(
            "https://ddragon.leagueoflegends.com/cdn/{}/img/profileicon/{}.png",
            self.ddragon_version, icon_id
        );
        self.cache.get_or_fetch(&self.http, &url).await
    }

    async fn fetch_item_image(&self, item_id: i32) -> Option<String> {
        let url = format!(
            "https://ddragon.leagueoflegends.com/cdn/{}/img/item/{}.png",
            self.ddragon_version, item_id
        );
        self.cache.get_or_fetch(&self.http, &url).await
    }

    fn render_svg_to_png(&self, svg_content: &str) -> Result<Vec<u8>, AppError> {
        let options = Options {
            fontdb: Arc::new(self.fontdb.clone()),
            ..Default::default()
        };

        let tree =
            Tree::from_str(svg_content, &options).map_err(|e| AppError::ImageGeneration {
                message: format!("Failed to parse SVG: {}", e),
            })?;

        let size = tree.size();
        let width = size.width() as u32;
        let height = size.height() as u32;

        let mut pixmap = Pixmap::new(width, height).ok_or_else(|| AppError::ImageGeneration {
            message: "Failed to create pixmap".to_string(),
        })?;

        resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

        let png_data = pixmap.encode_png().map_err(|e| AppError::ImageGeneration {
            message: format!("Failed to encode PNG: {}", e),
        })?;

        debug!(
            width,
            height,
            size = png_data.len(),
            "🖼️ ✅ Image generated"
        );
        Ok(png_data)
    }
}

fn format_damage(damage: i64) -> String {
    if damage >= 1_000_000 {
        format!("{:.1}M", damage as f64 / 1_000_000.0)
    } else if damage >= 1_000 {
        format!("{:.1}k", damage as f64 / 1_000.0)
    } else {
        damage.to_string()
    }
}

fn calculate_lp_diff(old_rank: Option<&RankInfo>, new_rank: Option<&RankInfo>) -> Option<i32> {
    let old = old_rank?;
    let new = new_rank?;
    Some(rank_to_lp(new) - rank_to_lp(old))
}

fn rank_to_lp(rank: &RankInfo) -> i32 {
    let tier_value = match rank.tier.to_uppercase().as_str() {
        "IRON" => 0,
        "BRONZE" => 400,
        "SILVER" => 800,
        "GOLD" => 1200,
        "PLATINUM" => 1600,
        "EMERALD" => 2000,
        "DIAMOND" => 2400,
        "MASTER" => 2800,
        "GRANDMASTER" => 3200,
        "CHALLENGER" => 3600,
        _ => 0,
    };

    let division_value = match rank.rank.as_str() {
        "IV" => 0,
        "III" => 100,
        "II" => 200,
        "I" => 300,
        _ => 0,
    };

    tier_value + division_value + rank.lp
}

fn capitalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
