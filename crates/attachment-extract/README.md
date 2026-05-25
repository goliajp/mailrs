# mailrs-attachment-extract

[![Crates.io](https://img.shields.io/crates/v/mailrs-attachment-extract.svg)](https://crates.io/crates/mailrs-attachment-extract)
[![Docs.rs](https://docs.rs/mailrs-attachment-extract/badge.svg)](https://docs.rs/mailrs-attachment-extract)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](#license)

Extract text from email (or any) attachments — **PDF** via
`pdf-extract` (pure Rust embedded-text path) and **images** via
the **tesseract CLI** subprocess.

Designed for the case where you need plain-text from arbitrary
attachments — indexing, search, embedding generation, spam scoring,
LLM context. **No** linking against libtesseract C library; the
crate shells out to the `tesseract` binary so it works wherever
tesseract is installed and avoids the C-library bindings tax.

## Two-stage PDF fallback

PDFs come in two flavours:
1. **Real PDFs** with embedded text — extract via `pdf-extract` in
   ~1 ms, confidence `1.0`, exact.
2. **Scanned PDFs** with image pages only — embedded extraction
   returns near-empty text; fall back to OCR on the raw PDF bytes
   (tesseract can handle PDFs directly), confidence `~0.85`.

[`extract_content`] does the dispatch automatically — if embedded
text is < 50 chars (heuristic), it tries OCR as fallback.

## Quick start

```no_run
use mailrs_attachment_extract::{extract_content, extraction_method, ExtractionMethod};

let pdf_bytes: &[u8] = b"%PDF-1.0\n..."; // your PDF bytes

// Single auto-dispatch entrypoint.
let result = extract_content(pdf_bytes, "application/pdf", "eng").unwrap();
println!("text: {}", result.text);
println!("confidence: {}", result.confidence);

// Or check the method first if you want to skip unsupported types early.
if extraction_method("application/pdf") != ExtractionMethod::Unsupported {
    // …
}
```

## What's in the box

| Function | Purpose |
|---|---|
| `extract_content(data, content_type, ocr_langs)` | One-call auto-dispatch (PDF or image) |
| `extract_pdf_text(data)` | PDF embedded-text only |
| `ocr_image(data, langs)` | OCR via `tesseract` CLI |
| `extraction_method(content_type)` | Inspect which backend applies |
| `tesseract_available()` | Spawn-test for tesseract binary |
| `ExtractionResult` | text + language + confidence + page count + metadata |
| `MAX_EXTRACT_SIZE` | Recommended 50 MiB input cap (caller enforces) |

## Supported types

| Content-Type | Method |
|---|---|
| `application/pdf` | PDF text + OCR fallback |
| `image/png` | OCR |
| `image/jpeg` | OCR |
| `image/webp` | OCR |
| `image/tiff` | OCR |
| `image/bmp` | OCR |
| `image/gif` | OCR |
| anything else | unsupported (returns empty result) |

## Runtime requirements

- **tesseract** CLI for image OCR. Install via `brew install
  tesseract` / `apt install tesseract-ocr` / etc. Language packs
  optional but recommended (`tesseract-ocr-jpn`, etc.).
- **pdf-extract** is a pure-Rust dep, no system requirement.

If tesseract isn't installed, `extract_content` for image
content-types returns an `Err`. PDF extraction still works for
embedded-text PDFs.

<!-- AUDIT-FOOTER:BEGIN -->

## Stone audit (v3 cycle, 2026-05-25)

| Axis | Status |
|---|---|
| **doc** | ✅ clean (`cargo doc --no-deps -p mailrs-attachment-extract`) |
| **test** | line cov: 84.9% (`cargo llvm-cov -p mailrs-attachment-extract --summary-only`) |
| **bench** | ✅ 1 file(s) criterion + ✅ 1 gate(s) `perf_gate.rs` |
| **size** | release rlib: 128 KB |
| **fuzz** | ✅ 1 target(s) |
| **mem**  | dhat profile pending (v3.4 backlog) |

### Competitor comparisons

- Searched crates.io + competing impls: see PERFORMANCE.md or 'first-in-Rust' marker.

<!-- AUDIT-FOOTER:END -->

## License

Apache-2.0 OR MIT.
