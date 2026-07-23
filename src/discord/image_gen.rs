//! Match-card image generation.
//!
//! `generate_match_image` builds the card HTML ([`ScorecardVm`]), inlines the
//! Data Dragon assets it references as data URIs through the [`ImageCache`], then
//! renders that HTML to a PNG via the remote Browserless service
//! ([`ImageGenerator::render_html_to_png`]). All rasterisation happens in the
//! renderer, so the bot itself ships no font stack or SVG engine.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine;
use reqwest::Client;
use serde::Serialize;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use super::ddragon::DdragonData;
use super::scorecard::{CARD_WIDTH_PX, ScorecardVm};
use crate::config::USER_AGENT;
use crate::db::{Player, RankInfo};
use crate::error::AppError;
use crate::riot::{InfoDto, ParticipantDto};

/// Timeout for one render round-trip. Generous enough to absorb the renderer's
/// scale-to-zero cold start (~1–3 s) plus font/asset fetches inside Chromium.
const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

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
                    let data_uri = to_data_uri(&bytes);

                    let Some(key) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };

                    let entry = CacheEntry {
                        data_uri,
                        size_bytes: bytes.len(),
                        created_at: modified,
                    };

                    let mut cache = self.memory_cache.write().await;
                    cache.insert(key.to_string(), entry);
                    loaded_count += 1;
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

    /// Convert URL to a stable cache key (also used as filename).
    fn cache_key(&self, url: &str) -> String {
        // Create a hash-based key to avoid path issues
        let hash = url
            .bytes()
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        format!("{:016x}", hash)
    }

    /// Get cache file path for a cache key
    fn get_cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.png", key))
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
            .map(|(key, entry)| (key.clone(), entry.created_at))
            .collect();
        entries.sort_by_key(|e| e.1);

        let mut freed: u64 = 0;
        let target_free = current_size - (self.max_size_bytes * 80 / 100); // Free to 80% capacity

        for (key, _) in entries {
            if freed >= target_free {
                break;
            }

            if let Some(entry) = cache.remove(&key) {
                freed += entry.size_bytes as u64;

                // Also remove from disk
                let path = self.get_cache_path(&key);
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
        let key = self.cache_key(url);

        // Check memory cache first
        {
            let cache = self.memory_cache.read().await;
            if let Some(entry) = cache.get(&key)
                && !self.is_expired(entry)
            {
                trace!(url, "🖼️ Memory cache hit");
                return Some(entry.data_uri.clone());
            }
        }

        // Check disk cache
        let cache_path = self.get_cache_path(&key);
        if cache_path.exists()
            && let Ok(metadata) = fs::metadata(&cache_path).await
            && let Ok(modified) = metadata.modified()
        {
            if modified.elapsed().unwrap_or(Duration::MAX) <= self.ttl {
                // Valid disk cache
                if let Ok(bytes) = fs::read(&cache_path).await {
                    let data_uri = to_data_uri(&bytes);

                    // Store in memory
                    let entry = CacheEntry {
                        data_uri: data_uri.clone(),
                        size_bytes: bytes.len(),
                        created_at: modified,
                    };

                    let mut cache = self.memory_cache.write().await;
                    cache.insert(key.clone(), entry);

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
                        let data_uri = to_data_uri(&bytes);

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
                            cache.insert(key.clone(), entry);
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

/// Champion mastery for the played champion, threaded into the card for the
/// non-ranked footer block (level + points; the API exposes no games count).
#[derive(Debug, Clone, Copy)]
pub struct MasteryInfo {
    pub level: i32,
    pub points: i32,
}

pub struct MatchImageContext<'a> {
    pub player: &'a Player,
    pub participant: &'a ParticipantDto,
    pub match_info: &'a InfoDto,
    pub old_rank: Option<&'a RankInfo>,
    pub new_rank: Option<&'a RankInfo>,
    /// Recent results (last ~5) for the ranked streak bar, oldest→newest. Empty
    /// hides the bar (Phase 4).
    pub streak: Vec<bool>,
    /// Champion mastery for the non-ranked footer block. `None` omits it.
    pub mastery: Option<MasteryInfo>,
}

/// Location + credentials of the remote HTML→PNG renderer (Browserless).
struct RendererConfig {
    /// Base URL without a trailing slash, e.g.
    /// `http://tentrackule-renderer.internal:3000`.
    base_url: String,
    token: String,
}

pub struct ImageGenerator {
    http: Client,
    cache: ImageCache,
    /// Boot-time DDragon lookup tables (spell/rune/champion icon resolution).
    ddragon: DdragonData,
    /// `None` when `RENDERER_URL`/`RENDERER_TOKEN` are unset — rendering then
    /// errors and the poller degrades to a text embed.
    renderer: Option<RendererConfig>,
}

impl ImageGenerator {
    pub async fn new(
        ddragon_version: String,
        renderer_url: Option<String>,
        renderer_token: Option<String>,
    ) -> Result<Self, AppError> {
        let http = Client::builder().user_agent(USER_AGENT).build()?;

        // Fetch the DDragon spell/rune/champion lookup tables once (best-effort).
        let ddragon = DdragonData::load(&http, &ddragon_version).await;

        // Initialize cache (loads from disk)
        let cache = ImageCache::new().await;

        let renderer = match (renderer_url, renderer_token) {
            (Some(base_url), Some(token)) => Some(RendererConfig {
                base_url: base_url.trim_end_matches('/').to_string(),
                token,
            }),
            _ => {
                warn!(
                    "🖼️ ⚠️ Renderer not configured (RENDERER_URL/RENDERER_TOKEN); \
                     match cards will fall back to a text embed"
                );
                None
            }
        };

        Ok(Self {
            http,
            cache,
            ddragon,
            renderer,
        })
    }

    /// Resolve a champion's numeric id (`Ahri` → `103`) from the boot-time
    /// DDragon tables, for the poller's champion-mastery lookup. `None` when the
    /// champion index failed to load or the name is unknown.
    pub fn champion_id(&self, champion_name: &str) -> Option<i64> {
        self.ddragon.champion_id(champion_name)
    }

    /// Build the scorecard HTML and render it to a PNG via the remote renderer.
    pub async fn generate_match_image(
        &self,
        ctx: &MatchImageContext<'_>,
    ) -> Result<Vec<u8>, AppError> {
        let mut vm = ScorecardVm::from_context(ctx, &self.ddragon);
        self.inline_assets(&mut vm).await;
        let html = vm.build_html();
        self.render_html_to_png(&html).await
    }

    /// Fetch every image the card references through the cache and inline it as a
    /// data URI, so the render is deterministic and offline (no DDragon fetches
    /// at capture time). Fetches that fail are simply left as their original URL,
    /// which the renderer can still resolve directly.
    async fn inline_assets(&self, vm: &mut ScorecardVm) {
        let mut map: HashMap<String, String> = HashMap::new();
        for url in vm.image_urls() {
            if map.contains_key(&url) {
                continue;
            }
            if let Some(data_uri) = self.cache.get_or_fetch(&self.http, &url).await {
                map.insert(url, data_uri);
            }
        }
        vm.apply_asset_map(&map);
    }

    /// POST the resolved HTML to Browserless and return the PNG bytes. Crops to
    /// the `#card-root` element and waits for both network idle and web fonts
    /// before capturing, so the card never renders in a fallback font.
    async fn render_html_to_png(&self, html: &str) -> Result<Vec<u8>, AppError> {
        let renderer = self
            .renderer
            .as_ref()
            .ok_or_else(|| AppError::ImageGeneration {
                message: "renderer not configured".to_string(),
            })?;

        let url = format!("{}/screenshot?token={}", renderer.base_url, renderer.token);
        let payload = ScreenshotRequest {
            html,
            selector: "#card-root",
            options: ScreenshotOptions { kind: "png" },
            viewport: Viewport {
                width: CARD_WIDTH_PX,
                // Tall enough to lay the card out; the element crop trims the rest.
                height: 1200,
                device_scale_factor: 2,
            },
            goto_options: GotoOptions {
                wait_until: "networkidle0",
                timeout: 15_000,
            },
            wait_for_function: WaitForFunction {
                func: "async () => { await document.fonts.ready; return true; }",
                timeout: 5_000,
            },
        };

        let response = self
            .http
            .post(&url)
            .header(reqwest::header::ACCEPT, "image/png")
            .json(&payload)
            .timeout(RENDER_TIMEOUT)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            return Err(AppError::ImageGeneration {
                message: format!("renderer returned {status}: {snippet}"),
            });
        }

        let bytes = response.bytes().await?;
        debug!(size = bytes.len(), "🖼️ ✅ Card rendered via renderer");
        Ok(bytes.to_vec())
    }
}

// ============================================================================
// Browserless `/screenshot` request payload
// ============================================================================
//
// Hand-rolled `Serialize` structs (serialized via reqwest's `json` feature) so
// we don't take a direct `serde_json` dependency. Field names/shape mirror the
// Browserless v2 BodySchema (`html`, `selector`, `options`, `viewport`,
// `gotoOptions`, `waitForFunction`).

#[derive(Serialize)]
struct ScreenshotRequest<'a> {
    html: &'a str,
    /// Screenshot only this element (tight crop, no page padding).
    selector: &'a str,
    options: ScreenshotOptions,
    viewport: Viewport,
    #[serde(rename = "gotoOptions")]
    goto_options: GotoOptions,
    #[serde(rename = "waitForFunction")]
    wait_for_function: WaitForFunction,
}

#[derive(Serialize)]
struct ScreenshotOptions {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct Viewport {
    width: u32,
    height: u32,
    #[serde(rename = "deviceScaleFactor")]
    device_scale_factor: u32,
}

#[derive(Serialize)]
struct GotoOptions {
    #[serde(rename = "waitUntil")]
    wait_until: &'static str,
    timeout: u32,
}

#[derive(Serialize)]
struct WaitForFunction {
    /// Arrow-function source evaluated in the page (Browserless `fn`).
    #[serde(rename = "fn")]
    func: &'static str,
    timeout: u32,
}

/// Base64 `data:` URI for image bytes, with a MIME type sniffed from the magic
/// bytes. DDragon serves PNG (items/spells/runes/icons) and JPEG (champion
/// splash); CommunityDragon serves the role icons and rank crests as SVG. The
/// right type matters — Chromium won't decode an SVG (or the JPEG splash)
/// mislabelled as PNG.
fn to_data_uri(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if is_svg(bytes) {
        "image/svg+xml"
    } else {
        "image/png"
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:{mime};base64,{b64}")
}

/// Whether `bytes` looks like an SVG document — the first non-whitespace byte is
/// `<` (a `<?xml …?>` prolog or a bare `<svg>`). Good enough given the only
/// non-raster assets we inline are CommunityDragon SVGs.
fn is_svg(bytes: &[u8]) -> bool {
    matches!(
        bytes.iter().copied().find(|b| !b.is_ascii_whitespace()),
        Some(b'<')
    )
}

#[cfg(test)]
mod tests {
    use super::to_data_uri;

    #[test]
    fn data_uri_sniffs_mime_from_magic_bytes() {
        // JPEG magic (FF D8 FF) -> image/jpeg; PNG magic -> image/png.
        assert!(to_data_uri(&[0xFF, 0xD8, 0xFF, 0x00]).starts_with("data:image/jpeg;base64,"));
        assert!(to_data_uri(&[0x89, b'P', b'N', b'G']).starts_with("data:image/png;base64,"));
        // SVG (leading `<svg`, or an `<?xml` prolog after whitespace) -> svg+xml.
        assert!(to_data_uri(b"<svg xmlns=\"x\"></svg>").starts_with("data:image/svg+xml;base64,"));
        assert!(
            to_data_uri(b"\n  <?xml version=\"1.0\"?><svg/>")
                .starts_with("data:image/svg+xml;base64,")
        );
    }
}
