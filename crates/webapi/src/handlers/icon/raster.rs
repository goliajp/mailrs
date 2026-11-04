//! An icon as a PNG, for clients that cannot decode SVG or ICO.
//!
//! The input comes from a sender's DNS and the open web, so it is parsed
//! with nothing that can reach outside it: no `<image>` href is followed
//! (neither a file path nor a data URL), resvg is built without text,
//! fonts or raster-image decoding, and the canvas is never bigger than
//! [`MAX_SIDE`] whatever the SVG declares. Anything that does not parse
//! is `None`, and the caller moves on to the next source.

use std::io::Cursor;

use resvg::{tiny_skia, usvg};

/// Longest side of the PNG.
pub const MAX_SIDE: u32 = 128;

/// Largest raster accepted for decoding, per axis. Favicon services send
/// at most 256; this leaves room without letting a header claim a canvas
/// that would take gigabytes.
const MAX_DECODE_SIDE: u32 = 4096;
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;

/// Most nodes an SVG may draw. A brand logo is tens to a few hundred.
const MAX_NODES: usize = 10_000;

/// The icon as a PNG of at most `MAX_SIDE` a side, aspect and
/// transparency kept. SVG is drawn at `MAX_SIDE`; a raster smaller than
/// that keeps its size.
pub fn to_png(content_type: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    match super::measure::is_svg(content_type, bytes) {
        true => svg_to_png(bytes),
        false => raster_to_png(bytes),
    }
}

fn svg_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(bytes, &options).ok()?;
    // filters cost in proportion to a region the SVG chooses, not to the
    // canvas, and BIMI's tiny-ps profile has none
    if !tree.filters().is_empty() || drawn_nodes(&tree) > MAX_NODES {
        return None;
    }
    let size = tree.size();
    let (w, h) = fit(size.width(), size.height())?;
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let scale = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, scale, &mut pixmap.as_mut());
    pixmap.encode_png().ok()
}

/// Every node resvg will visit: the tree, and the clip paths, masks and
/// patterns it draws through. `<use>` is already expanded here, so a
/// small document that references itself many times counts at its full
/// size.
fn drawn_nodes(tree: &usvg::Tree) -> usize {
    fn count(group: &usvg::Group) -> usize {
        group
            .children()
            .iter()
            .map(|node| match node {
                usvg::Node::Group(g) => 1 + count(g),
                _ => 1,
            })
            .sum()
    }
    count(tree.root())
        + tree
            .clip_paths()
            .iter()
            .map(|c| count(c.root()))
            .sum::<usize>()
        + tree.masks().iter().map(|m| count(m.root())).sum::<usize>()
        + tree
            .patterns()
            .iter()
            .map(|p| count(p.root()))
            .sum::<usize>()
}

/// Pixel size with the longer side at `MAX_SIDE`, the shorter at least 1.
fn fit(width: f32, height: f32) -> Option<(u32, u32)> {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return None;
    }
    let scale = MAX_SIDE as f32 / width.max(height);
    let side = |v: f32| ((v * scale).round() as u32).clamp(1, MAX_SIDE);
    Some((side(width), side(height)))
}

fn raster_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DECODE_SIDE);
    limits.max_image_height = Some(MAX_DECODE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let mut img = reader.decode().ok()?;
    if img.width() > MAX_SIDE || img.height() > MAX_SIDE {
        // keeps the aspect: the result fits inside the box
        img = img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Lanczos3);
    }
    let mut out = Vec::new();
    img.into_rgba8()
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn decode(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .expect("a PNG")
            .into_rgba8()
    }

    fn encode(img: &image::RgbaImage, format: image::ImageFormat) -> Vec<u8> {
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), format)
            .expect("encode");
        out
    }

    /// Drawn within a budget a request can afford; the handler gives up
    /// after `RENDER_BUDGET`, and these must not come near it.
    fn timed(svg: &str) -> Option<Vec<u8>> {
        let start = Instant::now();
        let out = to_png("image/svg+xml", svg.as_bytes());
        let took = start.elapsed();
        assert!(took < Duration::from_secs(1), "took {took:?}");
        out
    }

    #[test]
    fn a_bimi_logo_is_drawn_at_full_size_with_its_corners_clear() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" version="1.2" baseProfile="tiny-ps" viewBox="0 0 512 512"><title>Example</title><rect width="512" height="512" rx="96" fill="#1a73e8"/><path d="M128 256 L256 128 L384 256 L256 384 Z" fill="#fff"/></svg>"##;
        let png = decode(&timed(svg).expect("drawn"));
        assert_eq!(png.dimensions(), (128, 128));
        assert_eq!(png.get_pixel(0, 0)[3], 0);
        assert_eq!(png.get_pixel(64, 64).0, [255, 255, 255, 255]);
        assert_eq!(png.get_pixel(20, 64).0, [0x1a, 0x73, 0xe8, 255]);
    }

    #[test]
    fn a_wide_or_tall_svg_keeps_its_aspect() {
        let wide = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 100"><rect width="400" height="100"/></svg>"#;
        assert_eq!(decode(&timed(wide).expect("wide")).dimensions(), (128, 32));
        let tall = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="1000"><rect width="10" height="1000"/></svg>"#;
        assert_eq!(decode(&timed(tall).expect("tall")).dimensions(), (1, 128));
    }

    #[test]
    fn an_ico_becomes_a_png_and_a_big_raster_is_scaled_down() {
        let small = image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 128, 0, 200]));
        let png =
            decode(&to_png("image/x-icon", &encode(&small, image::ImageFormat::Ico)).unwrap());
        assert_eq!(png.dimensions(), (32, 32));
        assert_eq!(png.get_pixel(3, 3).0, [0, 128, 0, 200], "transparency kept");

        let big = image::RgbaImage::from_pixel(256, 64, image::Rgba([9, 9, 9, 255]));
        let png = decode(&to_png("image/png", &encode(&big, image::ImageFormat::Png)).unwrap());
        assert_eq!(png.dimensions(), (128, 32));
        let jpeg = image::RgbImage::from_pixel(300, 300, image::Rgb([200, 10, 10]));
        let mut bytes = Vec::new();
        jpeg.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(
            decode(&to_png("image/jpeg", &bytes).unwrap()).dimensions(),
            (128, 128)
        );
    }

    #[test]
    fn what_does_not_parse_is_none() {
        assert_eq!(to_png("image/svg+xml", b"<svg><g>"), None);
        assert_eq!(to_png("image/png", b"\x89PNG\r\n\x1a\nnot really"), None);
        assert_eq!(to_png("image/webp", b"RIFF????WEBPVP8 "), None);
        // a raster header claiming a canvas far beyond any icon
        let mut huge = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        huge.extend_from_slice(&100_000u32.to_be_bytes());
        huge.extend_from_slice(&100_000u32.to_be_bytes());
        huge.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(to_png("image/png", &huge), None);
    }

    /// A hostile SVG reaches nothing outside itself: an `<image>` or
    /// `<use>` pointing at a listening port is never connected to, and a
    /// local file is never read (the red PNG below would show).
    #[test]
    fn a_hostile_svg_fetches_and_reads_nothing() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = tempfile::tempdir().unwrap();
        let red = dir.path().join("red.png");
        std::fs::write(
            &red,
            encode(
                &image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255])),
                image::ImageFormat::Png,
            ),
        )
        .unwrap();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 100 100">
