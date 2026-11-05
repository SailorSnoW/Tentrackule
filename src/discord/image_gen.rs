use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use futures::stream::{self, StreamExt};
use reqwest::Client;
use tokio::sync::RwLock;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, info, warn};

use super::ddragon::{self, DdragonData};
use super::fonts;
use super::image_cache::ImageCache;
use super::renderer::Renderer;
use super::scorecard::ScorecardVm;
use crate::config::USER_AGENT;
use crate::db::{Player, RankInfo};
use crate::error::AppError;
use crate::metrics::Metrics;
use crate::riot::{InfoDto, ParticipantDto};

const FETCH_CONCURRENCY: usize = 4;
const DDRAGON_REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

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

pub struct ImageGenerator {
    http: Client,
    cache: ImageCache,
    ddragon: ArcSwap<DdragonData>,
    ddragon_version_override: Option<String>,
    renderer: Option<Renderer>,
    latin_font_css: RwLock<Option<Arc<str>>>,
    metrics: Arc<Metrics>,
}

impl ImageGenerator {
    pub async fn new(
        ddragon_version_override: Option<String>,
        cloudflare_account_id: Option<String>,
        cloudflare_api_token: Option<String>,
        metrics: Arc<Metrics>,
    ) -> Result<Self, AppError> {
        let http = Client::builder().user_agent(USER_AGENT).build()?;

        let version = ddragon::resolve_version(&http, ddragon_version_override.as_deref()).await;
        let ddragon = DdragonData::load(&http, &version).await;

        let cache = ImageCache::new(http.clone(), Arc::clone(&metrics)).await;

        Ok(Self {
            http,
            cache,
            ddragon: ArcSwap::from_pointee(ddragon),
            ddragon_version_override,
            renderer: Renderer::from_config(cloudflare_account_id, cloudflare_api_token),
            latin_font_css: RwLock::new(None),
            metrics,
        })
    }

    pub fn ddragon_version(&self) -> String {
        self.ddragon.load().version().to_string()
    }

    pub fn has_renderer(&self) -> bool {
        self.renderer.is_some()
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
        self.render_scorecard(ScorecardVm::from_context(ctx, &ddragon))
            .await
    }

    pub(super) async fn render_scorecard(&self, mut vm: ScorecardVm) -> Result<Vec<u8>, AppError> {
        let Some(renderer) = &self.renderer else {
            return Err(AppError::ImageGeneration {
                message: "renderer not configured".to_string(),
            });
        };

        self.inline_assets(&mut vm).await;

        let fonts_inlined = match self.latin_font_css().await {
            Some(css) => {
                vm.set_inlined_font_css(css);
                true
            }
            None => false,
        };

        renderer
            .render(&self.http, &self.metrics, &vm.build_html(), fonts_inlined)
            .await
    }

    async fn inline_assets(&self, vm: &mut ScorecardVm) {
        let mut urls = vm.image_urls();
        urls.sort();
        urls.dedup();

        let map: HashMap<String, String> = stream::iter(urls)
            .map(|url| async move {
                self.cache
                    .get_or_fetch(&url)
                    .await
                    .map(|data_uri| (url, data_uri))
            })
            .buffer_unordered(FETCH_CONCURRENCY)
            .filter_map(|entry| async move { entry })
            .collect()
            .await;

        vm.apply_asset_map(&map);
    }

    async fn latin_font_css(&self) -> Option<Arc<str>> {
        if let Some(css) = self.latin_font_css.read().await.clone() {
            return Some(css);
        }

        match fonts::build_latin_css(&self.http).await {
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
}
