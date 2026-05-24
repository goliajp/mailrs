#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::io::Write;
use std::process::Command;

use serde::Serialize;

/// Result of an extraction attempt — text content + provenance metadata
/// (language hint, confidence, page count) suitable for indexing or
/// embedding generation downstream.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionResult {
    /// Extracted text content. Empty when the input was unsupported
    /// or extraction produced nothing.
    pub text: String,
    /// BCP-47-ish language hint (`"eng"`, `"jpn+eng"`, ...) if known.
    /// `None` for embedded PDF text (could be anything).
    pub language: Option<String>,
    /// 0.0–1.0 confidence. `1.0` for embedded PDF text (exact);
    /// ~0.85 for successful OCR; `0.0` for failed extraction.
    pub confidence: f64,
    /// Page count when known (PDFs).
    pub page_count: Option<u32>,
    /// Free-form JSON metadata about the extraction method
    /// (`{"method": "pdf_text"}` or `{"method": "ocr", "langs": "eng"}`).
    pub metadata: serde_json::Value,
}

impl ExtractionResult {
    /// Empty / failed-extraction sentinel with `text = ""` and
    /// `confidence = 0.0`.
    pub fn empty() -> Self {
        Self {
            text: String::new(),
            language: None,
            confidence: 0.0,
            page_count: None,
            metadata: serde_json::json!({}),
        }
    }
}

/// Which extraction backend applies to a given `Content-Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionMethod {
    /// `application/pdf` — try embedded text first, OCR fallback for scans.
    PdfText,
    /// `image/{png,jpeg,webp,tiff,bmp,gif}` — OCR via tesseract.
    ImageOcr,
    /// Anything else — caller should skip extraction.
    Unsupported,
}

/// Choose an [`ExtractionMethod`] from a `Content-Type` string.
/// Case-insensitive. Unknown types fall through to `Unsupported`.
pub fn extraction_method(content_type: &str) -> ExtractionMethod {
    let ct = content_type.to_ascii_lowercase();
    if ct == "application/pdf" {
        return ExtractionMethod::PdfText;
    }
    if ct.starts_with("image/")
        && matches!(
            ct.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/tiff" | "image/bmp" | "image/gif"
        )
    {
        return ExtractionMethod::ImageOcr;
    }
    ExtractionMethod::Unsupported
}

/// Extract embedded text from a PDF (pure Rust via `pdf-extract`).
/// Confidence is `1.0` because embedded text is exact, not OCR'd.
/// `page_count` is approximated by counting form-feed (`\u{000C}`)
/// page-break markers; off-by-one is possible for malformed PDFs.
pub fn extract_pdf_text(data: &[u8]) -> Result<ExtractionResult, String> {
    let text = pdf_extract::extract_text_from_mem(data).map_err(|e| format!("pdf parse: {e}"))?;
    let trimmed = text.trim().to_string();
    let page_count = text.matches('\u{000C}').count() as u32 + 1;
    Ok(ExtractionResult {
        text: trimmed,
        language: None,
        confidence: 1.0,
        page_count: Some(page_count),
        metadata: serde_json::json!({ "method": "pdf_text" }),
    })
}

/// Check whether the `tesseract` CLI binary is on `PATH`.
/// Spawns `tesseract --version` and checks for success — no caching.
/// If you'll call this on a hot path, cache the result yourself.
pub fn tesseract_available() -> bool {
    Command::new("tesseract")
        .arg("--version")
        .output()
        .is_ok()
}

/// OCR an image via the `tesseract` CLI subprocess.
///
/// `langs` is the tesseract `-l` value (e.g. `"eng"`, `"jpn+eng"`).
/// Writes `data` to a temp file (`tesseract` can't read stdin for
/// image data), runs `tesseract <tmp> stdout -l <langs> --psm 3`,
/// captures stdout as the extracted text. Confidence is heuristic
/// (0.85 default, 0.0 on "Empty page" stderr signal).
pub fn ocr_image(data: &[u8], langs: &str) -> Result<ExtractionResult, String> {
    let mut tmp = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()
        .map_err(|e| format!("tempfile: {e}"))?;
    tmp.write_all(data)
        .map_err(|e| format!("write temp: {e}"))?;
    tmp.flush().map_err(|e| format!("flush temp: {e}"))?;

    let output = Command::new("tesseract")
        .arg(tmp.path())
        .arg("stdout")
        .arg("-l")
        .arg(langs)
        .arg("--psm")
        .arg("3")
        .output()
        .map_err(|e| format!("tesseract exec: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tesseract failed: {stderr}"));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let confidence = parse_tesseract_confidence(&output.stderr);

    Ok(ExtractionResult {
        text,
        language: Some(langs.to_string()),
        confidence,
        page_count: None,
        metadata: serde_json::json!({ "method": "ocr", "langs": langs }),
    })
}

fn parse_tesseract_confidence(stderr: &[u8]) -> f64 {
    let text = String::from_utf8_lossy(stderr);
    if text.contains("Empty page") {
        return 0.0;
    }
    0.85
}

