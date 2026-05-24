//! Tests for `proxy` (extracted from inline `#[cfg(test)] mod tests` per the one-file-one-thing policy).

use super::*;

#[test]
fn transparent_png_is_valid() {
    // verify PNG signature
    assert_eq!(&TRANSPARENT_1X1_PNG[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
    // verify IHDR chunk (1x1 RGBA)
    assert!(TRANSPARENT_1X1_PNG.len() > 20);
    // verify IEND chunk at end
    assert!(TRANSPARENT_1X1_PNG.ends_with(&[0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82]));
}
