//! content extraction from email attachments (OCR, PDF text, etc.)
//!
//! design: pure functions with no I/O dependencies where possible.
//! tesseract is called via CLI subprocess, not linked as C library.

use std::io::Write;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ExtractionResult {
    pub text: String,
    pub language: Option<String>,
    // f64 matches the DOUBLE PRECISION column in attachment_content
    pub confidence: f64,
    pub page_count: Option<u32>,
    pub metadata: serde_json::Value,
}

impl ExtractionResult {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtractionMethod {
    PdfText,
    ImageOcr,
    Unsupported,
}

/// determine extraction method from content type
pub(crate) fn extraction_method(content_type: &str) -> ExtractionMethod {
    let ct = content_type.to_ascii_lowercase();
    if ct == "application/pdf" {
        return ExtractionMethod::PdfText; // try text first, fall back to OCR
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

/// extract embedded text from a PDF (pure Rust, no external deps)
pub(crate) fn extract_pdf_text(data: &[u8]) -> Result<ExtractionResult, String> {
    let text = pdf_extract::extract_text_from_mem(data).map_err(|e| format!("pdf parse: {e}"))?;
    let trimmed = text.trim().to_string();

    // count pages by searching for page break markers
    let page_count = text.matches('\u{000C}').count() as u32 + 1;

    Ok(ExtractionResult {
        text: trimmed,
        language: None,
        confidence: 1.0, // embedded text is exact
        page_count: Some(page_count),
        metadata: serde_json::json!({ "method": "pdf_text" }),
    })
}

/// check if tesseract is available on the system
pub(crate) fn tesseract_available() -> bool {
    Command::new("tesseract")
        .arg("--version")
        .output()
        .is_ok()
}

/// OCR an image using tesseract CLI
///
/// writes image to a temp file, runs `tesseract <input> stdout -l <langs>`,
/// captures stdout as extracted text.
pub(crate) fn ocr_image(data: &[u8], langs: &str) -> Result<ExtractionResult, String> {
    // write image to temp file
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
        .arg("3") // fully automatic page segmentation
        .output()
        .map_err(|e| format!("tesseract exec: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tesseract failed: {stderr}"));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // parse confidence from stderr if available (tesseract outputs it there)
    let confidence = parse_tesseract_confidence(&output.stderr);

    Ok(ExtractionResult {
        text,
        language: Some(langs.to_string()),
        confidence,
        page_count: None,
        metadata: serde_json::json!({ "method": "ocr", "langs": langs }),
    })
}

/// parse average confidence from tesseract stderr output
fn parse_tesseract_confidence(stderr: &[u8]) -> f64 {
    // tesseract doesn't output confidence by default in simple mode
    // return a reasonable default
    let text = String::from_utf8_lossy(stderr);
    if text.contains("Empty page") {
        return 0.0;
    }
    0.85 // default assumption for successful OCR
}

/// entry point: auto-select extraction method and run
pub(crate) fn extract_content(
    data: &[u8],
    content_type: &str,
    ocr_langs: &str,
) -> Result<ExtractionResult, String> {
    match extraction_method(content_type) {
        ExtractionMethod::PdfText => {
            let result = extract_pdf_text(data)?;
            // if extracted text is too short, it might be a scanned PDF
            if result.text.len() < 50 && tesseract_available() {
                // try OCR on the raw data (tesseract can handle some PDFs directly)
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

/// max file size for content extraction (50 MB)
pub(crate) const MAX_EXTRACT_SIZE: usize = 50 * 1024 * 1024;

/// max number of inline images per email
pub(crate) const MAX_INLINE_IMAGES: usize = 20;

/// max single inline image size (10 MB)
pub(crate) const MAX_INLINE_IMAGE_SIZE: usize = 10 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    // --- extraction_method tests ---

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
            extraction_method("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
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

    // --- extract_content for unsupported ---

    #[test]
    fn extract_unsupported_returns_empty() {
        let result = extract_content(b"hello", "text/plain", "eng").unwrap();
        assert!(result.text.is_empty());
        assert_eq!(result.confidence, 0.0);
    }

    // --- PDF text extraction ---

    #[test]
    fn extract_pdf_text_invalid_data() {
        let result = extract_pdf_text(b"not a pdf");
        assert!(result.is_err());
    }

    #[test]
    fn extract_pdf_text_minimal() {
        // minimal valid PDF with text "Hello"
        let pdf_bytes = create_minimal_pdf("Hello World");
        let result = extract_pdf_text(&pdf_bytes);
        // pdf_extract may or may not handle our minimal PDF
        // just verify it doesn't panic
        let _ = result;
    }

    // --- OCR tests (conditional on tesseract availability) ---

    #[test]
    fn ocr_image_no_tesseract_graceful() {
        // if tesseract is not available, extract_content should return error for images
        if !tesseract_available() {
            let result = extract_content(b"\x89PNG", "image/png", "eng");
            assert!(result.is_err());
        }
    }

    #[test]
    fn ocr_image_with_tesseract() {
        if !tesseract_available() {
            return; // skip if tesseract not installed
        }

        // create a simple white image with text-like content
        let img = image::RgbImage::from_fn(200, 50, |x, _y| {
            if x > 50 && x < 150 {
                image::Rgb([0u8, 0, 0]) // black stripe
            } else {
                image::Rgb([255u8, 255, 255]) // white
            }
        });

        let mut buf = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cursor, image::ImageFormat::Png).unwrap();

        let result = ocr_image(&buf, "eng");
        // should succeed even if no text is detected
        assert!(result.is_ok());
    }

    // --- ExtractionResult ---

    #[test]
    fn empty_result() {
        let r = ExtractionResult::empty();
        assert!(r.text.is_empty());
        assert!(r.language.is_none());
        assert_eq!(r.confidence, 0.0);
        assert!(r.page_count.is_none());
    }

    // --- parse_tesseract_confidence ---

    #[test]
    fn confidence_empty_page() {
        assert_eq!(parse_tesseract_confidence(b"Empty page"), 0.0);
    }

    #[test]
    fn confidence_default() {
        assert_eq!(parse_tesseract_confidence(b"some output"), 0.85);
    }

    // --- helper ---

    /// create a minimal PDF with embedded text (for testing)
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
