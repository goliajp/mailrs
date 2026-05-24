//! Tests for `inline_image` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

// --- generate_inline_id ---

#[test]
fn inline_id_format() {
    let id = generate_inline_id();
    assert!(id.starts_with("img_"));
    assert!(id.len() > 12);
}

#[test]
fn inline_id_unique() {
    let a = generate_inline_id();
    let b = generate_inline_id();
    assert_ne!(a, b);
}

// --- user_hash ---

#[test]
fn user_hash_deterministic() {
    let a = user_hash("alice@example.com");
    let b = user_hash("alice@example.com");
    assert_eq!(a, b);
}

#[test]
fn user_hash_different_users() {
    let a = user_hash("alice@example.com");
    let b = user_hash("bob@example.com");
    assert_ne!(a, b);
}

#[test]
fn user_hash_length() {
    let h = user_hash("test@test.com");
    assert_eq!(h.len(), 16); // 8 bytes hex
}

// --- inline_path ---

#[test]
fn inline_path_structure() {
    let path = inline_path("/var/mail", "alice@example.com", "img_123_abc", "png");
    let path_str = path.to_str().unwrap();
    assert!(path_str.starts_with("/var/mail/inline/"));
    assert!(path_str.ends_with("/img_123_abc.png"));
}

// --- ext_from_content_type ---

#[test]
fn ext_png() {
    assert_eq!(ext_from_content_type("image/png"), "png");
}

#[test]
fn ext_jpeg() {
    assert_eq!(ext_from_content_type("image/jpeg"), "jpg");
}

#[test]
fn ext_webp() {
    assert_eq!(ext_from_content_type("image/webp"), "webp");
}

#[test]
fn ext_unknown() {
    assert_eq!(ext_from_content_type("application/octet-stream"), "bin");
}

// --- validate_inline_upload ---

#[test]
fn validate_ok() {
    // use real PNG magic bytes
    let png = b"\x89PNG\r\n\x1a\nfake";
    assert!(validate_inline_upload(png, "image/png").is_ok());
}

#[test]
fn validate_too_large() {
    let big = vec![0u8; MAX_INLINE_IMAGE_SIZE + 1];
    assert!(validate_inline_upload(&big, "image/png").is_err());
}

#[test]
fn validate_not_image() {
    assert!(validate_inline_upload(b"data", "text/plain").is_err());
}

// --- find_inline_urls ---

#[test]
fn find_no_urls() {
    let html = "<p>Hello world</p>";
    assert!(find_inline_urls(html).is_empty());
}

#[test]
fn find_single_url() {
    let html = r#"<img src="/api/mail/inline/img_123_abc">"#;
    let ids = find_inline_urls(html);
    assert_eq!(ids, vec!["img_123_abc"]);
}

#[test]
fn find_multiple_urls() {
    let html = r#"<img src="/api/mail/inline/img_1_aa"><img src="/api/mail/inline/img_2_bb">"#;
    let ids = find_inline_urls(html);
    assert_eq!(ids, vec!["img_1_aa", "img_2_bb"]);
}

#[test]
fn find_dedup_urls() {
    let html = r#"<img src="/api/mail/inline/img_1_aa"><img src="/api/mail/inline/img_1_aa">"#;
    let ids = find_inline_urls(html);
    assert_eq!(ids, vec!["img_1_aa"]);
}

#[test]
fn find_url_with_quotes() {
    let html = r#"src="/api/mail/inline/img_test_123""#;
    let ids = find_inline_urls(html);
    assert_eq!(ids, vec!["img_test_123"]);
}

