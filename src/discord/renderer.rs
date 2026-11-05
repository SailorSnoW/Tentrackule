use std::time::{Duration, Instant};

use reqwest::header::{CONTENT_TYPE, HeaderMap, RETRY_AFTER};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use tracing::{debug, info, warn};

use super::scorecard::{CARD_WIDTH_PX, FONTS_READY_SELECTOR};
use crate::error::AppError;
use crate::metrics::Metrics;

const API_BASE: &str = "https://api.cloudflare.com/client/v4";
const BROWSER_MS_HEADER: &str = "x-browser-ms-used";
const DAILY_QUOTA_MESSAGE: &str = "time limit";

const RENDER_TIMEOUT: Duration = Duration::from_secs(30);
const RENDER_ATTEMPTS: u32 = 3;
const TRANSIENT_RETRY_DELAY: Duration = Duration::from_secs(5);
const DEFAULT_RATE_LIMIT_DELAY: Duration = Duration::from_secs(10);
const MAX_RATE_LIMIT_DELAY: Duration = Duration::from_secs(30);

const SCREENSHOT_TYPE: &str = "jpeg";
const SCREENSHOT_QUALITY: Option<u32> = Some(90);

pub(super) struct Renderer {
    endpoint: String,
    api_token: String,
    transient_retry_delay: Duration,
}

struct RenderFailure {
    error: AppError,
    retry_in: Option<Duration>,
}

