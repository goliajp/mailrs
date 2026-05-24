//! Tests for `render_preview` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

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
fn extract_body_strips_html_wrapper() {
    let html = r#"<html><head><style>.x{color:red}</style></head><body><p>hello</p></body></html>"#;
    let result = extract_body_content(html);
    assert!(result.contains("<p>hello</p>"));
    assert!(result.contains(".x{color:red}"));
    assert!(!result.contains("<html>"));
    assert!(!result.contains("<body>"));
}

#[test]
fn extract_body_passthrough_fragment() {
    let html = "<p>just a fragment</p>";
    let result = extract_body_content(html);
    assert_eq!(result, html);
}

#[test]
fn restore_proxy_urls_converts_back() {
    let html = r#"<img src="/api/proxy/image?url=https%3A%2F%2Fexample.com%2Fimg.png&token=abc123">"#;
    let result = restore_proxy_urls(html);
    assert!(result.contains(r#"src="https://example.com/img.png""#));
    assert!(!result.contains("/api/proxy/image"));
}

#[test]
fn restore_proxy_urls_no_token() {
    let html = r#"<img src="/api/proxy/image?url=https%3A%2F%2Fexample.com%2Fimg.png">"#;
    let result = restore_proxy_urls(html);
    assert!(result.contains(r#"src="https://example.com/img.png""#));
}

#[test]
fn preprocess_full_email_document() {
    let preset = ViewportPreset {
        name: "outlook".into(),
        width: 660,
        height: 900,
        device_scale_factor: 1.0,
        is_mobile: false,
        inject_css: Some("body { font-family: Calibri; }".into()),
        strip_style_tags: false,
    };
    let html = r#"<!DOCTYPE html><html><head><style>.email{padding:10px}</style></head><body><p>content</p></body></html>"#;
    let result = preprocess_html(html, &preset);
    // should not have nested <html> tags
    assert_eq!(result.matches("<html>").count(), 1);
    assert_eq!(result.matches("<body>").count(), 1);
    // should preserve email styles and inject preset css
    assert!(result.contains(".email{padding:10px}"));
    assert!(result.contains("font-family: Calibri"));
    assert!(result.contains("<p>content</p>"));
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
