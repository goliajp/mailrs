//! predefined cjk cmaps named by a type0 font's /Encoding
//!
//! the unicode-keyed cmaps (`UniJIS-UCS2-H`, `UniGB-UTF16-H`, ...) carry the
//! character in the code itself, and the legacy ones (`90ms-RKSJ-H`,
//! `GBK-EUC-H`, `ETen-B5-H`, `KSCms-UHC-H`, ...) carry a code in a national
//! character set, so both decode without the cid tables.

use encoding_rs::{BIG5, EUC_JP, EUC_KR, Encoding, GB18030, GBK, SHIFT_JIS};

use super::japan1::{JAPAN1_ASTRAL, JAPAN1_BMP};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Charset {
    Ucs2,
    Utf16,
    Utf8,
    Utf32,
    Legacy(&'static Encoding),
    /// raw two-byte jis x 0208 codes (the `H` / `V` cmaps)
    Jis,
}

pub(super) fn charset_for(name: &[u8]) -> Option<Charset> {
    let name = std::str::from_utf8(name).ok()?;
    if name == "H" || name == "V" {
        return Some(Charset::Jis);
    }
    if name.starts_with("Uni") {
        return [
            ("-UCS2-", Charset::Ucs2),
            ("-UTF16-", Charset::Utf16),
            ("-UTF8-", Charset::Utf8),
            ("-UTF32-", Charset::Utf32),
        ]
        .into_iter()
        .find(|(k, _)| name.contains(k))
        .map(|(_, c)| c);
    }
    let legacy = if name.contains("RKSJ") {
        SHIFT_JIS
    } else if name.starts_with("EUC-") {
        EUC_JP
    } else if name.starts_with("GBK2K") {
        GB18030
    } else if name.starts_with("GB") {
        GBK
    } else if name.contains("B5") {
        BIG5
    } else if name.starts_with("KSC") {
        EUC_KR
    } else {
        return None;
    };
    Some(Charset::Legacy(legacy))
}

/// Byte length of the code that starts `bytes` (non-empty).
pub(super) fn code_len(cs: Charset, bytes: &[u8]) -> usize {
    let b = bytes[0];
    let n = match cs {
        Charset::Ucs2 | Charset::Jis => 2,
        Charset::Utf32 => 4,
        Charset::Utf16 if (0xD8..0xDC).contains(&b) => 4,
        Charset::Utf16 => 2,
        Charset::Utf8 => match b {
            0xF0.. => 4,
            0xE0.. => 3,
            0xC0.. => 2,
            _ => 1,
        },
        Charset::Legacy(e) if e == SHIFT_JIS => {
            usize::from(matches!(b, 0x81..=0x9F | 0xE0..=0xFC)) + 1
        }
        Charset::Legacy(e) if e == EUC_JP => match b {
            0x8F => 3,
            0x8E | 0xA1..=0xFE => 2,
            _ => 1,
        },
        Charset::Legacy(_) => usize::from(b >= 0x81) + 1,
    };
    n.min(bytes.len())
}

pub(super) fn decode(cs: Charset, code: &[u8], out: &mut String) {
    let text = match cs {
        Charset::Ucs2 | Charset::Utf16 => {
            let units = code
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]));
            char::decode_utf16(units).filter_map(Result::ok).collect()
        }
        Charset::Utf32 => code
            .chunks_exact(4)
            .filter_map(|c| char::from_u32(u32::from_be_bytes([c[0], c[1], c[2], c[3]])))
            .collect(),
        Charset::Utf8 => String::from_utf8_lossy(code).into_owned(),
        Charset::Legacy(e) => e.decode_without_bom_handling(code).0.into_owned(),
        Charset::Jis => {
            let euc: Vec<u8> = code.iter().map(|b| b | 0x80).collect();
            EUC_JP.decode_without_bom_handling(&euc).0.into_owned()
        }
    };
    out.extend(text.chars().filter(|&c| c != '\u{FFFD}'));
}

/// Unicode of an adobe-japan1 cid.
pub(super) fn japan1(cid: u32) -> Option<char> {
    let cid = u16::try_from(cid).ok()?;
    match JAPAN1_BMP.get(usize::from(cid)) {
        Some(0) | None => {
            let i = JAPAN1_ASTRAL.binary_search_by_key(&cid, |e| e.0).ok()?;
            char::from_u32(JAPAN1_ASTRAL[i].1)
        }
        Some(&u) => char::from_u32(u32::from(u)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &[u8], bytes: &[u8]) -> String {
        let cs = charset_for(name).expect("known cmap");
        let mut out = String::new();
        let mut rest = bytes;
        while !rest.is_empty() {
            let n = code_len(cs, rest);
            decode(cs, &rest[..n], &mut out);
            rest = &rest[n..];
        }
        out
    }

    #[test]
    fn unicode_cmaps() {
        assert_eq!(run(b"UniJIS-UCS2-H", &[0x30, 0x42, 0x00, 0x41]), "あA");
        assert_eq!(
            run(b"UniGB-UTF16-V", &[0xD8, 0x3D, 0xDE, 0x00]),
            "\u{1F600}"
        );
    }

    #[test]
    fn legacy_cmaps() {
        assert_eq!(run(b"90ms-RKSJ-H", &[0x82, 0xA0, b'A']), "あA");
        assert_eq!(run(b"EUC-H", &[0xA4, 0xA2]), "あ");
        assert_eq!(run(b"H", &[0x24, 0x22]), "あ");
    }

    #[test]
    fn japan1_cids() {
        assert_eq!(japan1(34), Some('A'));
        assert_eq!(japan1(843), Some('あ'));
        assert_eq!(japan1(0), None);
        assert_eq!(japan1(u32::MAX), None);
    }

    #[test]
    fn unknown_cmap() {
        assert_eq!(charset_for(b"Adobe-Japan1-6"), None);
    }
}
