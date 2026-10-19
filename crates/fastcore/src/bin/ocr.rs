//! `mailrs-fastcore-ocr`: reads the text in one attachment, in a process of
//! its own.
//!
//! fastcore starts one of these per image or scanned PDF. The file came
//! from whoever sent the mail: a decoder or renderer that overflows its
//! stack, loops or balloons must cost this process, never the mail server,
//! so the limits below are set before a byte of input is read.
//!
//! Usage: `mailrs-fastcore-ocr image` or `mailrs-fastcore-ocr pdf 0,2,5`
//! (pages from 0). The attachment comes on stdin; one JSON object goes to
//! stdout. The models are read from `MAILRS_OCR_MODEL_DIR`.

use std::io::{Read, Write};
use std::path::PathBuf;

use rustix::process::{Resource, Rlimit, setpriority_process, setrlimit};

/// Address space. The resident peak measured over 60 real attachment
/// images was 540MB; address space runs higher than resident memory.
#[cfg(target_os = "linux")]
const MEMORY_LIMIT: u64 = 1536 * 1024 * 1024;
const CPU_SECONDS: u64 = 600;
/// Images smaller than this many pixels carry logos and icons, not text
/// worth reading.
const MIN_PIXELS: u64 = 300 * 300;
const MAX_IMAGE_SIDE: u32 = 10_000;
const MAX_DECODE_BYTES: u64 = 400 * 1024 * 1024;
const PDF_DPI: f32 = 200.0;
const MAX_SIDE: u32 = 2000;

#[derive(serde::Serialize, Default)]
struct Answer {
    text: String,
    too_small: bool,
}

fn main() {
    if let Err(e) = limit_memory()
        .and_then(|()| limit(Resource::Cpu, CPU_SECONDS))
        .and_then(|()| setpriority_process(None, 19))
    {
        fail(&format!("setting limits: {e}"));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(models) = std::env::var_os("MAILRS_OCR_MODEL_DIR").map(PathBuf::from) else {
        fail("MAILRS_OCR_MODEL_DIR is not set");
    };
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut input) {
        fail(&format!("reading stdin: {e}"));
    }
    let answer = match args.first().map(String::as_str) {
        Some("image") => image(&models, &input),
        Some("pdf") => pdf(
            &models,
            input,
            args.get(1).map(String::as_str).unwrap_or(""),
        ),
        _ => fail("usage: mailrs-fastcore-ocr image | pdf <pages>"),
    };
    match answer
        .map_err(|e| e.to_string())
        .and_then(|a| serde_json::to_vec(&a).map_err(|e| e.to_string()))
    {
        Ok(json) => {
            let _ = std::io::stdout().write_all(&json);
        }
        Err(e) => fail(&e),
    }
}

fn limit(r: Resource, v: u64) -> rustix::io::Result<()> {
    setrlimit(
        r,
        Rlimit {
            current: Some(v),
            maximum: Some(v),
        },
    )
}

/// Production runs on Linux. macOS refuses to lower the address-space
/// limit (EINVAL), so a development machine runs without it.
#[cfg(target_os = "linux")]
fn limit_memory() -> rustix::io::Result<()> {
    limit(Resource::As, MEMORY_LIMIT)
}

#[cfg(not(target_os = "linux"))]
fn limit_memory() -> rustix::io::Result<()> {
    Ok(())
}

fn fail(msg: &str) -> ! {
    eprintln!("mailrs-fastcore-ocr: {msg}");
    std::process::exit(1);
}

fn engine(models: &std::path::Path) -> Result<mailrs_ocr::Engine, mailrs_ocr::OcrError> {
    mailrs_ocr::Engine::load(models, 1)
}

fn image(models: &std::path::Path, bytes: &[u8]) -> Result<Answer, mailrs_ocr::OcrError> {
    let img = mailrs_ocr::input::decode_image(bytes, MAX_IMAGE_SIDE, MAX_DECODE_BYTES)?;
    if u64::from(img.width()) * u64::from(img.height()) < MIN_PIXELS {
        return Ok(Answer {
            too_small: true,
            ..Answer::default()
        });
    }
    let img = bounded(img);
    let lines = engine(models)?.read(&img)?;
    Ok(Answer {
        text: mailrs_ocr::text_of(&lines),
        too_small: false,
    })
}

/// Large photos are scaled down before recognition: the pipeline would
/// do it anyway, and the copy it would make is the largest allocation.
fn bounded(img: image::RgbImage) -> image::RgbImage {
    let (w, h) = img.dimensions();
    if w.max(h) <= MAX_SIDE {
        return img;
    }
    let r = MAX_SIDE as f32 / w.max(h) as f32;
    let (nw, nh) = (
        ((w as f32 * r) as u32).max(1),
        ((h as f32 * r) as u32).max(1),
    );
    image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
}

fn pdf(
    models: &std::path::Path,
    bytes: Vec<u8>,
    pages: &str,
) -> Result<Answer, mailrs_ocr::OcrError> {
    let doc = mailrs_ocr::input::PdfPages::open(bytes)?;
    let engine = engine(models)?;
    let mut texts = Vec::new();
    for page in pages
        .split(',')
        .filter_map(|p| p.trim().parse::<usize>().ok())
    {
        if page >= doc.len() {
            continue;
        }
        let img = doc.render(page, PDF_DPI, MAX_SIDE)?;
        let text = mailrs_ocr::text_of(&engine.read(&img)?);
        if !text.is_empty() {
            texts.push(text);
        }
    }
    Ok(Answer {
        text: texts.join("\n\n"),
        too_small: false,
    })
}
