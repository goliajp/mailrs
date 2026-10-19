//! Image preparation shared by detection and recognition: OpenCV-style
//! bilinear resizing and the BGR, [-1, 1] tensor layout the PaddleOCR
//! models were trained on

use image::{Rgb, RgbImage};
use rten_tensor::NdTensor;

/// Resize with OpenCV's `INTER_LINEAR` sampling (pixel centres, no
/// antialiasing), so crops and inputs match the reference pipeline.
pub(crate) fn resize(img: &RgbImage, w: u32, h: u32) -> RgbImage {
    let (sw, sh) = img.dimensions();
    let (fx, fy) = (sw as f32 / w as f32, sh as f32 / h as f32);
    let xs: Vec<(u32, u32, f32)> = (0..w).map(|x| taps(x, fx, sw)).collect();
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        let (y0, y1, wy) = taps(y, fy, sh);
        for (x, &(x0, x1, wx)) in xs.iter().enumerate() {
            let p = |xx, yy| img.get_pixel(xx, yy).0;
            let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
            let mut px = [0u8; 3];
            for k in 0..3 {
                let top = f32::from(a[k]) * (1.0 - wx) + f32::from(b[k]) * wx;
                let bot = f32::from(c[k]) * (1.0 - wx) + f32::from(d[k]) * wx;
                px[k] = (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
            out.put_pixel(x as u32, y, Rgb(px));
        }
    }
    out
}

fn taps(dst: u32, scale: f32, len: u32) -> (u32, u32, f32) {
    let s = ((dst as f32 + 0.5) * scale - 0.5).max(0.0);
    let i0 = (s.floor() as u32).min(len - 1);
    let i1 = (i0 + 1).min(len - 1);
    (i0, i1, s - i0 as f32)
}

/// `n` rounded to the nearest multiple of 32 (ties to even, as the
/// reference does), at least 32.
pub(crate) fn round32(n: f32) -> u32 {
    ((n / 32.0).round_ties_even() as u32).max(1) * 32
}

/// Keep the longest side within `max_side` and the shortest at least
/// `min_side`, both rounded to multiples of 32. Returns the image and the
/// factor that maps its coordinates back to the original.
pub(crate) fn within_bounds(img: &RgbImage, min_side: u32, max_side: u32) -> (RgbImage, f32, f32) {
    let (w, h) = img.dimensions();
    let mut out = img.clone();
    let (mut rx, mut ry) = (1.0f32, 1.0f32);
    if w.max(h) > max_side {
        let r = max_side as f32 / w.max(h) as f32;
        let (nw, nh) = (
            round32((w as f32 * r).trunc()),
            round32((h as f32 * r).trunc()),
        );
        out = resize(img, nw, nh);
        (rx, ry) = (w as f32 / nw as f32, h as f32 / nh as f32);
    }
    let (w2, h2) = out.dimensions();
    if w2.min(h2) < min_side {
        let r = min_side as f32 / w2.min(h2) as f32;
        let (nw, nh) = (
            round32((w2 as f32 * r).trunc()),
            round32((h2 as f32 * r).trunc()),
        );
        out = resize(&out, nw, nh);
        (rx, ry) = (w as f32 / nw as f32, h as f32 / nh as f32);
    }
    (out, rx, ry)
}

/// Black bands above and below a very wide or very short image, so a single
/// line of text is not stretched into nothing by the detector's resize.
/// Returns the padded image and the band height.
pub(crate) fn pad_vertically(img: &RgbImage, max_ratio: f32, min_height: u32) -> (RgbImage, u32) {
    let (w, h) = img.dimensions();
    if h > min_height && (w as f32 / h as f32) <= max_ratio {
        return (img.clone(), 0);
    }
    let target = ((w as f32 / max_ratio) as u32).max(min_height) * 2;
    let pad = target.abs_diff(h) / 2;
    let mut out = RgbImage::new(w, h + 2 * pad);
    image::imageops::replace(&mut out, img, 0, i64::from(pad));
    (out, pad)
}

/// Size the detector sees: the shortest side brought up to `limit` when
/// smaller, both sides rounded to multiples of 32.
pub(crate) fn det_size(w: u32, h: u32, limit: u32) -> (u32, u32) {
    let mut r = 1.0f32;
    if w.min(h) < limit {
        r = limit as f32 / w.min(h) as f32;
    }
    (
        round32((w as f32 * r).trunc()),
        round32((h as f32 * r).trunc()),
    )
}

/// Write `img` into `t[0, .., .., x0..]` as BGR scaled to [-1, 1].
pub(crate) fn write_bgr(t: &mut NdTensor<f32, 4>, n: usize, img: &RgbImage) {
    for (x, y, p) in img.enumerate_pixels() {
        for (c, k) in [2usize, 1, 0].into_iter().enumerate() {
            t[[n, c, y as usize, x as usize]] = f32::from(p.0[k]) / 127.5 - 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_to_the_same_size_changes_nothing() {
        let mut img = RgbImage::new(5, 3);
        img.put_pixel(2, 1, Rgb([200, 10, 30]));
        assert_eq!(resize(&img, 5, 3), img);
    }

    #[test]
    fn halving_averages_neighbours() {
        let mut img = RgbImage::new(2, 1);
        img.put_pixel(1, 0, Rgb([100, 100, 100]));
        assert_eq!(resize(&img, 1, 1).get_pixel(0, 0).0, [50, 50, 50]);
    }

    #[test]
    fn detector_size_brings_the_short_side_up() {
        assert_eq!(det_size(1654, 2338, 736), (1664, 2336));
        assert_eq!(det_size(1000, 800, 736), (992, 800));
        // 1104 / 32 = 34.5 rounds to the even 34
        assert_eq!(det_size(600, 400, 736), (1088, 736));
    }

    #[test]
    fn a_single_line_strip_gets_bands() {
        let (out, pad) = pad_vertically(&RgbImage::new(800, 40), 8.0, 30);
        // 800 / 8 = 100, doubled to 200: 80 above and 80 below
        assert_eq!(pad, 80);
        assert_eq!(out.height(), 200);
        let (same, none) = pad_vertically(&RgbImage::new(400, 300), 8.0, 30);
        assert_eq!((same.height(), none), (300, 0));
    }

    #[test]
    fn channels_go_in_as_bgr() {
        let mut img = RgbImage::new(1, 1);
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        let mut t = NdTensor::zeros([1, 3, 1, 1]);
        write_bgr(&mut t, 0, &img);
        assert_eq!((t[[0, 0, 0, 0]], t[[0, 2, 0, 0]]), (-1.0, 1.0));
    }
}