#[test]
fn find_urls_max_limit() {
    // create HTML with 25 inline images (over MAX_INLINE_IMAGES=20)
    let html: String = (0..25)
        .map(|i| format!(r#"<img src="/api/mail/inline/img_{i}">"#))
        .collect();
    let ids = find_inline_urls(&html);
    assert_eq!(ids.len(), MAX_INLINE_IMAGES);
}

// --- replace_inline_urls_with_cid ---

#[test]
fn replace_single() {
    let html = r#"<img src="/api/mail/inline/img_1">"#;
    let images = vec![InlineImage {
        id: "img_1".to_string(),
        content_type: "image/png".to_string(),
        data: vec![],
        cid: "img_1@mail.example.com".to_string(),
    }];
    let result = replace_inline_urls_with_cid(html, &images);
    assert_eq!(result, r#"<img src="cid:img_1@mail.example.com">"#);
}

#[test]
fn replace_multiple() {
    let html = r#"<img src="/api/mail/inline/a"><img src="/api/mail/inline/b">"#;
    let images = vec![
        InlineImage {
            id: "a".to_string(),
            content_type: "image/png".to_string(),
            data: vec![],
            cid: "a@host".to_string(),
        },
        InlineImage {
            id: "b".to_string(),
            content_type: "image/jpeg".to_string(),
            data: vec![],
            cid: "b@host".to_string(),
        },
    ];
    let result = replace_inline_urls_with_cid(html, &images);
    assert!(result.contains("cid:a@host"));
    assert!(result.contains("cid:b@host"));
    assert!(!result.contains("/api/mail/inline/"));
}

#[test]
fn replace_preserves_non_inline() {
    let html = r#"<img src="https://example.com/img.png"><img src="/api/mail/inline/x">"#;
    let images = vec![InlineImage {
        id: "x".to_string(),
        content_type: "image/png".to_string(),
        data: vec![],
        cid: "x@host".to_string(),
    }];
    let result = replace_inline_urls_with_cid(html, &images);
    assert!(result.contains("https://example.com/img.png"));
    assert!(result.contains("cid:x@host"));
}

// --- build_inline_parts ---

#[test]
fn build_parts_structure() {
    let images = vec![InlineImage {
        id: "test".to_string(),
        content_type: "image/png".to_string(),
        data: vec![0x89, 0x50, 0x4E, 0x47], // PNG magic bytes
        cid: "test@host".to_string(),
    }];
    let parts = build_inline_parts(&images, "boundary123");
    assert!(parts.contains("--boundary123\r\n"));
    assert!(parts.contains("Content-Type: image/png\r\n"));
    assert!(parts.contains("Content-ID: <test@host>\r\n"));
    assert!(parts.contains("Content-Disposition: inline\r\n"));
    assert!(parts.contains("Content-Transfer-Encoding: base64\r\n"));
}

#[test]
fn build_parts_empty() {
    let parts = build_inline_parts(&[], "boundary");
    assert!(parts.is_empty());
}

// --- is_valid_inline_id ---

#[test]
fn valid_id_generated() {
    let id = generate_inline_id();
    assert!(is_valid_inline_id(&id));
}

#[test]
fn valid_id_known_good() {
    assert!(is_valid_inline_id("img_1234567890_deadbeef"));
}

#[test]
fn invalid_id_path_traversal() {
    assert!(!is_valid_inline_id("../../../etc/passwd"));
    assert!(!is_valid_inline_id("img_1/../secret"));
    assert!(!is_valid_inline_id("img_1/../../etc"));
}

#[test]
fn invalid_id_missing_prefix() {
    // must start with "img_"
    assert!(!is_valid_inline_id("photo_123"));
    assert!(!is_valid_inline_id("1234567890"));
}

#[test]
fn invalid_id_empty() {
    assert!(!is_valid_inline_id(""));
}

#[test]
fn invalid_id_too_long() {
    let long = format!("img_{}", "a".repeat(100));
    assert!(!is_valid_inline_id(&long));
}

#[test]
fn invalid_id_special_chars() {
    assert!(!is_valid_inline_id("img_1.jpg"));
    assert!(!is_valid_inline_id("img_1 2"));
    assert!(!is_valid_inline_id("img_1\x00"));
}

// --- verify_magic_bytes / validate_inline_upload ---

#[test]
fn validate_png_correct_magic() {
    let png_magic = b"\x89PNG\r\n\x1a\nfake data";
    assert!(validate_inline_upload(png_magic, "image/png").is_ok());
}

#[test]
fn validate_png_wrong_magic_rejected() {
    // claims to be PNG but starts with JPEG magic bytes
    let jpg_magic = b"\xFF\xD8\xFF\xE0fake";
    assert!(validate_inline_upload(jpg_magic, "image/png").is_err());
}

#[test]
fn validate_jpeg_correct_magic() {
    let jpg = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
    assert!(validate_inline_upload(jpg, "image/jpeg").is_ok());
}

#[test]
fn validate_gif_correct_magic() {
    let gif = b"GIF89afake";
    assert!(validate_inline_upload(gif, "image/gif").is_ok());
}

#[test]
fn validate_svg_always_rejected() {
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    assert!(validate_inline_upload(svg, "image/svg+xml").is_err());
}

#[test]
fn validate_html_disguised_as_png_rejected() {
    let html = b"<html><script>alert(1)</script></html>";
    assert!(validate_inline_upload(html, "image/png").is_err());
}

#[test]
fn validate_webp_correct_magic() {
    let mut webp = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
    webp.extend_from_slice(b"VP8 fake");
    assert!(validate_inline_upload(&webp, "image/webp").is_ok());
}
