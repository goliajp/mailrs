//! What kind of image an upstream sent, and whether it is big enough to
//! be a logo — read from headers, without decoding.

/// Smallest icon worth drawing.  Real favicons start at 16×16; the
/// placeholders are 1×1.  Eight is comfortably between them and needs
/// no judgement about which sizes a brand might publish.
const MIN_ICON_PX: u32 = 8;

/// Whether the bytes are an SVG, by the upstream's content type or the
/// document's first bytes.
pub(super) fn is_svg(content_type: &str, bytes: &[u8]) -> bool {
    content_type.contains("svg") || bytes.starts_with(b"<svg") || bytes.starts_with(b"<?xml")
}

/// Whether these bytes carry an image big enough to be a logo.
///
/// Vector icons (BIMI's SVG) have no pixel size to read and are taken
/// at face value — a brand that published one meant it.  Everything
/// else states its dimensions in its header, and this reads only that:
/// four formats, fixed offsets, no decoding.
pub(super) fn usable_icon(content_type: &str, bytes: &[u8]) -> bool {
    if is_svg(content_type, bytes) {
        return true;
    }
    match icon_dimensions(bytes) {
        Some((w, h)) => w >= MIN_ICON_PX && h >= MIN_ICON_PX,
        // Not a format we can measure: keep the old behaviour and
        // serve it.  The rule is for the placeholders we have seen,
        // not a licence to drop anything unfamiliar.
        None => true,
    }
}

/// Width and height from an image header — PNG, GIF, ICO, JPEG.
fn icon_dimensions(b: &[u8]) -> Option<(u32, u32)> {
    // PNG: IHDR is always first, at a fixed offset.
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.len() >= 24 {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h));
    }
    // GIF: logical screen descriptor, little-endian, right after the
    // six-byte signature.
    if (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) && b.len() >= 10 {
        let w = u16::from_le_bytes(b[6..8].try_into().ok()?);
        let h = u16::from_le_bytes(b[8..10].try_into().ok()?);
        return Some((u32::from(w), u32::from(h)));
    }
    // ICO: one byte per axis in each directory entry, 0 meaning 256.
    // An .ico can hold several sizes; the largest is the one a 36 px
    // circle would use.
    if b.starts_with(b"\x00\x00\x01\x00") && b.len() >= 6 {
        let count = u16::from_le_bytes(b[4..6].try_into().ok()?) as usize;
        let mut best = (0u32, 0u32);
        for i in 0..count {
            let e = 6 + i * 16;
            if b.len() < e + 16 {
                break;
            }
            let dim = |v: u8| if v == 0 { 256u32 } else { u32::from(v) };
            let (w, h) = (dim(b[e]), dim(b[e + 1]));
            if w.min(h) > best.0.min(best.1) {
                best = (w, h);
            }
        }
        return (best != (0, 0)).then_some(best);
    }
    // JPEG: walk the segment chain to the frame header that carries
    // the dimensions.
    if b.starts_with(b"\xff\xd8") {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                return None;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes(b[i + 2..i + 4].try_into().ok()?) as usize;
            // SOF0..SOF15, minus the four that are not frame headers.
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC | 0xD8) {
                let h = u16::from_be_bytes(b[i + 5..i + 7].try_into().ok()?);
                let w = u16::from_be_bytes(b[i + 7..i + 9].try_into().ok()?);
                return Some((u32::from(w), u32::from(h)));
            }
            i += 2 + len;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DuckDuckGo's answer for a domain it has no icon for, byte for
    /// byte off the wire (2026-09-18,
    /// `customeremail.microsoftrewards.com`): a 1×1 transparent GIF,
    /// served as `200 image/x-icon`.  Serving it renders a sender with
    /// no avatar at all.
    const DDG_PLACEHOLDER: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\xff\xff\x00\x00\x00\x21\xf9\x04\x01\x00\x00\x00\x00\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b";

    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b
    }

    fn ico(entries: &[(u8, u8)]) -> Vec<u8> {
        let mut b = vec![0, 0, 1, 0];
        b.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (w, h) in entries {
            b.extend_from_slice(&[*w, *h, 0, 0, 1, 0, 32, 0]);
            b.extend_from_slice(&[0; 8]);
        }
        b
    }

    #[test]
    fn a_providers_one_by_one_placeholder_is_not_an_icon() {
        assert_eq!(DDG_PLACEHOLDER.len(), 43);
        assert_eq!(icon_dimensions(DDG_PLACEHOLDER), Some((1, 1)));
        assert!(!usable_icon("image/x-icon", DDG_PLACEHOLDER));
    }

    #[test]
    fn a_real_favicon_is_an_icon() {
        assert!(usable_icon("image/png", &png_header(128, 128)));
        assert!(usable_icon("image/png", &png_header(16, 16)));
        assert!(!usable_icon("image/png", &png_header(1, 1)));
    }

    /// An `.ico` carries several sizes; the biggest is the one a 36 px
    /// circle draws, so a file that also holds a 1×1 is still fine.
    #[test]
    fn an_ico_is_measured_by_its_largest_entry() {
        assert_eq!(icon_dimensions(&ico(&[(16, 16), (32, 32)])), Some((32, 32)));
        assert!(usable_icon("image/x-icon", &ico(&[(1, 1), (32, 32)])));
        assert!(!usable_icon("image/x-icon", &ico(&[(1, 1)])));
        // 0 means 256 in the ICO directory.
        assert_eq!(icon_dimensions(&ico(&[(0, 0)])), Some((256, 256)));
    }

    /// BIMI icons are SVG: nothing to measure, and a brand that
    /// published one meant it.  And a format we cannot read is served
    /// as before rather than dropped.
    #[test]
    fn vector_and_unknown_formats_are_kept() {
        assert!(usable_icon(
            "image/svg+xml",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        ));
        assert!(usable_icon("image/webp", b"RIFF????WEBPVP8 "));
        assert_eq!(icon_dimensions(b"RIFF????WEBPVP8 "), None);
    }
}
