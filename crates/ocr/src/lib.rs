#![doc = "Printed Chinese, Japanese and English text out of images and scanned \
PDF pages. PaddleOCR's detection and recognition models run on rten; the \
pipeline around them (resizing, box extraction, cropping, CTC decoding, page \
orientation) is implemented here in plain Rust."]
#![deny(missing_docs)]

use std::path::Path;
use std::sync::Arc;

use image::RgbImage;
use rten::{Model, RunOptions, ThreadPool};

mod crop;
mod db;
mod det;
mod geom;
pub mod input;
mod onnx_meta;
mod prep;
mod rec;

pub use geom::Pt;

/// Lines below this mean confidence are noise, not text.
const MIN_LINE_SCORE: f32 = 0.5;
/// Boxes sampled to decide which way up a page is.
const ORIENTATION_SAMPLE: usize = 8;
/// How much better a turned page has to read before it is turned.
const ORIENTATION_MARGIN: f32 = 0.1;

/// Why reading failed.
#[derive(Debug)]
pub enum OcrError {
    /// A model file could not be read or parsed.
    Load(String),
    /// A model file is not the kind this pipeline expects.
    Model(&'static str),
    /// Inference failed.
    Run(String),
    /// The input is not an image or PDF this crate can open, or exceeds
    /// a limit.
    Input(String),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::Load(e) => write!(f, "loading model: {e}"),
            OcrError::Model(e) => write!(f, "unexpected model: {e}"),
            OcrError::Run(e) => write!(f, "inference: {e}"),
            OcrError::Input(e) => write!(f, "input: {e}"),
        }
    }
}

impl std::error::Error for OcrError {}

/// One recognised line of text.
#[derive(Debug, Clone)]
pub struct Line {
    /// The characters read.
    pub text: String,
    /// Mean per-character confidence, 0 to 1.
    pub score: f32,
}

/// Detection and recognition models, loaded once and reused.
pub struct Engine {
    det: Model,
    rec: rec::Recognizer,
    opts: RunOptions,
}

impl Engine {
    /// Load `det.onnx` and `rec.onnx` from `dir`; inference uses at most
    /// `threads` threads.
    pub fn load(dir: &Path, threads: usize) -> Result<Engine, OcrError> {
        let read = |name: &str| {
            std::fs::read(dir.join(name)).map_err(|e| OcrError::Load(format!("{name}: {e}")))
        };
        let det = Model::load(read("det.onnx")?).map_err(|e| OcrError::Load(e.to_string()))?;
        let rec = rec::Recognizer::load(read("rec.onnx")?)?;
        let opts = RunOptions::default()
            .with_thread_pool(Some(Arc::new(ThreadPool::with_num_threads(threads.max(1)))));
        Ok(Engine { det, rec, opts })
    }

    /// Lines of text in `img`, in reading order. The page is turned upright
    /// first when it was scanned sideways or upside down.
    pub fn read(&self, img: &RgbImage) -> Result<Vec<Line>, OcrError> {
        let found = det::detect(&self.det, img, &self.opts)?;
        if found.quads.is_empty() {
            return Ok(Vec::new());
        }
        match self.best_turn(&found)? {
            0 => self.recognise(&found),
            turn => self.recognise(&det::detect(&self.det, &rotate(img, turn), &self.opts)?),
        }
    }

    fn recognise(&self, found: &det::Detected) -> Result<Vec<Line>, OcrError> {
        let strips: Vec<RgbImage> = found
            .quads
            .iter()
            .filter_map(|&q| crop::crop(&found.img, q))
            .collect();
        let read = self.rec.read(&strips, &self.opts)?;
        Ok(read
            .into_iter()
            .filter(|(text, score)| *score >= MIN_LINE_SCORE && !text.trim().is_empty())
            .map(|(text, score)| Line { text, score })
            .collect())
    }

    /// Clockwise quarter turns (0 to 3) that make the page read best: the
    /// largest boxes are read in all four orientations and the mean
    /// confidence decides.
    fn best_turn(&self, found: &det::Detected) -> Result<u8, OcrError> {
        let mut quads = found.quads.clone();
        quads.sort_by(|a, b| area(b).total_cmp(&area(a)));
        let strips: Vec<RgbImage> = quads
            .iter()
            .take(ORIENTATION_SAMPLE)
            .filter_map(|&q| crop::crop_as_is(&found.img, q))
            .collect();
        let mut scores = [0f32; 4];
        for (turn, score) in scores.iter_mut().enumerate() {
            let turned: Vec<RgbImage> = strips.iter().map(|s| rotate(s, turn as u8)).collect();
            let read = self.rec.read(&turned, &self.opts)?;
            *score = read.iter().map(|(_, s)| s).sum::<f32>() / read.len().max(1) as f32;
        }
        let mut best = 0usize;
        for turn in 1..4 {
            if scores[turn] > scores[best] {
                best = turn;
            }
        }
        if scores[best] > scores[0] + ORIENTATION_MARGIN {
            return Ok(best as u8);
        }
        Ok(0)
    }
}

fn area(q: &[Pt; 4]) -> f32 {
    db::dist(q[0], q[1]) * db::dist(q[0], q[3])
}

/// `img` turned clockwise by `turns` quarter turns.
fn rotate(img: &RgbImage, turns: u8) -> RgbImage {
    match turns % 4 {
        1 => image::imageops::rotate90(img),
        2 => image::imageops::rotate180(img),
        3 => image::imageops::rotate270(img),
        _ => img.clone(),
    }
}

/// The lines joined into one text, one per line.
pub fn text_of(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
