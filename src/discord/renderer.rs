use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use tracing::{debug, info, warn};

use super::scorecard::CARD_WIDTH_PX;
use crate::error::AppError;
use crate::metrics::Metrics;

const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

const RENDER_ATTEMPTS: u32 = 2;
const RENDER_RETRY_DELAY: Duration = Duration::from_secs(25);

const SCREENSHOT_TYPE: &str = "jpeg";
const SCREENSHOT_QUALITY: Option<u32> = Some(90);

pub(super) struct Renderer {
    base_url: String,
    token: String,
}

impl Renderer {
    pub(super) fn from_config(base_url: Option<String>, token: Option<String>) -> Option<Self> {
        match (base_url, token) {
            (Some(base_url), Some(token)) => Some(Self {
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
        }
    }

    pub(super) async fn render(
        &self,
        http: &Client,
        metrics: &Metrics,
        html: &str,
        fonts_inlined: bool,
    ) -> Result<Vec<u8>, AppError> {
        let mut attempt: u32 = 1;
        loop {
            match self.screenshot(http, metrics, html, fonts_inlined).await {
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

    async fn screenshot(
        &self,
        http: &Client,
        metrics: &Metrics,
        html: &str,
        fonts_inlined: bool,
    ) -> Result<Vec<u8>, AppError> {
        let url = format!("{}/screenshot?token={}", self.base_url, self.token);
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
        let response = http
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
        metrics.record_render(started.elapsed());
        debug!(size = bytes.len(), "🖼️ ✅ Card rendered via renderer");
        Ok(bytes.to_vec())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotRequest<'a> {
    html: &'a str,
    selector: &'a str,
    options: ScreenshotOptions,
    viewport: Viewport,
    goto_options: GotoOptions,
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
#[serde(rename_all = "camelCase")]
struct Viewport {
    width: u32,
    height: u32,
    device_scale_factor: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GotoOptions {
    wait_until: &'static str,
    timeout: u32,
}

#[derive(Serialize)]
struct WaitForFunction {
    #[serde(rename = "fn")]
    func: &'static str,
    timeout: u32,
}
