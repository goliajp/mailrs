//! email rendering preview engine
//!
//! connects to a headless Chrome instance via CDP (Chrome DevTools Protocol)
//! to render HTML emails and capture screenshots at different viewport sizes.
//! used to preview how emails will look in different clients.

use std::path::PathBuf;
use std::sync::Arc;

use chromiumoxide::Browser;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, Semaphore};

const RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const CACHE_DIR: &str = "/tmp/mailrs-render-cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewportPreset {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
    pub is_mobile: bool,
    pub inject_css: Option<String>,
    pub strip_style_tags: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderResult {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub image_url: String,
}

pub fn default_presets() -> Vec<ViewportPreset> {
    vec![
        ViewportPreset {
            name: "desktop".into(),
            width: 660,
            height: 900,
            device_scale_factor: 1.0,
            is_mobile: false,
            inject_css: None,
            strip_style_tags: false,
        },
        ViewportPreset {
            name: "mobile".into(),
            width: 375,
            height: 812,
            device_scale_factor: 2.0,
            is_mobile: true,
            inject_css: None,
            strip_style_tags: false,
        },
        ViewportPreset {
            name: "gmail".into(),
            width: 660,
            height: 900,
            device_scale_factor: 1.0,
            is_mobile: false,
            inject_css: Some("body { font-family: Roboto, Arial, sans-serif !important; }".into()),
            strip_style_tags: true, // gmail strips <style> blocks
        },
        ViewportPreset {
            name: "outlook".into(),
            width: 660,
            height: 900,
            device_scale_factor: 1.0,
            is_mobile: false,
            inject_css: Some(
                "* { display: block !important; border-radius: 0 !important; box-shadow: none !important; } \
                 body { font-family: Calibri, Arial, sans-serif !important; }"
                    .into(),
            ),
            strip_style_tags: false,
        },
    ]
}

pub fn find_preset(name: &str) -> Option<ViewportPreset> {
    default_presets().into_iter().find(|p| p.name == name)
}

pub struct RenderPreviewClient {
    cdp_url: String,
    browser: Mutex<Option<Arc<Browser>>>,
    semaphore: Arc<Semaphore>,
    cache_dir: PathBuf,
}

impl RenderPreviewClient {
    pub fn new(cdp_url: String, max_concurrent: usize) -> Self {
        let cache_dir = PathBuf::from(CACHE_DIR);
        let _ = std::fs::create_dir_all(&cache_dir);
        Self {
            cdp_url,
            browser: Mutex::new(None),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            cache_dir,
        }
    }

    /// resolve the full WebSocket debugger URL from the CDP base URL
    async fn resolve_ws_url(&self) -> Result<String, String> {
        let host = self.cdp_url
            .replace("ws://", "")
            .replace("wss://", "")
            .split('/')
            .next()
            .unwrap_or("localhost:9222")
            .to_string();
        let version_url = format!("http://{host}/json/version");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        let text = client.get(&version_url)
            .send()
            .await
            .map_err(|e| format!("CDP version query failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("CDP version read failed: {e}"))?;

        let resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("CDP version parse failed: {e} body={}", &text[..text.len().min(200)]))?;

        let ws_url = resp["webSocketDebuggerUrl"]
            .as_str()
            .ok_or("webSocketDebuggerUrl not found in /json/version")?;

        // replace localhost with the container hostname
        let fixed = ws_url.replace("localhost:9222", &host).replace("127.0.0.1:9222", &host);
        eprintln!("render_preview: resolved WS URL: {fixed}");
        Ok(fixed)
    }

    async fn get_browser(&self) -> Result<Arc<Browser>, String> {
        let mut guard = self.browser.lock().await;
        if let Some(ref browser) = *guard {
            return Ok(browser.clone());
        }

        // resolve full WebSocket URL
        let ws_url = self.resolve_ws_url().await?;
        eprintln!("render_preview: connecting to {ws_url}");

        // connect to remote Chrome
        let (browser, mut handler) = Browser::connect(&ws_url)
            .await
            .map_err(|e| format!("CDP connect failed: {e}"))?;

        // spawn handler to process CDP events
        tokio::spawn(async move {
            while handler.next().await.is_some() {}
        });

        let browser = Arc::new(browser);
        *guard = Some(browser.clone());
        eprintln!("render_preview: connected to Chrome at {}", self.cdp_url);
        Ok(browser)
    }

    /// render a single preset and return cached file path
    async fn render_single(
        &self,
        html: &str,
        preset: &ViewportPreset,
    ) -> Result<RenderResult, String> {
        // check cache
        let cache_key = cache_hash(html, &preset.name);
        let cache_path = self.cache_dir.join(format!("{cache_key}.png"));
        if cache_path.exists() {
            return Ok(RenderResult {
                name: preset.name.clone(),
                width: preset.width,
                height: preset.height,
                image_url: format!("/api/mail/render-preview/cache/{cache_key}.png"),
            });
        }

        let _permit = self.semaphore.acquire().await.map_err(|e| e.to_string())?;

        let browser = self.get_browser().await?;

        // preprocess html for client simulation
        let processed_html = preprocess_html(html, preset);

        // create a new page with viewport
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("new page failed: {e}"))?;

        page.set_content(&processed_html)
            .await
            .map_err(|e| format!("set content failed: {e}"))?;

        // wait for rendering
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // capture screenshot
        let screenshot = tokio::time::timeout(
            RENDER_TIMEOUT,
            page.screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .full_page(true)
                    .build(),
            ),
        )
        .await
        .map_err(|_| "screenshot timeout".to_string())?
        .map_err(|e| format!("screenshot failed: {e}"))?;

