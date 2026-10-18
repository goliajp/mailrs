//! glyph names to unicode: the adobe glyph list plus the `uniXXXX` / `uXXXX` forms

use super::glyphs::AGL;

/// Unicode text for a glyph name, or `None` when the name carries no meaning
/// (`g12`, `cid0042`, `.notdef`).
pub(super) fn to_unicode(name: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(name).ok()?;
    if let Some(s) = lookup(name) {
        return Some(s.to_string());
    }
    // "a.sc" and "f_f_i" name variants and ligatures of known glyphs
    let base = name.split('.').next().unwrap_or("");
    if base.is_empty() {
        return None;
    }
    let mut out = String::new();
    for part in base.split('_') {
        out.push_str(&component(part)?);
    }
    Some(out)
}

fn lookup(name: &str) -> Option<&'static str> {
    AGL.binary_search_by(|(n, _)| n.as_bytes().cmp(name.as_bytes()))
        .ok()
        .map(|i| AGL[i].1)
}

fn component(part: &str) -> Option<String> {
    if let Some(s) = lookup(part) {
        return Some(s.to_string());
    }
    if let Some(hex) = part.strip_prefix("uni")
        && !hex.is_empty()
        && hex.len() % 4 == 0
    {
        let mut out = String::new();
        for i in (0..hex.len()).step_by(4) {
            out.push(bmp_scalar(hex.get(i..i + 4)?)?);
        }
        return Some(out);
    }
    if let Some(hex) = part.strip_prefix('u')
        && (4..=6).contains(&hex.len())
    {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return char::from_u32(v).map(String::from);
    }
    None
}

fn bmp_scalar(hex: &str) -> Option<char> {
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    char::from_u32(u32::from_str_radix(hex, 16).ok()?)
}

#[cfg(test)]
mod tests {
    use super::to_unicode;

    #[test]
    fn agl_names() {
        assert_eq!(to_unicode(b"A").as_deref(), Some("A"));
        assert_eq!(to_unicode(b"quoteright").as_deref(), Some("\u{2019}"));
        assert_eq!(to_unicode(b"fi").as_deref(), Some("\u{fb01}"));
    }

    #[test]
    fn uni_and_u_forms() {
        assert_eq!(to_unicode(b"uni3042").as_deref(), Some("あ"));
        assert_eq!(to_unicode(b"uni00410042").as_deref(), Some("AB"));
        assert_eq!(to_unicode(b"u1F600").as_deref(), Some("\u{1F600}"));
        assert_eq!(to_unicode(b"uniD800"), None);
    }

    #[test]
    fn variants_and_ligatures() {
        assert_eq!(to_unicode(b"a.sc").as_deref(), Some("a"));
        assert_eq!(to_unicode(b"f_f_i").as_deref(), Some("ffi"));
    }

    #[test]
    fn meaningless_names() {
        assert_eq!(to_unicode(b"g123"), None);
        assert_eq!(to_unicode(b".notdef"), None);
        assert_eq!(to_unicode(&[0xff, 0xfe]), None);
    }
}
