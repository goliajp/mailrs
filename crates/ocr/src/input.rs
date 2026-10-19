//! Turning attachment bytes into pixels: image decoding with size limits,
//! and rendering of PDF pages

use std::io::Cursor;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
use image::{ImageReader, Limits, Rgb, RgbImage, RgbaImage};

use crate::OcrError;

/// Decode an image attachment. Anything wider or taller than `max_side`
/// pixels, or needing more than `max_bytes` to decode, is refused before
/// its pixels are allocated.
pub fn decode_image(bytes: &[u8], max_side: u32, max_bytes: u64) -> Result<RgbImage, OcrError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| OcrError::Input(e.to_string()))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(max_bytes);
    reader.limits(limits);
    let img = reader
        .decode()
        .map_err(|e| OcrError::Input(e.to_string()))?;
    if !img.color().has_alpha() {
        return Ok(img.to_rgb8());
    }
    Ok(flatten(&img.to_rgba8()))
}

/// Composite onto white when the visible pixels are dark and onto black
/// when they are light, so text drawn on a transparent background keeps
/// its contrast.
fn flatten(img: &RgbaImage) -> RgbImage {
    let (mut sum, mut n) = (0f64, 0u64);
    for p in img.pixels().filter(|p| p.0[3] > 0) {
        sum += 0.299 * f64::from(p.0[0]) + 0.587 * f64::from(p.0[1]) + 0.114 * f64::from(p.0[2]);
        n += 1;
    }
    let mut bg = 255f32;
    if n > 0 && sum / n as f64 >= 128.0 {
        bg = 0.0;
    }
    RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y).0;
        let a = f32::from(p[3]) / 255.0;
        let mix = |c: u8| (f32::from(c) * a + bg * (1.0 - a)) as u8;
        Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

/// A parsed PDF whose pages can be rendered one at a time.
pub struct PdfPages {
    pdf: Pdf,
}

impl PdfPages {
    /// Parse the document.
    pub fn open(bytes: Vec<u8>) -> Result<PdfPages, OcrError> {
        let pdf = Pdf::new(bytes).map_err(|e| OcrError::Input(format!("pdf: {e:?}")))?;
        Ok(PdfPages { pdf })
    }

    /// Number of pages.
    pub fn len(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Whether the document has no pages.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Page `index` (from 0) on white at `dpi`, scaled down further when
    /// that would make a side longer than `max_side` pixels.
    pub fn render(&self, index: usize, dpi: f32, max_side: u32) -> Result<RgbImage, OcrError> {
        let pages = self.pdf.pages();
        let page = pages
            .get(index)
            .ok_or_else(|| OcrError::Input(format!("no page {index}")))?;
        let (w, h) = page.render_dimensions();
        if w <= 0.0 || h <= 0.0 {
            return Err(OcrError::Input(format!("page {index} has no area")));
        }
        let scale = (dpi / 72.0).min(max_side as f32 / w.max(h));
        let settings = PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        };
        let cache = RenderCache::new();
        let pixmap = render(
            page,
            &cache,
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &settings,
        );
        let (pw, ph) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        // the page is drawn onto opaque white, so premultiplied is plain
        let rgba = pixmap.data();
        Ok(RgbImage::from_fn(pw, ph, |x, y| {
            let p = rgba[(y * pw + x) as usize];
            Rgb([p.r, p.g, p.b])
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_text_on_transparency_goes_onto_white() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        let flat = flatten(&img);
        assert_eq!(flat.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(flat.get_pixel(1, 0).0, [255, 255, 255]);
    }

    #[test]
    fn light_text_on_transparency_goes_onto_black() {
        let mut img = RgbaImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgba([250, 250, 250, 255]));
        assert_eq!(flatten(&img).get_pixel(1, 0).0, [0, 0, 0]);
    }

    #[test]
    fn an_oversized_image_is_refused_before_decoding() {
        let mut png = Vec::new();
        RgbImage::new(300, 10)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(decode_image(&png, 200, 1 << 30).is_err());
        assert!(decode_image(&png, 400, 1 << 30).is_ok());
        assert!(decode_image(b"not an image", 400, 1 << 30).is_err());
    }
}
