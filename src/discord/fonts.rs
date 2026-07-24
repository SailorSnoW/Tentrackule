use base64::Engine;
use reqwest::Client;

use crate::error::AppError;

const LATIN_FONT_CSS_URL: &str = "https://fonts.googleapis.com/css2?family=Cinzel:wght@600;700&family=Barlow+Semi+Condensed:wght@400;500;600;700&display=swap";
const FONT_FETCH_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const INLINED_SUBSETS: &[&str] = &["latin", "latin-ext"];

pub(super) async fn build_latin_css(http: &Client) -> Result<String, AppError> {
    let css = http
        .get(LATIN_FONT_CSS_URL)
        .header(reqwest::header::USER_AGENT, FONT_FETCH_UA)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    let inlined = inline_font_faces(http, &css).await?;

    let woff2_count = inlined.matches("data:font/woff2").count();
    if !(inlined.contains("Cinzel") && inlined.contains("Barlow") && woff2_count >= 4) {
        return Err(AppError::ImageGeneration {
            message: format!("font inlining produced too few faces ({woff2_count} woff2)"),
        });
    }
    Ok(inlined)
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

#[cfg(test)]
mod tests {
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
}
