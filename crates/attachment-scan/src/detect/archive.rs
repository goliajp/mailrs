//! What an archive says it holds.
//!
//! A zip names its entries in its central directory, at the end of the
//! file, uncompressed; reading that is reading a table of names, so a zip
//! bomb is a table like any other and nothing is decompressed. The read
//! touches only the directory, so the byte cap does not apply.

use super::Found;
use crate::Attachment;

/// Entries read from one directory, at most.
const ENTRIES_MAX: usize = 10_000;

/// One entry of a zip's central directory.
pub(crate) struct Entry {
    pub name: String,
    pub encrypted: bool,
}

/// The entries of a zip, or `None` when the bytes are not one or its
/// directory cannot be found.
pub(crate) fn zip_entries(b: &[u8]) -> Option<Vec<Entry>> {
    if !b.starts_with(b"PK\x03\x04") || b.len() < 22 {
        return None;
    }
    // the end-of-directory record sits in the last 64 KiB + 22 bytes
    let from = b.len().saturating_sub(65_535 + 22);
    let eocd = (from..=b.len() - 22)
        .rev()
        .find(|&i| b[i..].starts_with(b"PK\x05\x06"))?;
    let count = u16_at(b, eocd + 10)? as usize;
    let mut at = u32_at(b, eocd + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..count.min(ENTRIES_MAX) {
        if !b.get(at..)?.starts_with(b"PK\x01\x02") {
            break;
        }
        let flags = u16_at(b, at + 8)?;
        let name_len = u16_at(b, at + 28)? as usize;
        let extra_len = u16_at(b, at + 30)? as usize;
        let comment_len = u16_at(b, at + 32)? as usize;
        let name = b.get(at + 46..at + 46 + name_len)?;
        out.push(Entry {
            name: String::from_utf8_lossy(name).to_string(),
            encrypted: flags & 1 == 1,
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Some(out)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A zip whose directory names something the machine runs.
pub(crate) fn holds_code(a: &Attachment<'_>) -> Option<Found> {
    let entries = zip_entries(a.bytes)?;
    let inside = entries.iter().find_map(|e| {
        let base = e.name.rsplit('/').next().unwrap_or(&e.name);
        super::names::executable(base).map(|_| e.name.clone())
    })?;
    Some(Found::new(format!("{inside} inside {:?}", a.name)))
}

/// A zip with an entry it encrypted.
pub(crate) fn encrypted(a: &Attachment<'_>) -> Option<Found> {
    let entries = zip_entries(a.bytes)?;
    let e = entries.iter().find(|e| e.encrypted)?;
    Some(Found::new(format!("{} is encrypted", e.name)))
}

/// A rar or 7z archive; their contents are not listed here.
pub(crate) fn unlistable(a: &Attachment<'_>) -> Option<String> {
    if a.bytes.starts_with(b"Rar!\x1a\x07") {
        return Some("a rar archive".into());
    }
    if a.bytes.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Some("a 7z archive".into());
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A zip with these `(name, encrypted)` entries, contents empty.
    pub(crate) fn zip(entries: &[(&str, bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut dir = Vec::new();
        for (name, encrypted) in entries {
            let offset = out.len() as u32;
            let flags: u16 = u16::from(*encrypted);
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&[20, 0]);
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&[0u8; 18]);
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            dir.extend_from_slice(b"PK\x01\x02");
            dir.extend_from_slice(&[20, 0, 20, 0]);
            dir.extend_from_slice(&flags.to_le_bytes());
            dir.extend_from_slice(&[0u8; 18]);
            dir.extend_from_slice(&(name.len() as u16).to_le_bytes());
            dir.extend_from_slice(&[0u8; 12]);
            dir.extend_from_slice(&offset.to_le_bytes());
            dir.extend_from_slice(name.as_bytes());
        }
        let (dir_at, dir_len) = (out.len() as u32, dir.len() as u32);
        out.extend_from_slice(&dir);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&[0u8; 4]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&dir_len.to_le_bytes());
        out.extend_from_slice(&dir_at.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    fn att<'a>(name: &'a str, bytes: &'a [u8]) -> Attachment<'a> {
        Attachment {
            name,
            declared_type: "",
            bytes,
        }
    }

    #[test]
    fn a_program_inside_a_zip() {
        let z = zip(&[("docs/readme.txt", false), ("docs/invoice.pdf.exe", false)]);
        assert!(
            holds_code(&att("invoice.zip", &z))
                .unwrap()
                .what
                .contains("invoice.pdf.exe")
        );
        let clean = zip(&[("report.xml", false), ("data/a.csv", false)]);
        assert!(holds_code(&att("report.zip", &clean)).is_none());
    }

    #[test]
    fn an_encrypted_entry() {
        assert!(encrypted(&att("scan.zip", &zip(&[("scan.pdf", true)]))).is_some());
        assert!(encrypted(&att("scan.zip", &zip(&[("scan.pdf", false)]))).is_none());
    }

    #[test]
    fn broken_and_foreign_input_is_read_safely() {
        let mut z = zip(&[("a.exe", false)]);
        z.truncate(z.len() - 10);
        let _ = zip_entries(&z);
        assert!(zip_entries(b"PK\x03\x04").is_none());
        assert!(zip_entries(b"not a zip at all, but long enough to try").is_none());
        // a directory offset pointing past the end
        let mut z = zip(&[("a.exe", false)]);
        let n = z.len();
        z[n - 6..n - 2].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(zip_entries(&z).is_none_or(|e| e.is_empty()));
        assert_eq!(
            unlistable(&att("a.rar", b"Rar!\x1a\x07\x01\x00")).as_deref(),
            Some("a rar archive")
        );
        assert_eq!(
            unlistable(&att("a.7z", b"7z\xbc\xaf\x27\x1c\x00")).as_deref(),
            Some("a 7z archive")
        );
    }
}
