//! inline image upload, serving, and CID conversion for outbound emails.
//!
//! flow:
//! 1. user pastes/drops image in editor → POST /api/mail/inline-upload
//! 2. server stores in {maildir_root}/inline/{user_hash}/{id}.{ext}
//! 3. editor inserts <img src="/api/mail/inline/{id}">
//! 4. on send: scan HTML for inline URLs → read files → replace with CID → multipart/related

use std::path::{Path, PathBuf};

use base64::Engine;
use rand_core::RngCore;
use sha2::{Digest, Sha256};

/// Maximum number of inline images per email (mailrs UI limit, not
/// a Sieve/RFC concept). Moved here in 2026-05-24 when
/// `content_extract` was extracted as the `mailrs-attachment-extract`
/// stone — those constants weren't generic to attachment extraction,
/// they're specific to mailrs's inline-image upload flow.
const MAX_INLINE_IMAGES: usize = 20;

/// Maximum single inline image size in bytes (10 MiB).
const MAX_INLINE_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// validate that an inline image ID is safe to use in filesystem paths.
///
/// IDs must be strictly alphanumeric + underscore. This prevents path traversal
/// even if the serve handler's manual checks are bypassed.
pub(crate) fn is_valid_inline_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.starts_with("img_")
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// generate a storage ID for an inline image
pub(crate) fn generate_inline_id() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let rand = rand_core::OsRng.next_u32();
    format!("img_{ts}_{rand:08x}")
}

/// derive user-specific subdirectory from email address
pub(crate) fn user_hash(address: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(address.as_bytes());
    let hash = hasher.finalize();
    hex::encode(&hash[..8])
}

/// resolve the storage path for an inline image
pub(crate) fn inline_path(maildir_root: &str, address: &str, id: &str, ext: &str) -> PathBuf {
    let hash = user_hash(address);
    Path::new(maildir_root)
        .join("inline")
        .join(hash)
        .join(format!("{id}.{ext}"))
}

/// extract file extension from content type
pub(crate) fn ext_from_content_type(content_type: &str) -> &str {
    match content_type {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/tiff" => "tiff",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        _ => "bin",
    }
}

/// verify that file bytes match the declared content type via magic bytes.
///
/// this prevents attackers from uploading HTML/SVG/scripts while claiming
/// an innocuous image content type.
fn verify_magic_bytes(data: &[u8], content_type: &str) -> bool {
    match content_type {
        "image/png" => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" | "image/jpg" => data.starts_with(b"\xFF\xD8\xFF"),
        "image/gif" => data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"),
        "image/webp" => data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP",
        "image/bmp" => data.starts_with(b"BM"),
        "image/tiff" => data.starts_with(b"II*\x00") || data.starts_with(b"MM\x00*"),
        // svg is XML text — allow only if it starts with expected XML/SVG markers
        // but we explicitly block svg to prevent XSS via inline SVG scripts
        "image/svg+xml" => false,
        _ => false,
    }
}

/// validate an inline image upload
pub(crate) fn validate_inline_upload(data: &[u8], content_type: &str) -> Result<(), String> {
    if data.len() > MAX_INLINE_IMAGE_SIZE {
        return Err(format!(
            "image too large ({} bytes, max {})",
            data.len(),
            MAX_INLINE_IMAGE_SIZE
        ));
    }
    if !content_type.starts_with("image/") {
        return Err(format!("not an image: {content_type}"));
    }
    // block svg — may contain embedded scripts that execute in browser context
    if content_type == "image/svg+xml" {
        return Err("svg uploads are not permitted".into());
    }
    // verify file bytes match the claimed content type
    if !verify_magic_bytes(data, content_type) {
        return Err(format!(
            "file content does not match content type: {content_type}"
        ));
    }
    Ok(())
}

/// an inline image extracted from HTML, ready to be embedded as CID
#[derive(Debug, Clone)]
pub(crate) struct InlineImage {
    pub id: String,
    pub content_type: String,
    pub data: Vec<u8>,
    pub cid: String,
}

/// scan HTML for inline image URLs matching `/api/mail/inline/{id}`
/// and return the list of IDs found
pub(crate) fn find_inline_urls(html: &str) -> Vec<String> {
    let prefix = "/api/mail/inline/";
    let mut ids = Vec::new();
    let mut search_from = 0;

    while let Some(pos) = html[search_from..].find(prefix) {
        let abs_pos = search_from + pos + prefix.len();
        // extract ID (alphanumeric + underscore until quote or space)
        let id_end = html[abs_pos..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|i| abs_pos + i)
            .unwrap_or(html.len());
        let id = &html[abs_pos..id_end];
        if !id.is_empty() && !ids.contains(&id.to_string()) {
            ids.push(id.to_string());
        }
        search_from = id_end;
    }

    if ids.len() > MAX_INLINE_IMAGES {
        ids.truncate(MAX_INLINE_IMAGES);
    }

    ids
}

/// replace inline URLs in HTML with CID references
pub(crate) fn replace_inline_urls_with_cid(html: &str, images: &[InlineImage]) -> String {
    let mut result = html.to_string();
    for img in images {
        let url = format!("/api/mail/inline/{}", img.id);
        let cid_ref = format!("cid:{}", img.cid);
        result = result.replace(&url, &cid_ref);
    }
    result
}

/// build the inline image MIME parts for multipart/related
pub(crate) fn build_inline_parts(images: &[InlineImage], boundary: &str) -> String {
    let mut parts = String::new();
    for img in images {
        parts.push_str(&format!("--{boundary}\r\n"));
        parts.push_str(&format!("Content-Type: {}\r\n", img.content_type));
        parts.push_str("Content-Transfer-Encoding: base64\r\n");
        parts.push_str(&format!("Content-ID: <{}>\r\n", img.cid));
        parts.push_str("Content-Disposition: inline\r\n\r\n");

        let encoded = base64::engine::general_purpose::STANDARD.encode(&img.data);
        for chunk in encoded.as_bytes().chunks(76) {
            parts.push_str(std::str::from_utf8(chunk).unwrap_or(""));
            parts.push_str("\r\n");
        }
    }
    parts
}

#[cfg(test)]
mod tests {
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
}
