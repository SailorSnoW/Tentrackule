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
use crate::riot::{InfoDto, ParticipantDto};

/// Timeout for one render round-trip. Generous enough to absorb the renderer's
/// scale-to-zero cold start (~1–3 s) plus font/asset fetches inside Chromium.
const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

/// How many times to attempt a render before giving up to the text fallback.
/// The first attempt on a scaled-to-zero renderer may hit a cold Chrome that
/// overruns Browserless's launch timeout; the second, after a short pause, lands
/// on a warm Chrome (1–6 s). A just-finished match tolerates the added latency.
const RENDER_ATTEMPTS: u32 = 2;
/// Pause between render attempts — long enough for a cold-woken machine to finish
/// launching Chrome before the retry.
const RENDER_RETRY_DELAY: Duration = Duration::from_secs(25);

/// Screenshot output format. JPEG (vs PNG) is 3–6× smaller for the splash-
/// dominated card and uploads to Discord faster; q90 keeps the fine scorecard
/// text crisp. To revert, set this to `"png"` and `SCREENSHOT_QUALITY` to `None`.
const SCREENSHOT_TYPE: &str = "jpeg";
/// JPEG/WebP quality (0–100); `None` for PNG (which ignores it).
const SCREENSHOT_QUALITY: Option<u32> = Some(90);

/// The always-present Latin card families (name-independent), requested as one
/// Google Fonts stylesheet whose `latin`/`latin-ext` faces we inline. The CJK
/// families (Noto Sans JP/KR/SC) are deliberately absent: names needing them
/// keep the `<link>` path (see [`ScorecardVm::needs_cjk_fonts`]).
const LATIN_FONT_CSS_URL: &str = "https://fonts.googleapis.com/css2?family=Cinzel:wght@600;700&family=Barlow+Semi+Condensed:wght@400;500;600;700&display=swap";
/// A desktop-Chrome UA so Google Fonts serves woff2 (its smallest format) in the
/// `@font-face` `src`, instead of falling back to ttf for an unknown agent.
const FONT_FETCH_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
/// Google Fonts `/* subset */` comment labels we inline for the Latin path. The
/// card's non-name text is all basic Latin; names may carry accents (latin-ext).
const INLINED_SUBSETS: &[&str] = &["latin", "latin-ext"];

// Cache configuration
/// TTL for *mutable* assets only — CommunityDragon `latest` (role/rank/objective
/// icons), a rolling pointer that can change between patches. DDragon CDN assets
/// are immutable and bypass the TTL entirely (see [`ImageCache::ttl_for`]), so
/// the versioned set — and the big champion splash — is never re-downloaded on a
/// schedule, only evicted under disk pressure (Phase 5.3).
const MUTABLE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24h
/// Disk is the durable cache layer — sized for the full asset set.
const DISK_MAX_SIZE_MB: u64 = 100;
/// The in-RAM layer is only a bounded accelerator in front of the disk. Kept
/// small so the bot's RSS stays well under the 256 MB VM even with a full disk
/// cache — the memory layer, not the disk, was the OOM path.
const MEMORY_MAX_SIZE_MB: u64 = 24;
const CACHE_DIR: &str = ".cache/images";
/// Bounded concurrency when fetching a cold card's assets (~15 URLs).
const FETCH_CONCURRENCY: usize = 4;

/// One image held in the in-RAM accelerator. Stores the *raw* bytes (not the
/// base64 data URI): the ×1.33 base64 expansion is paid on demand at render
/// time, keeping the memory layer ~25% leaner.
struct MemoryEntry {
    bytes: Arc<[u8]>,
    created_at: SystemTime,
    /// Monotonic tick of the last access, for LRU eviction. Interior-mutable so
    /// a cache hit can refresh it while holding only a read lock.
    last_access: AtomicU64,
}