<image href="http://127.0.0.1:{port}/a.png" width="100" height="100"/>
<image xlink:href="{red}" width="100" height="100"/>
<image href="file://{red}" width="100" height="100"/>
<use href="http://127.0.0.1:{port}/b.svg#x"/>
<style>@import url(http://127.0.0.1:{port}/c.css);</style>
<rect width="100" height="100" fill="url(http://127.0.0.1:{port}/d.svg#p)"/>
</svg>"#,
            red = red.display()
        );
        let png = decode(&timed(&svg).expect("drawn"));
        assert!(
            png.pixels().all(|p| p.0 != [255, 0, 0, 255]),
            "a local file was read into the drawing"
        );
        assert!(
            listener.accept().is_err(),
            "the SVG made a connection to a URL it named"
        );
    }

    #[test]
    fn a_huge_viewbox_or_size_still_draws_a_small_canvas() {
        let huge = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1e30 1e30"><rect width="1e30" height="1e30"/></svg>"#;
        assert_eq!(
            decode(&timed(huge).expect("drawn")).dimensions(),
            (128, 128)
        );
        let big = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000000000" height="1"><rect width="1000000000" height="1"/></svg>"#;
        assert_eq!(decode(&timed(big).expect("drawn")).dimensions(), (128, 1));
        assert_eq!(
            timed(r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="10"/>"#),
            None
        );
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let depth = 5_000;
        let deep = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">{}<rect width="10" height="10"/>{}</svg>"#,
            "<g>".repeat(depth),
            "</g>".repeat(depth)
        );
        let _ = timed(&deep);
    }

    /// Each level draws the one below ten times: ten levels would be ten
    /// billion rectangles if `<use>` were expanded without limit. usvg
    /// stops at a million nodes, and the document is refused.
    #[test]
    fn use_fan_out_is_bounded() {
        let mut defs = String::from(r#"<rect id="l0" width="1" height="1"/>"#);
        for level in 1..=10 {
            defs.push_str(&format!(r#"<g id="l{level}">"#));
            for _ in 0..10 {
                defs.push_str(&format!(r##"<use href="#l{}"/>"##, level - 1));
            }
            defs.push_str("</g>");
        }
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs>{defs}</defs><use href="#l10"/></svg>"##
        );
        assert_eq!(timed(&svg), None, "ten billion rectangles were drawn");
    }

    #[test]
    fn entity_expansion_is_bounded() {
        let laughs = r#"<?xml version="1.0"?>
<!DOCTYPE svg [
<!ENTITY a "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">
<!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">
<!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">
<!ENTITY d "&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;">
<!ENTITY e "&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;">
<!ENTITY f "&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;">
]>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><title>&f;</title></svg>"#;
        let _ = timed(laughs);
    }

    #[test]
    fn filters_are_not_drawn() {
        // a blur over a region far beyond the canvas, two hundred times
        let blur = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1e6 1e6"><filter id="f" x="-100" y="-100" width="200" height="200"><feGaussianBlur stdDeviation="1e9"/></filter>{}</svg>"#,
            r#"<rect width="1e6" height="1e6" filter="url(#f)"/>"#.repeat(200)
        );
        assert_eq!(timed(&blur), None, "a filter was drawn");
    }
}
