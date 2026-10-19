//! Cutting a detected text quad out of the page as an upright strip

use image::{Rgb, RgbImage};

use crate::db::dist;
use crate::geom::{Pt, homography};

/// The quad (top-left, top-right, bottom-right, bottom-left) warped onto an
/// upright rectangle; edges outside the image repeat the border pixel.
/// A strip at least 1.5 times taller than wide is a vertical line of text
/// and is turned a quarter counter-clockwise, as the recogniser reads
/// left to right.
pub(crate) fn crop(img: &RgbImage, q: [Pt; 4]) -> Option<RgbImage> {
    let out = crop_as_is(img, q)?;
    if out.height() as f32 / out.width() as f32 >= 1.5 {
        return Some(image::imageops::rotate270(&out));
    }
    Some(out)
}

/// The quad warped upright, without laying tall strips down.
pub(crate) fn crop_as_is(img: &RgbImage, q: [Pt; 4]) -> Option<RgbImage> {
    let w = dist(q[0], q[1]).max(dist(q[2], q[3])) as u32;
    let h = dist(q[0], q[3]).max(dist(q[1], q[2])) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    let (wf, hf) = (w as f32, h as f32);
    let dst = [
        Pt { x: 0.0, y: 0.0 },
        Pt { x: wf, y: 0.0 },
        Pt { x: wf, y: hf },
        Pt { x: 0.0, y: hf },
    ];
    let m = homography(dst, q)?;
    let mut out = RgbImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (xf, yf) = (x as f32, y as f32);
            let d = m[6] * xf + m[7] * yf + m[8];
            let u = (m[0] * xf + m[1] * yf + m[2]) / d;
            let v = (m[3] * xf + m[4] * yf + m[5]) / d;
            out.put_pixel(x, y, sample(img, u, v));
        }
    }
    Some(out)
}

fn sample(img: &RgbImage, u: f32, v: f32) -> Rgb<u8> {
    let (w, h) = img.dimensions();
    let u = u.clamp(0.0, (w - 1) as f32);
    let v = v.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (u.floor() as u32, v.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (u - x0 as f32, v - y0 as f32);
    let (a, b) = (img.get_pixel(x0, y0).0, img.get_pixel(x1, y0).0);
    let (c, d) = (img.get_pixel(x0, y1).0, img.get_pixel(x1, y1).0);
    let mut px = [0u8; 3];
    for k in 0..3 {
        let top = f32::from(a[k]) * (1.0 - fx) + f32::from(b[k]) * fx;
        let bot = f32::from(c[k]) * (1.0 - fx) + f32::from(d[k]) * fx;
        px[k] = (top * (1.0 - fy) + bot * fy).round() as u8;
    }
    Rgb(px)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    #[test]
    fn an_axis_aligned_quad_is_a_plain_cut() {
        let mut img = RgbImage::new(20, 10);
        img.put_pixel(5, 3, Rgb([9, 9, 9]));
        let c = crop(&img, [p(4.0, 2.0), p(14.0, 2.0), p(14.0, 7.0), p(4.0, 7.0)]).unwrap();
        assert_eq!(c.dimensions(), (10, 5));
        assert_eq!(c.get_pixel(1, 1).0, [9, 9, 9]);
    }

    #[test]
    fn a_tall_strip_is_laid_down() {
        let img = RgbImage::new(20, 40);
        let c = crop(&img, [p(2.0, 2.0), p(8.0, 2.0), p(8.0, 32.0), p(2.0, 32.0)]).unwrap();
        assert_eq!(c.dimensions(), (30, 6));
    }
}
