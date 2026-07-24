use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use arc_swap::ArcSwap;
use base64::Engine;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, info, trace, warn};

use super::ddragon::{self, DDRAGON_CDN, DdragonData};
use super::scorecard::{CARD_WIDTH_PX, ScorecardVm};
use crate::config::USER_AGENT;
use crate::db::{Player, RankInfo};
use crate::error::AppError;
use crate::metrics::Metrics;
use crate::riot::{InfoDto, ParticipantDto};

const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

const RENDER_ATTEMPTS: u32 = 2;
const RENDER_RETRY_DELAY: Duration = Duration::from_secs(25);

const SCREENSHOT_TYPE: &str = "jpeg";
const SCREENSHOT_QUALITY: Option<u32> = Some(90);

const LATIN_FONT_CSS_URL: &str = "https://fonts.googleapis.com/css2?family=Cinzel:wght@600;700&family=Barlow+Semi+Condensed:wght@400;500;600;700&display=swap";
const FONT_FETCH_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const INLINED_SUBSETS: &[&str] = &["latin", "latin-ext"];

const MUTABLE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const DISK_MAX_SIZE_MB: u64 = 100;
const MEMORY_MAX_SIZE_MB: u64 = 24;
const CACHE_DIR: &str = ".cache/images";
const FETCH_CONCURRENCY: usize = 4;

struct MemoryEntry {
    bytes: Arc<[u8]>,
    created_at: SystemTime,
    last_access: AtomicU64,
}

pub struct ImageCache {
    memory_cache: RwLock<HashMap<String, MemoryEntry>>,
    cache_dir: PathBuf,
    mutable_ttl: Duration,
    disk_max_bytes: u64,
    memory_max_bytes: u64,
    clock: AtomicU64,
}

impl ImageCache {
    pub async fn new() -> Self {
        let cache_dir = PathBuf::from(CACHE_DIR);

        if let Err(e) = fs::create_dir_all(&cache_dir).await {
            warn!(error = ?e, "🖼️ ⚠️ Failed to create cache directory");
        }

        Self {
            memory_cache: RwLock::new(HashMap::new()),
            cache_dir,
            mutable_ttl: MUTABLE_TTL,
            disk_max_bytes: DISK_MAX_SIZE_MB * 1024 * 1024,
            memory_max_bytes: MEMORY_MAX_SIZE_MB * 1024 * 1024,
            clock: AtomicU64::new(0),
        }
    }

