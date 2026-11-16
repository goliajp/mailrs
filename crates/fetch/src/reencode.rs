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
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let decoded = match reader.format() {
        Some(image::ImageFormat::Gif) => {
            image::DynamicImage::ImageRgba8(first_gif_frame(bytes, max_side)?)
        }
        _ => decode(reader, max_side)?,
    };
    encode(&decoded)
}

fn decode(
    mut reader: image::ImageReader<Cursor<&[u8]>>,
    max_side: u32,
) -> Result<image::DynamicImage, String> {
    let mut limits = image::Limits::default();
    // checked against the header before any pixel buffer is allocated
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().map_err(|e| e.to_string())
}

/// A GIF's first frame on its canvas. A frame whose data ends before its
/// last pixel is kept as far as it goes, the rest transparent, as a
/// browser draws it; mailers send such files, and refusing them left a
/// hole where the reader's browser would have shown the image.
fn first_gif_frame(bytes: &[u8], max_side: u32) -> Result<image::RgbaImage, String> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    options.set_memory_limit(gif::MemoryLimit::Bytes(
        std::num::NonZeroU64::new(MAX_ALLOC).expect("a non-zero limit"),
    ));
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|e| e.to_string())?;
    let (width, height) = (u32::from(decoder.width()), u32::from(decoder.height()));
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return Err(format!("a {width}x{height} canvas"));
    }
    let Some(frame) = decoder.next_frame_info().map_err(|e| e.to_string())? else {
        return Err("no frame".into());
    };
    let (left, top) = (i64::from(frame.left), i64::from(frame.top));
    let (fw, fh) = (u32::from(frame.width), u32::from(frame.height));
    if fw > max_side || fh > max_side {
        return Err(format!("a {fw}x{fh} frame"));
    }
    let mut pixels = vec![0u8; decoder.buffer_size()];
    match decoder.read_into_buffer(&mut pixels) {
        Ok(()) => {}
        Err(gif::DecodingError::UnexpectedEof) => {}
        Err(e) if e.to_string() == "image truncated" => {}
        Err(e) => return Err(e.to_string()),
    }
    let frame = image::RgbaImage::from_raw(fw, fh, pixels).ok_or("a frame of the wrong size")?;
    let mut canvas = image::RgbaImage::new(width, height);
    image::imageops::overlay(&mut canvas, &frame, left, top);
    Ok(canvas)
}

fn encode(decoded: &image::DynamicImage) -> Result<Image, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 GIF whose image data ends after its first pixel, white.
    const SHORT_GIF: &[u8] = &[
        b'G', b'I', b'F', b'8', b'9', b'a', 2, 0, 2, 0, 0x80, 0, 0, // screen, two colours
        0, 0, 0, 0xff, 0xff, 0xff, // black, white
        0x2c, 0, 0, 0, 0, 2, 0, 2, 0, 0, // the frame
        2, 2, 0x4c, 0x01, 0, // LZW: clear, white, end
        0x3b,
    ];

    #[test]
    fn a_gif_that_ends_early_is_kept_as_far_as_it_goes() {
        let refused = image::load_from_memory(SHORT_GIF);
        assert!(
            refused.is_err(),
            "the general decoder takes it: {refused:?}"
        );

        let image = reencode(SHORT_GIF, 4096).unwrap();
        assert_eq!(image.content_type, "image/png");
        let back = image::load_from_memory(&image.bytes).unwrap().to_rgba8();
        assert_eq!(back.dimensions(), (2, 2));
        assert_eq!(back.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(back.get_pixel(1, 1).0[3], 0, "the rest is transparent");
    }

    #[test]
    fn a_gif_wider_than_allowed_is_refused() {
        assert!(reencode(SHORT_GIF, 1).is_err());
    }
}