/// Two-layer cache for card images: a durable disk cache (the real cache, capped
/// at [`DISK_MAX_SIZE_MB`]) fronted by a small bounded in-RAM LRU
/// ([`MEMORY_MAX_SIZE_MB`]). Freshness is per-URL ([`ImageCache::ttl_for`]):
/// immutable DDragon assets never expire by time (only the disk size cap evicts
/// them), while mutable CommunityDragon `latest` assets carry [`MUTABLE_TTL`].
/// The memory layer fills lazily on first hit for a key and is never preloaded
/// at boot.
pub struct ImageCache {
    memory_cache: RwLock<HashMap<String, MemoryEntry>>,
    cache_dir: PathBuf,
    /// TTL applied to mutable assets; immutable assets ignore it entirely.
    mutable_ttl: Duration,
    disk_max_bytes: u64,
    memory_max_bytes: u64,
    /// Source of monotonic ticks for LRU ordering.
    clock: AtomicU64,
}

impl ImageCache {
    pub async fn new() -> Self {
        let cache_dir = PathBuf::from(CACHE_DIR);

        // Create cache directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(&cache_dir).await {
            warn!(error = ?e, "🖼️ ⚠️ Failed to create cache directory");
        }

        // Boot does no disk purge: immutable assets must survive indefinitely,
        // and the few mutable ones are tiny — a stale one is caught lazily at
        // read time (re-fetched past its TTL) and the disk stays bounded by the
        // size cap, so a boot-time sweep would reclaim ~nothing.
        Self {
            memory_cache: RwLock::new(HashMap::new()),
            cache_dir,
            mutable_ttl: MUTABLE_TTL,
            disk_max_bytes: DISK_MAX_SIZE_MB * 1024 * 1024,
            memory_max_bytes: MEMORY_MAX_SIZE_MB * 1024 * 1024,
            clock: AtomicU64::new(0),
        }
    }

    /// Freshness policy for a URL (Phase 5.3). DDragon CDN assets — the versioned
    /// paths (`/cdn/<ver>/img/…`, pinned forever) *and* the versionless splash /
    /// rune art (`/cdn/img/…`, stable per champion/rune) — are immutable: `None`,
    /// so they never expire by time and are only ever dropped by the disk size
    /// cap. Everything else (CommunityDragon `latest`) keeps [`MUTABLE_TTL`] so a
    /// genuine upstream change is eventually re-fetched.
    fn ttl_for(&self, url: &str) -> Option<Duration> {
        if url.starts_with(DDRAGON_CDN) {
            None
        } else {
            Some(self.mutable_ttl)
        }
    }

    /// Next monotonic tick, for LRU recency ordering.
    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed)
    }

    /// Stable 128-bit cache key (also used as the on-disk filename stem). A wide
    /// hash so two distinct URLs can't collide and silently serve each other's
    /// image, as the old 64-bit multiply-by-31 hash could.
    fn cache_key(url: &str) -> String {
        let digest = Sha256::digest(url.as_bytes());
        let prefix: [u8; 16] = digest[..16].try_into().expect("sha-256 is 32 bytes");
        format!("{:032x}", u128::from_be_bytes(prefix))
    }

    /// Get cache file path for a cache key
    fn get_cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.png", key))
    }

    /// Insert raw bytes into the in-RAM layer, then evict LRU entries if the
    /// layer is over its (small) cap.
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

    /// Evict least-recently-used memory entries until the in-RAM layer is back
    /// under 80% of its cap. Touches memory only — the disk cache is untouched,
    /// so an evicted asset is still served from disk (no re-download).
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
        entries.sort_by_key(|e| e.1); // least-recently-used first

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

    /// Enforce the disk cap by deleting the oldest files (by mtime) until disk
    /// usage is back under 80%. Scans the cache dir, so it reflects real disk
    /// state regardless of what's currently in memory.
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

        files.sort_by_key(|f| f.1); // oldest first

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
            debug!(freed_mb = freed / 1024 / 1024, "🖼️ Disk cache eviction completed");
        }
    }

    async fn get_or_fetch(&self, http: &Client, url: &str) -> Option<String> {
        let key = Self::cache_key(url);
        // Freshness is per-URL: immutable DDragon assets never expire, mutable
        // CommunityDragon assets carry the TTL. The key is `hash(url)`, so the
        // policy derived from `url` is the right one for this entry in both layers.
        let ttl = self.ttl_for(url);

        // 1. In-RAM accelerator.
        {
            let cache = self.memory_cache.read().await;
            if let Some(entry) = cache.get(&key)
                && is_fresh(entry.created_at, ttl)
            {
                entry.last_access.store(self.tick(), Ordering::Relaxed);
                trace!(url, "🖼️ Memory cache hit");
                return Some(to_data_uri(&entry.bytes));
            }
        }

        // 2. Durable disk cache.
        let cache_path = self.get_cache_path(&key);
        if let Ok(metadata) = fs::metadata(&cache_path).await
            && let Ok(modified) = metadata.modified()
        {
            if is_fresh(modified, ttl) {
                if let Ok(bytes) = fs::read(&cache_path).await {
                    let data_uri = to_data_uri(&bytes);
                    self.store_in_memory(key, Arc::from(bytes), modified).await;
                    trace!(url, "🖼️ Disk cache hit");
                    return Some(data_uri);
                }
            } else {
                // Expired (only a mutable asset can reach here), remove.
                let _ = fs::remove_file(&cache_path).await;
            }
        }

        // 3. Network.
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

        // Save to the durable layer and enforce its cap.
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

/// Interval between background Data Dragon table refreshes (Phase 5.2). A patch
/// ships roughly every two weeks, so a daily check picks up a new champion well
/// within a run while costing one tiny `versions.json` request a day.
const DDRAGON_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub struct ImageGenerator {
    http: Client,
    cache: ImageCache,
    /// DDragon lookup tables (spell/rune/champion icon resolution), hot-swapped
    /// by the periodic refresh so a champion released mid-run stops 404-ing.
    /// `ArcSwap` gives readers a lock-free snapshot while a refresh swaps in the
    /// next patch's tables underneath them.
    ddragon: ArcSwap<DdragonData>,
    /// Pinned `DDRAGON_VERSION` override, if set. When present the refresh is a
    /// no-op (the operator has taken manual control of the patch); when absent
    /// the live patch is re-resolved each cycle.
    ddragon_version_override: Option<String>,
    /// `None` when `RENDERER_URL`/`RENDERER_TOKEN` are unset — rendering then
    /// errors and the poller degrades to a text embed.
    renderer: Option<RendererConfig>,
    /// The invariant Latin font faces (Cinzel + Barlow, latin/latin-ext) inlined
    /// as data URIs — identical for every card, so built once on first use and
    /// reused. `None` until the one-time fetch succeeds; a failed attempt is not
    /// cached, so a later render retries it (the card meanwhile uses `<link>`).
    latin_font_css: RwLock<Option<Arc<str>>>,
}

impl ImageGenerator {
    pub async fn new(
        ddragon_version_override: Option<String>,
        renderer_url: Option<String>,
        renderer_token: Option<String>,
    ) -> Result<Self, AppError> {
        let http = Client::builder().user_agent(USER_AGENT).build()?;

        // Resolve the live patch (or honour a pinned override), then fetch the
        // DDragon spell/rune/champion lookup tables for it (best-effort).
        let version = ddragon::resolve_version(&http, ddragon_version_override.as_deref()).await;
        let ddragon = DdragonData::load(&http, &version).await;

        // Initialize cache (lazy; no boot purge).
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
        })
    }

    /// The Data Dragon patch the lookup tables are currently built for.
    pub fn ddragon_version(&self) -> String {
        self.ddragon.load().version().to_string()
    }

    /// Periodically re-resolve the live patch and hot-swap the lookup tables so a
    /// champion released mid-run stops 404-ing on its assets/mastery, with no
    /// redeploy (Phase 5.2). Runs until the process exits; spawn it on the shared
    /// `Arc<ImageGenerator>`.
    pub async fn run_ddragon_refresh(self: Arc<Self>) {
        let mut ticker = interval(DDRAGON_REFRESH_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick fires immediately; skip it since the tables are already
        // fresh from boot, so the first real refresh is one interval out.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            self.refresh_ddragon().await;
        }
    }

    /// One refresh pass: re-resolve the live patch and, only if it changed, reload
    /// and hot-swap the tables. A pinned override is left untouched, and a failed
    /// version lookup keeps the current tables — we never downgrade the running
    /// patch on a transient blip.
    async fn refresh_ddragon(&self) {
        if self.ddragon_version_override.is_some() {
            return;
        }
        let Some(latest) = ddragon::fetch_latest_version(&self.http).await else {
            return; // logged inside; keep serving the current tables
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

    /// Filename for the rendered card attachment. Extension tracks
    /// [`SCREENSHOT_TYPE`] so Discord previews it correctly.
    pub const ATTACHMENT_FILENAME: &'static str = "match_result.jpg";

    /// Resolve a champion's numeric id (`Ahri` → `103`) from the boot-time
    /// DDragon tables, for the poller's champion-mastery lookup. `None` when the
    /// champion index failed to load or the name is unknown.
    pub fn champion_id(&self, champion_name: &str) -> Option<i64> {
        self.ddragon.load().champion_id(champion_name)
    }

    /// Build the scorecard HTML and render it to an image via the remote renderer.
    pub async fn generate_match_image(
        &self,
        ctx: &MatchImageContext<'_>,
    ) -> Result<Vec<u8>, AppError> {
        // Snapshot the current tables: a concurrent refresh may swap in the next
        // patch's tables, but this card renders consistently against one snapshot.
        let ddragon = self.ddragon.load_full();
        let mut vm = ScorecardVm::from_context(ctx, &ddragon);
        self.inline_assets(&mut vm).await;

        // Font strategy: a pure-Latin name gets the invariant inlined Latin faces
        // (no network at capture → the renderer waits on `load`); a CJK name keeps
        // the Google Fonts `<link>` (needs `networkidle0`). An inlining failure
        // also falls back to the `<link>`, so the card is never fontless.
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

    /// Render with a bounded retry: the always-on renderer is being retired in
    /// favour of scale-to-zero, so the first request may land on a cold Chrome
    /// that overruns the launch timeout. One spaced retry lands on a warm Chrome;
    /// only if every attempt fails does the caller fall back to a text embed.
    async fn render_with_retry(&self, html: &str, fonts_inlined: bool) -> Result<Vec<u8>, AppError> {
        // A missing renderer is a permanent config error, not a transient cold
        // start — don't burn the retry delay on it; fall straight to the fallback.
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

    /// Fetch every image the card references through the cache and inline it as a
    /// data URI, so the render is deterministic and offline (no DDragon fetches
    /// at capture time). Fetches that fail are simply left as their original URL,
    /// which the renderer can still resolve directly.
    async fn inline_assets(&self, vm: &mut ScorecardVm) {
        // Dedup first, then fetch the card's assets with bounded concurrency so a
        // cold card (~15 assets) isn't serialised on network round-trips.
        let mut urls = vm.image_urls();
        urls.sort();
        urls.dedup();

        let map: HashMap<String, String> = stream::iter(urls)
            .map(|url| async move {
                self.cache
                    .get_or_fetch(&self.http, &url)
                    .await
                    .map(|data_uri| (url, data_uri))
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        vm.apply_asset_map(&map);
    }

    /// POST the resolved HTML to Browserless and return the image bytes. Crops to
    /// the `#card-root` element and waits for web fonts before capturing, so the
    /// card never renders in a fallback font. With `fonts_inlined` (and images
    /// already inlined) the page makes no network requests, so we wait on `load`;
    /// otherwise we wait on `networkidle0` for the Google Fonts fetch to settle.
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
                // Tall enough to lay the card out; the element crop trims the rest.
                height: 1200,
                device_scale_factor: 2,
            },
            goto_options: GotoOptions {
                // Fully-inlined page has no network to idle on; wait on `load`.
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
        debug!(size = bytes.len(), "🖼️ ✅ Card rendered via renderer");
        Ok(bytes.to_vec())
    }

    /// The invariant inlined Latin font `<style>` payload, built once and reused.
    /// Returns `None` if the one-time fetch/parse fails — the caller then keeps
    /// the Google Fonts `<link>` path — and does not cache the failure, so a
    /// later render retries the build.
    async fn latin_font_css(&self) -> Option<Arc<str>> {
        if let Some(css) = self.latin_font_css.read().await.clone() {
            return Some(css);
        }

        match self.build_latin_font_css().await {
            Ok(css) => {
                let arc: Arc<str> = Arc::from(css);
                // A concurrent first render may race here; both produce the same
                // bytes, so last-writer-wins is harmless.
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

    /// Fetch the Latin font stylesheet, inline its `latin`/`latin-ext` faces as
    /// data URIs, and sanity-check the result.
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

        // Guard against Google changing its CSS shape (subset comments, url()
        // format): if we didn't inline both families' faces, treat it as a parse
        // failure and keep the resilient `<link>` path rather than ship a card in
        // a fallback font.
        let woff2_count = inlined.matches("data:font/woff2").count();
        if !(inlined.contains("Cinzel") && inlined.contains("Barlow") && woff2_count >= 4) {
            return Err(AppError::ImageGeneration {
                message: format!("font inlining produced too few faces ({woff2_count} woff2)"),
            });
        }
        Ok(inlined)
    }
}

/// Rewrite a Google Fonts stylesheet into a self-contained one: keep only the
/// `@font-face` blocks whose `/* subset */` comment is in [`INLINED_SUBSETS`],
/// and replace each `url(https://…woff2)` in them with an inlined data URI.
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

/// Split a Google Fonts stylesheet into `(subset, block)` pairs. Google emits a
/// `/* subset */` comment (e.g. `/* latin */`, or `/* [0] */` for CJK) before
/// each `@font-face` block; the block runs until the next comment or end.
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

/// Replace every `url(https://…)` in a single `@font-face` block with the fetched
/// bytes as a `data:font/woff2` URI. Errors if any referenced font can't be
/// fetched, so the caller falls back to the `<link>` path wholesale rather than
/// ship a block with a dangling gstatic URL.
async fn inline_block_urls(http: &Client, block: &str) -> Result<String, AppError> {
    let mut inlined = block.to_string();
    for url in extract_css_urls(block) {
        let bytes = fetch_font(http, &url).await?;
        let data_uri = to_font_data_uri(&bytes);
        inlined = inlined.replace(&format!("url({url})"), &format!("url({data_uri})"));
    }
    Ok(inlined)
}

/// Every `http(s)` URL inside `url(...)` occurrences of a CSS fragment (unquoted,
/// as Google Fonts emits them).
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

/// Fetch a single font file (best-effort, with the woff2-eliciting UA).
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

/// `data:font/woff2` URI for font bytes. All inlined faces are woff2 by
/// construction (Chrome UA + Google's `format('woff2')` src).
fn to_font_data_uri(bytes: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    format!("data:font/woff2;base64,{b64}")
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
    /// 0–100 for JPEG/WebP; omitted for PNG (Browserless rejects `quality` on a
    /// PNG screenshot).
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
    /// Arrow-function source evaluated in the page (Browserless `fn`).
    #[serde(rename = "fn")]
    func: &'static str,
    timeout: u32,
}

/// Whether a cache entry created at `created_at` is still fresh under `ttl`.
/// `None` marks an immutable asset (versioned or versionless DDragon): always
/// fresh — only the disk size cap ever removes it.
fn is_fresh(created_at: SystemTime, ttl: Option<Duration>) -> bool {
    match ttl {
        None => true,
        Some(ttl) => created_at.elapsed().unwrap_or(Duration::MAX) <= ttl,
    }
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
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime};

    use tokio::sync::RwLock;

    use super::{ImageCache, to_data_uri};

    /// Build an `ImageCache` rooted at a throwaway temp dir with explicit caps.
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

    /// Immutable DDragon assets bypass the TTL (size-eviction only); everything
    /// else (CommunityDragon `latest`) carries the mutable TTL.
    #[test]
    fn ttl_for_classifies_ddragon_as_immutable() {
        let cache = temp_cache("ttl", 100, 24);
        // Versioned DDragon (spell/item/square) and versionless DDragon
        // (splash/rune art) are both immutable.
        assert_eq!(
            cache.ttl_for("https://ddragon.leagueoflegends.com/cdn/16.1.1/img/item/1001.png"),
            None
        );
        assert_eq!(
            cache.ttl_for(
                "https://ddragon.leagueoflegends.com/cdn/img/champion/loading/Ahri_0.jpg"
            ),
            None
        );
        // CommunityDragon `latest` is a rolling pointer → keeps the TTL.
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

    /// A file already on disk is served (and promoted into the in-RAM layer)
    /// without any network fetch — the lazy path that replaced the boot preload.
    #[tokio::test]
    async fn disk_hit_promotes_to_memory_without_network() {
        let cache = temp_cache("diskhit", 100, 24);
        let url = "https://ddragon.example/img/item/1001.png";
        let key = ImageCache::cache_key(url);

        let png = [0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
        tokio::fs::write(cache.get_cache_path(&key), &png)
            .await
            .unwrap();

        // Client is required by the signature but never used on a disk hit.
        let client = reqwest::Client::new();
        let uri = cache.get_or_fetch(&client, url).await.expect("disk hit");
        assert!(uri.starts_with("data:image/png;base64,"));
        assert!(
            cache.memory_cache.read().await.contains_key(&key),
            "disk hit must promote the entry into the memory layer"
        );

        let _ = std::fs::remove_dir_all(&cache.cache_dir);
    }

    /// The in-RAM layer stays under its own cap and drops the least-recently-used
    /// entry, independently of the (much larger) disk cap.
    #[tokio::test]
    async fn memory_layer_evicts_least_recently_used() {
        // 1 MB cap; three ~400 KB entries overflow it, forcing one eviction.
        let cache = temp_cache("lru", 100, 1);
        let blob = vec![0u8; 400_000];
        let store = |k: &'static str| {
            let bytes: Arc<[u8]> = Arc::from(blob.as_slice());
            cache.store_in_memory(k.to_string(), bytes, SystemTime::now())
        };

        store("a").await;
        store("b").await;
        // Touch "a" so "b" becomes the least-recently-used victim.
        {
            let mem = cache.memory_cache.read().await;
            mem["a"].last_access.store(cache.tick(), Ordering::Relaxed);
        }
        store("c").await;

        let mem = cache.memory_cache.read().await;
        let total: u64 = mem.values().map(|e| e.bytes.len() as u64).sum();
        assert!(total <= cache.memory_max_bytes, "memory layer stayed over cap");
        assert!(mem.contains_key("a"), "recently-touched entry must survive");
        assert!(mem.contains_key("c"), "freshly-inserted entry must survive");
        assert!(!mem.contains_key("b"), "the LRU entry must be evicted");

        drop(mem);
        let _ = std::fs::remove_dir_all(&cache.cache_dir);
    }

    /// A realistic Google Fonts stylesheet slice: two subset comments, an
    /// unquoted gstatic `url(...)`, and a numbered CJK subset that must be dropped.
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
        // The block text travels with its subset comment.
        assert!(segs[1].1.contains("barlow/lat.woff2"));
        assert!(segs[2].1.contains("Noto Sans JP"));
    }

    #[test]
    fn extract_css_urls_reads_unquoted_gstatic_urls() {
        let urls = super::extract_css_urls(SAMPLE_FONT_CSS);
        assert_eq!(urls.len(), 3);
        assert!(urls[0].ends_with("cyr.woff2"));
        assert!(urls.iter().all(|u| u.starts_with("https://fonts.gstatic.com")));
        // No `url()` → no URLs, and a relative/data url is skipped.
        assert!(super::extract_css_urls("body{color:red}").is_empty());
        assert!(super::extract_css_urls("src:url(data:font/woff2;base64,AA)").is_empty());
    }

    #[test]
    fn to_font_data_uri_labels_woff2() {
        assert!(super::to_font_data_uri(b"wOF2\x00\x01").starts_with("data:font/woff2;base64,"));
    }

    /// Opt-in (network) end-to-end check of the font-inlining path against the
    /// live Google Fonts CSS — catches drift in Google's stylesheet shape that
    /// the offline parser tests can't. Run with:
    ///   cargo test --quiet inline_real_google_fonts -- --ignored
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