    fn ttl_for(&self, url: &str) -> Option<Duration> {
        if url.starts_with(DDRAGON_CDN) {
            None
        } else {
            Some(self.mutable_ttl)
        }
    }

    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed)
    }

    fn cache_key(url: &str) -> String {
        let digest = Sha256::digest(url.as_bytes());
        let prefix: [u8; 16] = digest[..16].try_into().expect("sha-256 is 32 bytes");
        format!("{:032x}", u128::from_be_bytes(prefix))
    }

    fn get_cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.png", key))
    }

    async fn store_in_memory(&self, key: String, bytes: Arc<[u8]>, created_at: SystemTime) {
        {
            let mut cache = self.memory_cache.write().await;
            cache.insert(
                key,
                MemoryEntry {
                    bytes,
                    created_at,
                    last_access: AtomicU64::new(self.tick()),
                },
            );
        }
        self.evict_memory_if_needed().await;
    }

    async fn evict_memory_if_needed(&self) {
        let mut cache = self.memory_cache.write().await;

        let mut total: u64 = cache.values().map(|e| e.bytes.len() as u64).sum();
        if total <= self.memory_max_bytes {
            return;
        }

        let mut entries: Vec<_> = cache
            .iter()
            .map(|(key, entry)| {
                (
                    key.clone(),
                    entry.last_access.load(Ordering::Relaxed),
                    entry.bytes.len() as u64,
                )
            })
            .collect();
        entries.sort_by_key(|e| e.1);

        let target = self.memory_max_bytes * 80 / 100;
        for (key, _, size) in entries {
            if total <= target {
                break;
            }
            if cache.remove(&key).is_some() {
                total -= size;
            }
        }
    }

    async fn evict_disk_if_needed(&self) {
        let mut read_dir = match fs::read_dir(&self.cache_dir).await {
            Ok(rd) => rd,
            Err(_) => return,
        };

        let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
        let mut total: u64 = 0;

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let path = entry.path();
            if path.extension().map(|e| e != "png").unwrap_or(true) {
                continue;
            }
            if let Ok(metadata) = fs::metadata(&path).await {
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                total += metadata.len();
                files.push((path, modified, metadata.len()));
            }
        }

        if total <= self.disk_max_bytes {
            return;
        }

        files.sort_by_key(|f| f.1);

        let target = self.disk_max_bytes * 80 / 100;
        let mut freed: u64 = 0;
        for (path, _, size) in files {
            if total <= target {
                break;
            }
            if fs::remove_file(&path).await.is_ok() {
                total -= size;
                freed += size;
            }
        }

        if freed > 0 {
            debug!(
                freed_mb = freed / 1024 / 1024,
                "🖼️ Disk cache eviction completed"
            );
        }
    }

    async fn get_or_fetch(&self, http: &Client, url: &str, metrics: &Metrics) -> Option<String> {
        let key = Self::cache_key(url);
        let ttl = self.ttl_for(url);

        {
            let cache = self.memory_cache.read().await;
            if let Some(entry) = cache.get(&key)
                && is_fresh(entry.created_at, ttl)
            {
                entry.last_access.store(self.tick(), Ordering::Relaxed);
                metrics.record_cache_hit();
                trace!(url, "🖼️ Memory cache hit");
                return Some(to_data_uri(&entry.bytes));
            }
        }

        let cache_path = self.get_cache_path(&key);
        if let Ok(metadata) = fs::metadata(&cache_path).await
            && let Ok(modified) = metadata.modified()
        {
            if is_fresh(modified, ttl) {
                if let Ok(bytes) = fs::read(&cache_path).await {
                    let data_uri = to_data_uri(&bytes);
                    self.store_in_memory(key, Arc::from(bytes), modified).await;
                    metrics.record_cache_hit();
                    trace!(url, "🖼️ Disk cache hit");
                    return Some(data_uri);
                }
            } else {
                let _ = fs::remove_file(&cache_path).await;
            }
        }

        metrics.record_cache_miss();
        trace!(url, "🖼️ Fetching image");
        let bytes = match http.get(url).send().await {
            Ok(response) if response.status().is_success() => match response.bytes().await {
                Ok(bytes) => bytes,
                Err(e) => {
                    warn!(url, error = ?e, "🖼️ ⚠️ Failed to read image bytes");
                    return None;
                }
            },
            Ok(response) => {
                warn!(
                    url,
                    status = response.status().as_u16(),
                    "🖼️ ⚠️ Image fetch failed"
                );
                return None;
            }
            Err(e) => {
                warn!(url, error = ?e, "🖼️ ⚠️ Image request failed");
                return None;
            }
        };

        let data_uri = to_data_uri(&bytes);

        if let Err(e) = fs::write(&cache_path, &bytes).await {
            warn!(error = ?e, "🖼️ ⚠️ Failed to write cache file");
        }
        self.evict_disk_if_needed().await;

        self.store_in_memory(key, Arc::from(bytes.as_ref()), SystemTime::now())
            .await;

        debug!(url, "🖼️ ✅ Image cached");
        Some(data_uri)
    }
}

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
    pub streak: Vec<bool>,
    pub mastery: Option<MasteryInfo>,
}

struct RendererConfig {
    base_url: String,
    token: String,
}

const DDRAGON_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct ImageGenerator {
    http: Client,
    cache: ImageCache,
    ddragon: ArcSwap<DdragonData>,
    ddragon_version_override: Option<String>,
    renderer: Option<RendererConfig>,
    latin_font_css: RwLock<Option<Arc<str>>>,
    metrics: Arc<Metrics>,
}