impl Renderer {
    pub(super) fn from_config(
        account_id: Option<String>,
        api_token: Option<String>,
    ) -> Option<Self> {
        match (account_id, api_token) {
            (Some(account_id), Some(api_token)) => Some(Self {
                endpoint: format!(
                    "{API_BASE}/accounts/{}/browser-run/screenshot",
                    account_id.trim()
                ),
                api_token,
                transient_retry_delay: TRANSIENT_RETRY_DELAY,
            }),
            _ => {
                warn!(
                    "🖼️ ⚠️ Renderer not configured (CLOUDFLARE_ACCOUNT_ID/CLOUDFLARE_API_TOKEN); \
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
                Err(RenderFailure {
                    error,
                    retry_in: Some(delay),
                }) if attempt < RENDER_ATTEMPTS => {
                    warn!(
                        attempt,
                        error = ?error,
                        delay_s = delay.as_secs(),
                        "🖼️ ⚠️ Render attempt failed; retrying after delay"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                Err(failure) => return Err(failure.error),
            }
        }
    }

    async fn screenshot(
        &self,
        http: &Client,
        metrics: &Metrics,
        html: &str,
        fonts_inlined: bool,
    ) -> Result<Vec<u8>, RenderFailure> {
        let started = Instant::now();
        let response = http
            .post(&self.endpoint)
            .bearer_auth(&self.api_token)
            .json(&ScreenshotRequest::new(html, fonts_inlined))
            .timeout(RENDER_TIMEOUT)
            .send()
            .await
            .map_err(|e| self.transient(e.into()))?;

        let browser_ms = browser_ms_used(response.headers());
        if let Some(ms) = browser_ms {
            metrics.record_render_browser_ms(ms);
        }

        let status = response.status();
        if !status.is_success() {
            let retry_after = header_str(response.headers(), RETRY_AFTER).map(str::to_owned);
            let body = response.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(200).collect();
            return Err(RenderFailure {
                retry_in: retry_delay(
                    status,
                    retry_after.as_deref(),
                    &body,
                    self.transient_retry_delay,
                ),
                error: AppError::ImageGeneration {
                    message: format!("renderer returned {status}: {snippet}"),
                },
            });
        }

        let content_type = header_str(response.headers(), CONTENT_TYPE).unwrap_or_default();
        if !content_type.starts_with("image/") {
            let message = format!("renderer returned {content_type:?} instead of an image");
            return Err(self.transient(AppError::ImageGeneration { message }));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| self.transient(e.into()))?;
        metrics.record_render(started.elapsed());
        debug!(size = bytes.len(), browser_ms, "🖼️ ✅ Card rendered");
        Ok(bytes.to_vec())
    }

    fn transient(&self, error: AppError) -> RenderFailure {
        RenderFailure {
            error,
            retry_in: Some(self.transient_retry_delay),
        }
    }
}

fn header_str(headers: &HeaderMap, name: impl reqwest::header::AsHeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn browser_ms_used(headers: &HeaderMap) -> Option<u64> {
    let ms = header_str(headers, BROWSER_MS_HEADER)?
        .trim()
        .parse::<f64>()
        .ok()?;
    (ms.is_finite() && ms >= 0.0).then(|| ms.round() as u64)
}

fn retry_delay(
    status: StatusCode,
    retry_after: Option<&str>,
    body: &str,
    transient_delay: Duration,
) -> Option<Duration> {
    match status {
        StatusCode::TOO_MANY_REQUESTS => rate_limit_delay(retry_after, body),
        StatusCode::UNPROCESSABLE_ENTITY => Some(transient_delay),
        s if s.is_server_error() => Some(transient_delay),
        _ => None,
    }
}

fn rate_limit_delay(retry_after: Option<&str>, body: &str) -> Option<Duration> {
    if body.to_ascii_lowercase().contains(DAILY_QUOTA_MESSAGE) {
        return None;
    }
    match retry_after.and_then(|value| value.trim().parse::<u64>().ok()) {
        Some(secs) => Some(Duration::from_secs(secs)).filter(|d| *d <= MAX_RATE_LIMIT_DELAY),
        None => Some(DEFAULT_RATE_LIMIT_DELAY),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotRequest<'a> {
    html: &'a str,
    selector: &'a str,
    screenshot_options: ScreenshotOptions,
    viewport: Viewport,
    goto_options: GotoOptions,
    wait_for_selector: WaitForSelector,
}

impl<'a> ScreenshotRequest<'a> {
    fn new(html: &'a str, fonts_inlined: bool) -> Self {
        Self {
            html,
            selector: "#card-root",
            screenshot_options: ScreenshotOptions {
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
            wait_for_selector: WaitForSelector {
                selector: FONTS_READY_SELECTOR,
                timeout: 5_000,
            },
        }
    }
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
struct WaitForSelector {
    selector: &'static str,
    timeout: u32,
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    use super::*;

    const JPEG_OK: &str = "HTTP/1.1 200 OK\r\ncontent-type: image/jpeg\r\nx-browser-ms-used: 12.6\r\ncontent-length: 4\r\nconnection: close\r\n\r\nJPEG";

    fn error_response(status: &str, extra_header: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{extra_header}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn read_request(stream: &mut TcpStream) -> String {
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = stream.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&raw);
            let Some(split) = text.find("\r\n\r\n") else {
                continue;
            };
            let content_length = text[..split]
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if raw.len() >= split + 4 + content_length {
                break;
            }
        }
        String::from_utf8_lossy(&raw).into_owned()
    }

    fn mock_renderer(responses: Vec<String>) -> (Renderer, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/screenshot", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                recorded.lock().unwrap().push(read_request(&mut stream));
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        let renderer = Renderer {
            endpoint,
            api_token: "test-token".into(),
            transient_retry_delay: Duration::ZERO,
        };
        (renderer, requests)
    }

    async fn run(renderer: &Renderer) -> Result<Vec<u8>, AppError> {
        renderer
            .render(&Client::new(), &Metrics::new(), "<html></html>", true)
            .await
    }

    #[tokio::test]
    async fn sends_bearer_auth_and_cloudflare_payload() {
        let (renderer, requests) = mock_renderer(vec![JPEG_OK.into()]);

        let bytes = run(&renderer).await.expect("render should succeed");
        assert_eq!(bytes, b"JPEG");

        let requests = requests.lock().unwrap();
        let request = &requests[0];
        assert!(request.starts_with("POST /screenshot HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test-token"),
            "token must travel in the Authorization header"
        );
        assert!(request.contains("\"html\":\"<html></html>\""));
        assert!(request.contains("\"selector\":\"#card-root\""));
        assert!(request.contains("\"screenshotOptions\":{\"type\":\"jpeg\",\"quality\":90}"));
        assert!(request.contains("\"deviceScaleFactor\":2"));
        assert!(request.contains("\"gotoOptions\":{\"waitUntil\":\"load\",\"timeout\":15000}"));
        assert!(request.contains(&format!(
            "\"waitForSelector\":{{\"selector\":\"{FONTS_READY_SELECTOR}\",\"timeout\":5000}}"
        )));
    }

    #[tokio::test]
    async fn font_link_fallback_waits_for_network_idle() {
        let (renderer, requests) = mock_renderer(vec![JPEG_OK.into()]);

        renderer
            .render(&Client::new(), &Metrics::new(), "<html></html>", false)
            .await
            .expect("render should succeed");

        assert!(requests.lock().unwrap()[0].contains("\"waitUntil\":\"networkidle0\""));
    }

    #[tokio::test]
    async fn rate_limit_waits_retry_after_then_succeeds() {
        let (renderer, requests) = mock_renderer(vec![
            error_response(
                "429 Too Many Requests",
                "retry-after: 0\r\n",
                r#"{"success":false,"errors":[{"code":2001,"message":"Rate limit exceeded"}]}"#,
            ),
            JPEG_OK.into(),
        ]);

        assert_eq!(run(&renderer).await.unwrap(), b"JPEG");
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn daily_quota_is_not_retried() {
        let (renderer, requests) = mock_renderer(vec![
            error_response(
                "429 Too Many Requests",
                "",
                r#"{"success":false,"errors":[{"message":"Browser time limit exceeded for today"}]}"#,
            ),
            JPEG_OK.into(),
        ]);

        let error = run(&renderer).await.expect_err("quota must not be retried");
        assert!(error.to_string().contains("time limit"));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn auth_failure_is_not_retried() {
        let (renderer, requests) = mock_renderer(vec![
            error_response("401 Unauthorized", "", r#"{"success":false}"#),
            JPEG_OK.into(),
        ]);

        assert!(run(&renderer).await.is_err());
        assert_eq!(requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn transient_failures_are_retried_until_success() {
        let (renderer, requests) = mock_renderer(vec![
            error_response("503 Service Unavailable", "", "{}"),
            error_response("422 Unprocessable Entity", "", "{}"),
            JPEG_OK.into(),
        ]);

        assert_eq!(run(&renderer).await.unwrap(), b"JPEG");
        assert_eq!(requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn gives_up_after_the_attempt_budget() {
        let (renderer, requests) = mock_renderer(vec![
            error_response("503 Service Unavailable", "", "{}"),
            error_response("503 Service Unavailable", "", "{}"),
            error_response("503 Service Unavailable", "", "{}"),
            JPEG_OK.into(),
        ]);

        assert!(run(&renderer).await.is_err());
        assert_eq!(requests.lock().unwrap().len(), RENDER_ATTEMPTS as usize);
    }

    #[tokio::test]
    async fn non_image_success_is_rejected_and_retried() {
        let (renderer, requests) = mock_renderer(vec![
            error_response("200 OK", "", r#"{"success":false}"#),
            JPEG_OK.into(),
        ]);

        assert_eq!(run(&renderer).await.unwrap(), b"JPEG");
        assert_eq!(requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn rate_limit_delay_honours_retry_after_within_bounds() {
        assert_eq!(
            rate_limit_delay(Some("7"), ""),
            Some(Duration::from_secs(7))
        );
        assert_eq!(
            rate_limit_delay(None, "Rate limit exceeded"),
            Some(DEFAULT_RATE_LIMIT_DELAY)
        );
        assert_eq!(
            rate_limit_delay(Some("Wed, 21 Oct 2026 07:28:00 GMT"), ""),
            Some(DEFAULT_RATE_LIMIT_DELAY)
        );
        assert_eq!(rate_limit_delay(Some("3600"), ""), None);
        assert_eq!(
            rate_limit_delay(Some("5"), "Browser Time Limit exceeded"),
            None
        );
    }

    #[test]
    fn retry_delay_only_retries_recoverable_statuses() {
        let delay = Duration::from_secs(5);
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::PAYLOAD_TOO_LARGE,
        ] {
            assert_eq!(retry_delay(status, None, "", delay), None, "{status}");
        }
        for status in [
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
        ] {
            assert_eq!(
                retry_delay(status, None, "", delay),
                Some(delay),
                "{status}"
            );
        }
    }

    #[test]
    fn browser_ms_header_is_parsed_and_rounded() {
        let mut headers = HeaderMap::new();
        assert_eq!(browser_ms_used(&headers), None);

        headers.insert(BROWSER_MS_HEADER, "1057.835205078125".parse().unwrap());
        assert_eq!(browser_ms_used(&headers), Some(1058));

        headers.insert(BROWSER_MS_HEADER, "not-a-number".parse().unwrap());
        assert_eq!(browser_ms_used(&headers), None);

        headers.insert(BROWSER_MS_HEADER, "-3".parse().unwrap());
        assert_eq!(browser_ms_used(&headers), None);
    }

    #[test]
    fn from_config_requires_both_credentials() {
        assert!(Renderer::from_config(None, None).is_none());
        assert!(Renderer::from_config(Some("acct".into()), None).is_none());
        assert!(Renderer::from_config(None, Some("tok".into())).is_none());

        let renderer = Renderer::from_config(Some(" acct ".into()), Some("tok".into())).unwrap();
        assert_eq!(
            renderer.endpoint,
            "https://api.cloudflare.com/client/v4/accounts/acct/browser-run/screenshot"
        );
    }
}
