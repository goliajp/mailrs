//! Running the detector over a whole image

use image::RgbImage;
use rten::{Model, RunOptions};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use crate::OcrError;
use crate::db::{ProbMap, boxes};
use crate::geom::Pt;
use crate::prep::{det_size, pad_vertically, resize, within_bounds, write_bgr};

const MIN_SIDE: u32 = 30;
const MAX_SIDE: u32 = 2000;
const DET_LIMIT: u32 = 736;
/// Boxes whose tops are closer than this share a line in reading order.
const SAME_LINE: f32 = 10.0;

/// What detection saw: the image the boxes refer to (bounded and padded
/// copy of the input) and the text quads on it, in reading order.
pub(crate) struct Detected {
    pub img: RgbImage,
    pub quads: Vec<[Pt; 4]>,
}

pub(crate) fn detect(
    model: &Model,
    img: &RgbImage,
    opts: &RunOptions,
) -> Result<Detected, OcrError> {
    let (bounded, _, _) = within_bounds(img, MIN_SIDE, MAX_SIDE);
    let (padded, _) = pad_vertically(&bounded, 8.0, MIN_SIDE);
    let (pw, ph) = padded.dimensions();
    let (dw, dh) = det_size(pw, ph, DET_LIMIT);
    let mut input = NdTensor::zeros([1, 3, dh as usize, dw as usize]);
    write_bgr(&mut input, 0, &resize(&padded, dw, dh));
    let out: NdTensor<f32, 4> = model
        .run_one(input.view().into(), Some(opts.clone()))
        .map_err(|e| OcrError::Run(e.to_string()))?
        .try_into()
        .map_err(|_| OcrError::Model("detection output is not a 4-d float tensor"))?;
    let (mh, mw) = (out.size(2), out.size(3));
    let p = out.to_vec();
    let map = ProbMap {
        w: mw,
        h: mh,
        p: &p,
    };
    let mut found: Vec<[Pt; 4]> = boxes(&map, pw, ph).into_iter().map(|(q, _)| q).collect();
    reading_order(&mut found);
    Ok(Detected {
        img: padded,
        quads: found,
    })
}

/// Top to bottom; boxes whose tops lie within [`SAME_LINE`] of the previous
/// one form a line, read left to right.
fn reading_order(quads: &mut [[Pt; 4]]) {
    quads.sort_by(|a, b| a[0].y.total_cmp(&b[0].y));
    let mut line = 0u32;
    let mut keyed: Vec<(u32, f32, [Pt; 4])> = Vec::with_capacity(quads.len());
    for (i, q) in quads.iter().enumerate() {
        if i > 0 && q[0].y - quads[i - 1][0].y >= SAME_LINE {
            line += 1;
        }
        keyed.push((line, q[0].x, *q));
    }
    keyed.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (dst, (_, _, q)) in quads.iter_mut().zip(keyed) {
        *dst = q;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(x: f32, y: f32) -> [Pt; 4] {
        let p = |x, y| Pt { x, y };
        [p(x, y), p(x + 10.0, y), p(x + 10.0, y + 5.0), p(x, y + 5.0)]
    }

    #[test]
    fn boxes_on_one_line_read_left_to_right() {
        let mut q = vec![quad(50.0, 103.0), quad(5.0, 100.0), quad(30.0, 40.0)];
        reading_order(&mut q);
        let xs: Vec<f32> = q.iter().map(|q| q[0].x).collect();
        assert_eq!(xs, vec![30.0, 5.0, 50.0]);
    }
}
