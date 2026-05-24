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
        "image/webp" => {
            data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP"
        }
        "image/bmp" => data.starts_with(b"BM"),
        "image/tiff" => {
            data.starts_with(b"II*\x00") || data.starts_with(b"MM\x00*")
        }
        // svg is XML text — allow only if it starts with expected XML/SVG markers
        // but we explicitly block svg to prevent XSS via inline SVG scripts
        "image/svg+xml" => false,
        _ => false,
    }
}

/// validate an inline image upload
pub(crate) fn validate_inline_upload(
    data: &[u8],
    content_type: &str,
) -> Result<(), String> {
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
        parts.push_str(&format!(
            "Content-Type: {}\r\n",
            img.content_type
        ));
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
#[path = "inline_image_tests.rs"]
mod tests;