impl ImageGenerator {
    pub async fn new(
        ddragon_version_override: Option<String>,
        renderer_url: Option<String>,
        renderer_token: Option<String>,
        metrics: Arc<Metrics>,
    ) -> Result<Self, AppError> {
        let http = Client::builder().user_agent(USER_AGENT).build()?;

        let version = ddragon::resolve_version(&http, ddragon_version_override.as_deref()).await;
        let ddragon = DdragonData::load(&http, &version).await;

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
            ddragon: ArcSwap::from_pointee(ddragon),
            ddragon_version_override,
            renderer,
            latin_font_css: RwLock::new(None),
            metrics,
        })
    }

    pub fn ddragon_version(&self) -> String {
        self.ddragon.load().version().to_string()
    }

    pub async fn run_ddragon_refresh(self: Arc<Self>) {
        let mut ticker = interval(DDRAGON_REFRESH_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            self.refresh_ddragon().await;
        }
    }

    async fn refresh_ddragon(&self) {
        if self.ddragon_version_override.is_some() {
            return;
        }
        let Some(latest) = ddragon::fetch_latest_version(&self.http).await else {
            return;
        };
        let current = self.ddragon_version();
        if latest == current {
            debug!(version = %current, "🖼️ Data Dragon patch unchanged; tables kept");
            return;
        }
        let fresh = DdragonData::load(&self.http, &latest).await;
        self.ddragon.store(Arc::new(fresh));
        info!(from = %current, to = %latest, "🖼️ Data Dragon tables hot-swapped to new patch");
    }

    pub const ATTACHMENT_FILENAME: &'static str = "match_result.jpg";

    pub fn champion_id(&self, champion_name: &str) -> Option<i64> {
        self.ddragon.load().champion_id(champion_name)
    }

    pub async fn generate_match_image(
        &self,
        ctx: &MatchImageContext<'_>,
    ) -> Result<Vec<u8>, AppError> {
        let ddragon = self.ddragon.load_full();
        let mut vm = ScorecardVm::from_context(ctx, &ddragon);
        self.inline_assets(&mut vm).await;

        let fonts_inlined = if vm.needs_cjk_fonts() {
            false
        } else if let Some(css) = self.latin_font_css().await {
            vm.set_inlined_font_css(css);
            true
        } else {
            false
        };

        let html = vm.build_html();
        self.render_with_retry(&html, fonts_inlined).await
    }

    async fn render_with_retry(
        &self,
        html: &str,
        fonts_inlined: bool,
    ) -> Result<Vec<u8>, AppError> {
        if self.renderer.is_none() {
            return self.render_html_to_png(html, fonts_inlined).await;
        }

        let mut attempt: u32 = 1;
        loop {
            match self.render_html_to_png(html, fonts_inlined).await {
                Ok(bytes) => {
                    if attempt > 1 {
                        info!(attempt, "🖼️ ✅ Card rendered on retry");
                    }
                    return Ok(bytes);
                }
                Err(e) if attempt < RENDER_ATTEMPTS => {
                    warn!(
                        attempt,
                        error = ?e,
                        delay_s = RENDER_RETRY_DELAY.as_secs(),
                        "🖼️ ⚠️ Render attempt failed; retrying after delay"
                    );
                    tokio::time::sleep(RENDER_RETRY_DELAY).await;
                    attempt += 1;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn inline_assets(&self, vm: &mut ScorecardVm) {
        let mut urls = vm.image_urls();
        urls.sort();
        urls.dedup();

        let map: HashMap<String, String> = stream::iter(urls)
            .map(|url| async move {
                self.cache
                    .get_or_fetch(&self.http, &url, &self.metrics)
                    .await
                    .map(|data_uri| (url, data_uri))
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        vm.apply_asset_map(&map);
    }

    async fn render_html_to_png(
        &self,
        html: &str,
        fonts_inlined: bool,
    ) -> Result<Vec<u8>, AppError> {
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
            options: ScreenshotOptions {
                kind: SCREENSHOT_TYPE,
                quality: SCREENSHOT_QUALITY,
            },
            viewport: Viewport {
                width: CARD_WIDTH_PX,
                height: 1200,
                device_scale_factor: 2,
            },
            goto_options: GotoOptions {
                wait_until: if fonts_inlined {
                    "load"
                } else {
                    "networkidle0"
                },
                timeout: 15_000,
            },
            wait_for_function: WaitForFunction {
                func: "async () => { await document.fonts.ready; return true; }",
                timeout: 5_000,
            },
        };

        let started = std::time::Instant::now();
        let response = self
            .http
            .post(&url)
            .header(reqwest::header::ACCEPT, format!("image/{SCREENSHOT_TYPE}"))
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
        self.metrics.record_render(started.elapsed());
        debug!(size = bytes.len(), "🖼️ ✅ Card rendered via renderer");
        Ok(bytes.to_vec())
    }

    async fn latin_font_css(&self) -> Option<Arc<str>> {
        if let Some(css) = self.latin_font_css.read().await.clone() {
            return Some(css);
        }

        match self.build_latin_font_css().await {
            Ok(css) => {
                let arc: Arc<str> = Arc::from(css);
                *self.latin_font_css.write().await = Some(arc.clone());
                debug!("🖼️ ✅ Inlined Latin fonts ready");
                Some(arc)
            }
            Err(e) => {
                warn!(error = ?e, "🖼️ ⚠️ Font inlining failed; using Google Fonts <link>");
                None
            }
        }
    }

    async fn build_latin_font_css(&self) -> Result<String, AppError> {
        let css = self
            .http
            .get(LATIN_FONT_CSS_URL)
            .header(reqwest::header::USER_AGENT, FONT_FETCH_UA)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let inlined = inline_font_faces(&self.http, &css).await?;

        let woff2_count = inlined.matches("data:font/woff2").count();
        if !(inlined.contains("Cinzel") && inlined.contains("Barlow") && woff2_count >= 4) {
            return Err(AppError::ImageGeneration {
                message: format!("font inlining produced too few faces ({woff2_count} woff2)"),
            });
        }
        Ok(inlined)
    }
}

async fn inline_font_faces(http: &Client, css: &str) -> Result<String, AppError> {
    let mut out = String::new();
    for (subset, block) in font_face_segments(css) {
        if !INLINED_SUBSETS.contains(&subset) {
            continue;
        }
        out.push_str(&inline_block_urls(http, block).await?);
        out.push('\n');
    }
    Ok(out)
}

fn font_face_segments(css: &str) -> Vec<(&str, &str)> {
    let mut segments = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find("/*") {
        let after_open = &rest[open + 2..];
        let Some(close) = after_open.find("*/") else {
            break;
        };
        let subset = after_open[..close].trim();
        let after_comment = &after_open[close + 2..];
        let block_end = after_comment.find("/*").unwrap_or(after_comment.len());
        segments.push((subset, after_comment[..block_end].trim()));
        rest = &after_comment[block_end..];
    }
    segments
}

async fn inline_block_urls(http: &Client, block: &str) -> Result<String, AppError> {
    let mut inlined = block.to_string();
    for url in extract_css_urls(block) {
        let bytes = fetch_font(http, &url).await?;
        let data_uri = to_font_data_uri(&bytes);
        inlined = inlined.replace(&format!("url({url})"), &format!("url({data_uri})"));
    }
    Ok(inlined)
}

fn extract_css_urls(css: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find("url(") {
        let after = &rest[open + 4..];
        let Some(close) = after.find(')') else {
            break;
        };
        let url = after[..close].trim().trim_matches(['"', '\'']);
        if url.starts_with("http") {
            urls.push(url.to_string());
        }
        rest = &after[close + 1..];
    }
    urls
}

async fn fetch_font(http: &Client, url: &str) -> Result<Vec<u8>, AppError> {
    let bytes = http
        .get(url)
        .header(reqwest::header::USER_AGENT, FONT_FETCH_UA)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(bytes.to_vec())
}

fn to_font_data_uri(bytes: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:font/woff2;base64,{b64}")
}

#[derive(Serialize)]
struct ScreenshotRequest<'a> {
    html: &'a str,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    quality: Option<u32>,
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
    #[serde(rename = "fn")]
    func: &'static str,
    timeout: u32,
}

fn is_fresh(created_at: SystemTime, ttl: Option<Duration>) -> bool {
    match ttl {
        None => true,
        Some(ttl) => created_at.elapsed().unwrap_or(Duration::MAX) <= ttl,
    }
}

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

fn is_svg(bytes: &[u8]) -> bool {
    matches!(
        bytes.iter().copied().find(|b| !b.is_ascii_whitespace()),
        Some(b'<')
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime};

    use tokio::sync::RwLock;

    use super::{ImageCache, to_data_uri};

    fn temp_cache(suffix: &str, disk_mb: u64, mem_mb: u64) -> ImageCache {
        let dir =
            std::env::temp_dir().join(format!("tentrackule-{}-{}", std::process::id(), suffix));
        std::fs::create_dir_all(&dir).unwrap();
        ImageCache {
            memory_cache: RwLock::new(HashMap::new()),
            cache_dir: dir,
            mutable_ttl: Duration::from_secs(3600),
            disk_max_bytes: disk_mb * 1024 * 1024,
            memory_max_bytes: mem_mb * 1024 * 1024,
            clock: AtomicU64::new(0),
        }
    }

    #[test]
    fn ttl_for_classifies_ddragon_as_immutable() {
        let cache = temp_cache("ttl", 100, 24);
        assert_eq!(
            cache.ttl_for("https://ddragon.leagueoflegends.com/cdn/16.1.1/img/item/1001.png"),
            None
        );
        assert_eq!(
            cache
                .ttl_for("https://ddragon.leagueoflegends.com/cdn/img/champion/loading/Ahri_0.jpg"),
            None
        );
        assert_eq!(
            cache.ttl_for("https://raw.communitydragon.org/latest/.../position-top.svg"),
            Some(cache.mutable_ttl)
        );
        let _ = std::fs::remove_dir_all(&cache.cache_dir);
    }

    #[test]
    fn cache_key_is_stable_wide_and_distinct() {
        let a = ImageCache::cache_key("https://ddragon/img/champion/Ahri.png");
        let b = ImageCache::cache_key("https://ddragon/img/champion/Ahri.png");
        let c = ImageCache::cache_key("https://ddragon/img/champion/Ashe.png");
        assert_eq!(a, b, "same URL must map to the same key");
        assert_ne!(a, c, "distinct URLs must not collide");
        assert_eq!(a.len(), 32, "128-bit key rendered as 32 hex chars");
    }

    #[tokio::test]
    async fn disk_hit_promotes_to_memory_without_network() {
        let cache = temp_cache("diskhit", 100, 24);
        let url = "https://ddragon.example/img/item/1001.png";
        let key = ImageCache::cache_key(url);

        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
        tokio::fs::write(cache.get_cache_path(&key), &png)
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let metrics = crate::metrics::Metrics::new();
        let uri = cache
            .get_or_fetch(&client, url, &metrics)
            .await
            .expect("disk hit");
        assert!(uri.starts_with("data:image/png;base64,"));
        assert!(
            cache.memory_cache.read().await.contains_key(&key),
            "disk hit must promote the entry into the memory layer"
        );

        let _ = std::fs::remove_dir_all(&cache.cache_dir);
    }

    #[tokio::test]
    async fn memory_layer_evicts_least_recently_used() {
        let cache = temp_cache("lru", 100, 1);
        let blob = vec![0u8; 400_000];
        let store = |k: &'static str| {
            let bytes: Arc<[u8]> = Arc::from(blob.as_slice());
            cache.store_in_memory(k.to_string(), bytes, SystemTime::now())
        };

        store("a").await;
        store("b").await;
        {
            let mem = cache.memory_cache.read().await;
            mem["a"].last_access.store(cache.tick(), Ordering::Relaxed);
        }
        store("c").await;

        let mem = cache.memory_cache.read().await;
        let total: u64 = mem.values().map(|e| e.bytes.len() as u64).sum();
        assert!(
            total <= cache.memory_max_bytes,
            "memory layer stayed over cap"
        );
        assert!(mem.contains_key("a"), "recently-touched entry must survive");
        assert!(mem.contains_key("c"), "freshly-inserted entry must survive");
        assert!(!mem.contains_key("b"), "the LRU entry must be evicted");

        drop(mem);
        let _ = std::fs::remove_dir_all(&cache.cache_dir);
    }

    const SAMPLE_FONT_CSS: &str = "\
/* cyrillic */\n\
@font-face {font-family:'Barlow Semi Condensed';font-weight:400;src:url(https://fonts.gstatic.com/s/barlow/cyr.woff2) format('woff2');unicode-range:U+0400-045F;}\n\
/* latin */\n\
@font-face {font-family:'Barlow Semi Condensed';font-weight:400;src:url(https://fonts.gstatic.com/s/barlow/lat.woff2) format('woff2');unicode-range:U+0000-00FF;}\n\
/* [0] */\n\
@font-face {font-family:'Noto Sans JP';font-weight:500;src:url(https://fonts.gstatic.com/s/noto/0.woff2) format('woff2');unicode-range:U+25CB;}\n";

    #[test]
    fn font_face_segments_pairs_subset_with_block() {
        let segs = super::font_face_segments(SAMPLE_FONT_CSS);
        let subsets: Vec<&str> = segs.iter().map(|(s, _)| *s).collect();
        assert_eq!(subsets, ["cyrillic", "latin", "[0]"]);
        assert!(segs[1].1.contains("barlow/lat.woff2"));
        assert!(segs[2].1.contains("Noto Sans JP"));
    }

    #[test]
    fn extract_css_urls_reads_unquoted_gstatic_urls() {
        let urls = super::extract_css_urls(SAMPLE_FONT_CSS);
        assert_eq!(urls.len(), 3);
        assert!(urls[0].ends_with("cyr.woff2"));
        assert!(
            urls.iter()
                .all(|u| u.starts_with("https://fonts.gstatic.com"))
        );
        assert!(super::extract_css_urls("body{color:red}").is_empty());
        assert!(super::extract_css_urls("src:url(data:font/woff2;base64,AA)").is_empty());
    }

    #[test]
    fn to_font_data_uri_labels_woff2() {
        assert!(super::to_font_data_uri(b"wOF2\x00\x01").starts_with("data:font/woff2;base64,"));
    }

    #[tokio::test]
    #[ignore = "hits the network (Google Fonts)"]
    async fn inline_real_google_fonts() {
        let http = reqwest::Client::new();
        let css = http
            .get(super::LATIN_FONT_CSS_URL)
            .header(reqwest::header::USER_AGENT, super::FONT_FETCH_UA)
            .send()
            .await
            .expect("fetch css")
            .text()
            .await
            .expect("css body");

        let inlined = super::inline_font_faces(&http, &css)
            .await
            .expect("inline faces");

        assert!(inlined.contains("Cinzel"), "Cinzel faces missing");
        assert!(inlined.contains("Barlow"), "Barlow faces missing");
        assert!(
            inlined.matches("data:font/woff2").count() >= 4,
            "expected several inlined woff2 faces"
        );
        assert!(
            !inlined.contains("https://fonts.gstatic.com"),
            "a gstatic url was left un-inlined"
        );
    }

    #[test]
    fn data_uri_sniffs_mime_from_magic_bytes() {
        assert!(to_data_uri(&[0xFF, 0xD8, 0xFF, 0x00]).starts_with("data:image/jpeg;base64,"));
        assert!(to_data_uri(&[0x89, b'P', b'N', b'G']).starts_with("data:image/png;base64,"));
        assert!(to_data_uri(b"<svg xmlns=\"x\"></svg>").starts_with("data:image/svg+xml;base64,"));
        assert!(
            to_data_uri(b"\n  <?xml version=\"1.0\"?><svg/>")
                .starts_with("data:image/svg+xml;base64,")
        );
    }
}
