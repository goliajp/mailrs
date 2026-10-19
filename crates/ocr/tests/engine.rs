//! The pipeline on real models: run `scripts/fetch-ocr-models.sh` first, or
//! point `MAILRS_OCR_MODEL_DIR` at a directory holding them.

use std::path::PathBuf;

use image::RgbImage;
use mailrs_ocr::{Engine, input};

const MIXED: [&str; 5] = [
    "請求書InvoiceNo.2026-1019",
    "合計金額¥95,550(税込)",
    "東京都港区赤坂二丁目",
    "增值税专用发票总金额",
    "PleaseremitbyOctober31",
];

fn models() -> PathBuf {
    let dir = std::env::var_os("MAILRS_OCR_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ocr-models")
        });
    assert!(
        dir.join("rec.onnx").is_file(),
        "no OCR models in {}: run scripts/fetch-ocr-models.sh",
        dir.display()
    );
    dir
}

fn fixture(name: &str) -> RgbImage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    input::decode_image(&std::fs::read(path).unwrap(), 4000, 1 << 28).unwrap()
}

/// Lines with spacing taken out and full-width ASCII folded to half width:
/// where the spaces go and how wide a bracket is are the recogniser's
/// judgement, not what the page says.
fn read(engine: &Engine, img: &RgbImage) -> Vec<String> {
    engine
        .read(img)
        .unwrap()
        .into_iter()
        .map(|l| {
            l.text
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(half_width)
                .collect()
        })
        .collect()
}

fn half_width(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c,
    }
}

#[test]
fn reads_japanese_chinese_and_english_on_one_page() {
    let engine = Engine::load(&models(), 2).unwrap();
    assert_eq!(read(&engine, &fixture("mixed.png")), MIXED);
}

/// A page scanned sideways or upside down reads the same once turned.
#[test]
fn a_turned_page_is_read_upright() {
    let engine = Engine::load(&models(), 2).unwrap();
    let page = fixture("mixed.png");
    for (turn, img) in [
        (90, image::imageops::rotate90(&page)),
        (180, image::imageops::rotate180(&page)),
        (270, image::imageops::rotate270(&page)),
    ] {
        assert_eq!(read(&engine, &img), MIXED, "turned {turn}");
    }
}

/// Columns of vertical Japanese, read right to left.
#[test]
fn reads_vertical_japanese_in_column_order() {
    let engine = Engine::load(&models(), 2).unwrap();
    assert_eq!(
        read(&engine, &fixture("vertical.png")),
        ["東京都港区赤坂", "株式会社ゴリア御中", "令和八年十月十九日"]
    );
}

#[test]
fn a_blank_page_has_no_lines() {
    let engine = Engine::load(&models(), 2).unwrap();
    assert!(
        engine
            .read(&RgbImage::from_pixel(800, 600, image::Rgb([255, 255, 255])))
            .unwrap()
            .is_empty()
    );
}

/// A scan: one page whose only content is a JPEG of the text.
#[test]
fn a_scanned_pdf_page_is_rendered_and_read() {
    let page = fixture("mixed.png");
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)
        .encode_image(&page)
        .unwrap();
    let pdf = scan_pdf(&jpeg, page.width(), page.height());
    let doc = input::PdfPages::open(pdf).unwrap();
    assert_eq!(doc.len(), 1);
    let img = doc.render(0, 200.0, 2000).unwrap();
    let engine = Engine::load(&models(), 2).unwrap();
    assert_eq!(read(&engine, &img), MIXED);
}

fn scan_pdf(jpeg: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (pw, ph) = (w as f32 * 72.0 / 150.0, h as f32 * 72.0 / 150.0);
    let content = format!("q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q");
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>")
            .into_bytes(),
    ];
    let mut img = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    img.extend_from_slice(jpeg);
    img.extend_from_slice(b"\nendstream");
    objs.push(img);
    objs.push(
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
    );
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}