        // close page
        let _ = page.close().await;

        // save to cache
        tokio::fs::write(&cache_path, &screenshot)
            .await
            .map_err(|e| format!("cache write failed: {e}"))?;

        Ok(RenderResult {
            name: preset.name.clone(),
            width: preset.width,
            height: preset.height,
            image_url: format!("/api/mail/render-preview/cache/{cache_key}.png"),
        })
    }

    /// render html with multiple presets
    pub async fn render(
        &self,
        html: &str,
        preset_names: &[String],
    ) -> Vec<Result<RenderResult, String>> {
        let presets: Vec<ViewportPreset> = if preset_names.is_empty() {
            vec![
                find_preset("desktop").unwrap(),
                find_preset("mobile").unwrap(),
            ]
        } else {
            preset_names
                .iter()
                .filter_map(|n| find_preset(n))
                .collect()
        };

        let mut results = Vec::new();
        for preset in &presets {
            results.push(self.render_single(html, preset).await);
        }
        results
    }

    /// serve a cached screenshot file
    pub async fn get_cached(&self, id: &str) -> Option<Vec<u8>> {
        // sanitize id to prevent path traversal
        if id.contains("..") || id.contains('/') || id.contains('\\') {
            return None;
        }
        let path = self.cache_dir.join(id);
        tokio::fs::read(&path).await.ok()
    }
}

fn cache_hash(html: &str, preset: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(html.as_bytes());
    hasher.update(preset.as_bytes());
    hex::encode(&hasher.finalize()[..12])
}

/// preprocess html to simulate different email clients
fn preprocess_html(html: &str, preset: &ViewportPreset) -> String {
    let mut result = html.to_string();

    // strip <style> tags for gmail simulation
    if preset.strip_style_tags {
        // simple regex-free approach: remove everything between <style and </style>
        while let Some(start) = result.find("<style") {
            if let Some(end) = result[start..].find("</style>") {
                result = format!("{}{}", &result[..start], &result[start + end + 8..]);
            } else {
                break;
            }
        }
    }

    // wrap in a full HTML document with viewport meta
    let css_inject = preset.inject_css.as_deref().unwrap_or("");
    format!(
        r#"<!DOCTYPE html>
<html><head>
<meta charset="utf-8">
<meta name="viewport" content="width={w}, initial-scale=1">
<style>
body {{ margin: 0; padding: 16px; background: #fff; }}
{css_inject}
</style>
</head><body>{result}</body></html>"#,
        w = preset.width,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hash_deterministic() {
        let h1 = cache_hash("<h1>test</h1>", "desktop");
        let h2 = cache_hash("<h1>test</h1>", "desktop");
        assert_eq!(h1, h2);
    }

    #[test]
    fn cache_hash_varies_by_preset() {
        let h1 = cache_hash("<h1>test</h1>", "desktop");
        let h2 = cache_hash("<h1>test</h1>", "mobile");
        assert_ne!(h1, h2);
    }

    #[test]
    fn preprocess_strips_style_tags() {
        let preset = ViewportPreset {
            name: "gmail".into(),
            width: 660,
            height: 900,
            device_scale_factor: 1.0,
            is_mobile: false,
            inject_css: None,
            strip_style_tags: true,
        };
        let html = "<style>body{color:red}</style><p>hello</p>";
        let result = preprocess_html(html, &preset);
        assert!(!result.contains("color:red"));
        assert!(result.contains("<p>hello</p>"));
    }

    #[test]
    fn preprocess_injects_css() {
        let preset = ViewportPreset {
            name: "outlook".into(),
            width: 660,
            height: 900,
            device_scale_factor: 1.0,
            is_mobile: false,
            inject_css: Some("body { font-family: Calibri; }".into()),
            strip_style_tags: false,
        };
        let result = preprocess_html("<p>test</p>", &preset);
        assert!(result.contains("font-family: Calibri"));
    }

    #[test]
    fn default_presets_has_four() {
        assert_eq!(default_presets().len(), 4);
    }

    #[test]
    fn find_preset_works() {
        assert!(find_preset("desktop").is_some());
        assert!(find_preset("mobile").is_some());
        assert!(find_preset("gmail").is_some());
        assert!(find_preset("outlook").is_some());
        assert!(find_preset("nonexistent").is_none());
    }
}
