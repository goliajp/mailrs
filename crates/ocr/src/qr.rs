//! QR codes in an image, read for the web addresses they carry.
//!
//! A code is a link the reader cannot see before following it, and the
//! one a fraud puts in an image is meant for a phone the mail filter
//! never sees. The fraud rules compare where a code points with what the
//! image and the message say; this only finds the codes.

use image::RgbImage;

/// Codes read from one image, at most.
const MAX_CODES: usize = 4;
/// Longest payload kept.
const MAX_PAYLOAD: usize = 4096;
/// Longest address kept.
const MAX_URL: usize = 2048;
/// Smallest image looked at; a code below this is not readable anyway.
pub const MIN_SIDE: u32 = 100;

/// The `http(s)` addresses the QR codes in `img` carry.
#[must_use]
pub fn urls(img: &RgbImage) -> Vec<String> {
    let (w, h) = img.dimensions();
    if w.min(h) < MIN_SIDE {
        return Vec::new();
    }
    let mut prepared =
        rqrr::PreparedImage::prepare_from_greyscale(w as usize, h as usize, |x, y| {
            let p = img.get_pixel(x as u32, y as u32).0;
            ((u16::from(p[0]) * 3 + u16::from(p[1]) * 6 + u16::from(p[2])) / 10) as u8
        });
    prepared
        .detect_grids()
        .into_iter()
        .take(MAX_CODES)
        .filter_map(|grid| grid.decode().ok().map(|(_, text)| text))
        .filter(|text| text.len() <= MAX_PAYLOAD)
        .map(|text| text.trim().to_string())
        .filter(|text| {
            let lower = text.to_ascii_lowercase();
            lower.starts_with("http://") || lower.starts_with("https://")
        })
        .map(|mut text| {
            if text.len() > MAX_URL {
                let mut end = MAX_URL;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
            }
            text
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` as a QR code, `scale` pixels a module, with a quiet zone.
    fn code(text: &str, scale: u32) -> RgbImage {
        let qr = qrcode::QrCode::new(text.as_bytes()).expect("encodes");
        let width = qr.width() as u32;
        let colors = qr.to_colors();
        let side = (width + 8) * scale;
        RgbImage::from_fn(side, side, |x, y| {
            let (mx, my) = (x / scale, y / scale);
            let inside = (4..width + 4).contains(&mx) && (4..width + 4).contains(&my);
            let dark =
                inside && colors[((my - 4) * width + (mx - 4)) as usize] == qrcode::Color::Dark;
            match dark {
                true => image::Rgb([0, 0, 0]),
                false => image::Rgb([255, 255, 255]),
            }
        })
    }

    #[test]
    fn a_code_with_a_web_address_is_read() {
        let img = code("https://login.example/verify?id=7", 6);
        assert_eq!(urls(&img), vec!["https://login.example/verify?id=7"]);
    }

    #[test]
    fn a_code_without_a_web_address_and_a_tiny_image_give_nothing() {
        assert!(urls(&code("WIFI:S:office;T:WPA;P:secret;;", 6)).is_empty());
        assert!(urls(&RgbImage::new(40, 40)).is_empty());
    }
}
