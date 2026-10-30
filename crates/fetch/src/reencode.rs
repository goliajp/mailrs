//! Decoding a fetched image under hard bounds and encoding it afresh, so
//! nothing of the original file but its pixels comes through.

use std::io::Cursor;

/// A re-encoded image.
#[derive(Debug, Clone)]
pub struct Image {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Memory one decode may take, whatever the file claims.
const MAX_ALLOC: u64 = 128 * 1024 * 1024;

pub fn reencode(bytes: &[u8], max_side: u32) -> Result<Image, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    // checked against the header before any pixel buffer is allocated
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    match decoded.color().has_alpha() {
        true => {
            decoded
                .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            Ok(Image {
                content_type: "image/png",
                bytes: out,
            })
        }
        false => {
            let rgb = decoded.to_rgb8();
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
            enc.encode_image(&rgb).map_err(|e| e.to_string())?;
            Ok(Image {
                content_type: "image/jpeg",
                bytes: out,
            })
        }
    }
}