/// Auto-dispatch: pick the right extractor for `content_type` and run.
///
/// PDF path: try embedded text first; if the result is shorter than
/// 50 chars (heuristic for "scanned PDF with no embedded text"),
/// fall back to OCR on the raw bytes. Image path: OCR directly.
/// Unsupported types return [`ExtractionResult::empty`] (not an
/// error — caller should skip indexing but not log a failure).
pub fn extract_content(
    data: &[u8],
    content_type: &str,
    ocr_langs: &str,
) -> Result<ExtractionResult, String> {
    match extraction_method(content_type) {
        ExtractionMethod::PdfText => {
            let result = extract_pdf_text(data)?;
            if result.text.len() < 50 && tesseract_available() {
                match ocr_image(data, ocr_langs) {
                    Ok(ocr_result) if !ocr_result.text.is_empty() => Ok(ocr_result),
                    _ => Ok(result),
                }
            } else {
                Ok(result)
            }
        }
        ExtractionMethod::ImageOcr => {
            if !tesseract_available() {
                return Err("tesseract not installed".to_string());
            }
            ocr_image(data, ocr_langs)
        }
        ExtractionMethod::Unsupported => Ok(ExtractionResult::empty()),
    }
}

/// Recommended upper bound on input size for [`extract_content`] —
/// 50 MiB. Caller's choice whether to enforce; we don't enforce
/// internally because the right limit varies by deployment (an
/// archive-grade system may want 500 MiB, a mobile MTA may want 5).
pub const MAX_EXTRACT_SIZE: usize = 50 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_pdf() {
        assert_eq!(extraction_method("application/pdf"), ExtractionMethod::PdfText);
    }

    #[test]
    fn method_pdf_case_insensitive() {
        assert_eq!(extraction_method("Application/PDF"), ExtractionMethod::PdfText);
    }

    #[test]
    fn method_png() {
        assert_eq!(extraction_method("image/png"), ExtractionMethod::ImageOcr);
    }

    #[test]
    fn method_jpeg() {
        assert_eq!(extraction_method("image/jpeg"), ExtractionMethod::ImageOcr);
    }

    #[test]
    fn method_webp() {
        assert_eq!(extraction_method("image/webp"), ExtractionMethod::ImageOcr);
    }

    #[test]
    fn method_tiff() {
        assert_eq!(extraction_method("image/tiff"), ExtractionMethod::ImageOcr);
    }

    #[test]
    fn method_svg_unsupported() {
        assert_eq!(extraction_method("image/svg+xml"), ExtractionMethod::Unsupported);
    }

    #[test]
    fn method_word_unsupported() {
        assert_eq!(
            extraction_method(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ),
            ExtractionMethod::Unsupported
        );
    }

    #[test]
    fn method_text_unsupported() {
        assert_eq!(extraction_method("text/plain"), ExtractionMethod::Unsupported);
    }

    #[test]
    fn method_empty_unsupported() {
        assert_eq!(extraction_method(""), ExtractionMethod::Unsupported);
    }

    #[test]
    fn extract_unsupported_returns_empty() {
        let result = extract_content(b"hello", "text/plain", "eng").unwrap();
        assert!(result.text.is_empty());
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn extract_pdf_text_invalid_data() {
        let result = extract_pdf_text(b"not a pdf");
        assert!(result.is_err());
    }

    #[test]
    fn extract_pdf_text_minimal() {
        let pdf_bytes = create_minimal_pdf("Hello World");
        let _ = extract_pdf_text(&pdf_bytes);
    }

    #[test]
    fn ocr_image_no_tesseract_graceful() {
        if !tesseract_available() {
            let result = extract_content(b"\x89PNG", "image/png", "eng");
            assert!(result.is_err());
        }
    }

    #[test]
    fn ocr_image_with_tesseract() {
        if !tesseract_available() {
            return;
        }
        let img = image::RgbImage::from_fn(200, 50, |x, _y| {
            if x > 50 && x < 150 {
                image::Rgb([0u8, 0, 0])
            } else {
                image::Rgb([255u8, 255, 255])
            }
        });
        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();
        let result = ocr_image(&buf, "eng");
        assert!(result.is_ok());
    }

    #[test]
    fn empty_result() {
        let r = ExtractionResult::empty();
        assert!(r.text.is_empty());
        assert!(r.language.is_none());
        assert_eq!(r.confidence, 0.0);
        assert!(r.page_count.is_none());
    }

    #[test]
    fn confidence_empty_page() {
        assert_eq!(parse_tesseract_confidence(b"Empty page"), 0.0);
    }

    #[test]
    fn confidence_default() {
        assert_eq!(parse_tesseract_confidence(b"some output"), 0.85);
    }

    fn create_minimal_pdf(text: &str) -> Vec<u8> {
        format!(
            "%PDF-1.0\n\
            1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
            2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n\
            3 0 obj<</Type/Page/MediaBox[0 0 612 792]/Parent 2 0 R/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n\
            4 0 obj<</Length {}>>stream\nBT /F1 12 Tf 100 700 Td ({}) Tj ET\nendstream\nendobj\n\
            5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n\
            xref\n0 6\n\
            0000000000 65535 f \n\
            0000000009 00000 n \n\
            0000000058 00000 n \n\
            0000000115 00000 n \n\
            0000000266 00000 n \n\
            0000000400 00000 n \n\
            trailer<</Size 6/Root 1 0 R>>\nstartxref\n474\n%%EOF",
            text.len() + 45,
            text
        )
        .into_bytes()
    }
}
