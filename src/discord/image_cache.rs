use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use base64::Engine;
use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, trace, warn};

use super::ddragon::DDRAGON_CDN;
use crate::metrics::Metrics;

const MUTABLE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const DISK_MAX_SIZE_MB: u64 = 100;
const MEMORY_MAX_SIZE_MB: u64 = 24;
const CACHE_DIR: &str = ".cache/images";

struct MemoryEntry {
    bytes: Arc<[u8]>,
    created_at: SystemTime,
    last_access: AtomicU64,
}

pub(super) struct ImageCache {
    http: Client,
    metrics: Arc<Metrics>,
    memory_cache: RwLock<HashMap<String, MemoryEntry>>,
    cache_dir: PathBuf,
    mutable_ttl: Duration,
    disk_max_bytes: u64,
    memory_max_bytes: u64,
    clock: AtomicU64,
}

impl ImageCache {
    pub(super) async fn new(http: Client, metrics: Arc<Metrics>) -> Self {
        let cache_dir = PathBuf::from(CACHE_DIR);

        if let Err(e) = fs::create_dir_all(&cache_dir).await {
            warn!(error = ?e, "🖼️ ⚠️ Failed to create cache directory");
        }

        Self {
            http,
            metrics,
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
        self.cache_dir.join(format!("{key}.png"))
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

    pub(super) async fn get_or_fetch(&self, url: &str) -> Option<String> {
        let key = Self::cache_key(url);
        let ttl = self.ttl_for(url);

        {
            let cache = self.memory_cache.read().await;
            if let Some(entry) = cache.get(&key)
                && is_fresh(entry.created_at, ttl)
            {
                entry.last_access.store(self.tick(), Ordering::Relaxed);
                self.metrics.record_cache_hit();
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
                    self.metrics.record_cache_hit();
                    trace!(url, "🖼️ Disk cache hit");
                    return Some(data_uri);
                }
            } else {
                let _ = fs::remove_file(&cache_path).await;
            }
        }

        self.metrics.record_cache_miss();
        trace!(url, "🖼️ Fetching image");
        let bytes = match self.http.get(url).send().await {
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
            http: reqwest::Client::new(),
            metrics: Arc::new(crate::metrics::Metrics::new()),
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

        let uri = cache.get_or_fetch(url).await.expect("disk hit");
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
