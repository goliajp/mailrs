//! Text-line recognition: strips in, characters out, through a CTC model
//! whose alphabet ships inside the ONNX file

use image::RgbImage;
use rten::{Model, RunOptions};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use crate::OcrError;
use crate::prep::{resize, write_bgr};

const HEIGHT: u32 = 48;
/// Narrowest input the model is fed, as a width-to-height ratio.
const MIN_RATIO: f32 = 320.0 / 48.0;
const BATCH: usize = 6;

pub(crate) struct Recognizer {
    model: Model,
    /// Index 0 is the CTC blank, the last entry a space.
    alphabet: Vec<String>,
}

impl Recognizer {
    pub(crate) fn load(bytes: Vec<u8>) -> Result<Recognizer, OcrError> {
        let chars = crate::onnx_meta::metadata_value(&bytes, "character")
            .ok_or(OcrError::Model("recognition model carries no alphabet"))?;
        let mut alphabet = vec![String::new()];
        alphabet.extend(chars.split('\n').map(str::to_string));
        alphabet.push(" ".to_string());
        let model = Model::load(bytes).map_err(|e| OcrError::Load(e.to_string()))?;
        Ok(Recognizer { model, alphabet })
    }

    /// Text and mean confidence of each strip, in input order.
    pub(crate) fn read(
        &self,
        strips: &[RgbImage],
        opts: &RunOptions,
    ) -> Result<Vec<(String, f32)>, OcrError> {
        let ratio = |s: &RgbImage| s.width() as f32 / s.height() as f32;
        let mut order: Vec<usize> = (0..strips.len()).collect();
        // similar widths share a batch, so little of it is padding
        order.sort_by(|&a, &b| ratio(&strips[a]).total_cmp(&ratio(&strips[b])));
        let mut out = vec![(String::new(), 0.0); strips.len()];
        for chunk in order.chunks(BATCH) {
            let widest = chunk
                .iter()
                .map(|&i| ratio(&strips[i]))
                .fold(MIN_RATIO, f32::max);
            let w = (HEIGHT as f32 * widest) as u32;
            let mut input = NdTensor::zeros([chunk.len(), 3, HEIGHT as usize, w as usize]);
            for (n, &i) in chunk.iter().enumerate() {
                let fit = ((HEIGHT as f32 * ratio(&strips[i])).ceil() as u32).clamp(1, w);
                write_bgr(&mut input, n, &resize(&strips[i], fit, HEIGHT));
            }
            let probs: NdTensor<f32, 3> = self
                .model
                .run_one(input.view().into(), Some(opts.clone()))
                .map_err(|e| OcrError::Run(e.to_string()))?
                .try_into()
                .map_err(|_| OcrError::Model("recognition output is not a 3-d float tensor"))?;
            if probs.size(2) != self.alphabet.len() {
                return Err(OcrError::Model(
                    "recognition output does not match the alphabet",
                ));
            }
            for (n, &i) in chunk.iter().enumerate() {
                out[i] = self.decode(&probs, n);
            }
        }
        Ok(out)
    }

    /// Greedy CTC: best class per step, repeats collapsed, blanks dropped.
    fn decode(&self, probs: &NdTensor<f32, 3>, n: usize) -> (String, f32) {
        let mut text = String::new();
        let mut confs = Vec::new();
        let mut prev = usize::MAX;
        for t in 0..probs.size(1) {
            let (mut best, mut p) = (0usize, f32::MIN);
            for c in 0..probs.size(2) {
                let v = probs[[n, t, c]];
                if v > p {
                    (best, p) = (c, v);
                }
            }
            if best != 0 && best != prev {
                text.push_str(&self.alphabet[best]);
                confs.push(p);
            }
            prev = best;
        }
        let conf = match confs.len() {
            0 => 0.0,
            k => confs.iter().sum::<f32>() / k as f32,
        };
        (text, conf)
    }
}
