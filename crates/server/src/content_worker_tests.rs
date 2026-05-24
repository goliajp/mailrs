//! Tests for `content_worker` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn default_ocr_langs_includes_english() {
    assert!(DEFAULT_OCR_LANGS.contains("eng"));
}

#[test]
fn default_ocr_langs_includes_chinese() {
    assert!(DEFAULT_OCR_LANGS.contains("chi_sim"));
}

#[test]
fn default_ocr_langs_includes_japanese() {
    assert!(DEFAULT_OCR_LANGS.contains("jpn"));
}

#[test]
#[allow(clippy::assertions_on_constants)]
fn batch_size_reasonable() {
    assert!(BATCH_SIZE > 0 && BATCH_SIZE <= 100);
}

#[test]
fn poll_interval_reasonable() {
    assert!(POLL_INTERVAL.as_secs() >= 10);
    assert!(POLL_INTERVAL.as_secs() <= 300);
}
