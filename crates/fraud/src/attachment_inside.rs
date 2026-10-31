//! What an attachment hides inside it: an archive holding something the
//! machine would run, an archive that will not say what it holds, an
//! Office document carrying macros whatever its name says.
//!
//! **Listing, not unpacking.** A zip says what it holds in its central
//! directory, at the end of the file, uncompressed; reading that is
//! reading a table of names. Nothing is decompressed, so a zip bomb is a
//! table like any other and the work is bounded by the attachment's size.
//! An Office file from before 2007 names its streams in UTF-16 inside
//! the file, and the macro stream's name is looked for, not parsed.

use crate::attachment::is_executable_name;

/// Entries read from one archive's directory, at most.
const ENTRIES_MAX: usize = 10_000;

/// Whether this attachment carries code: named like something the machine
/// runs, or hiding one inside.
#[must_use]
pub fn carries_code(name: &str, bytes: &[u8]) -> bool {
    is_executable_name(name) || zip_hides_code(bytes) || ole_has_macros(bytes)
}

/// Whether any of `(name, bytes)` carries code.
#[must_use]
pub fn any_carries_code<'a>(parts: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> bool {
    parts.into_iter().any(|(n, b)| carries_code(n, b))
}

/// A zip (or an OOXML document, which is one) whose directory names an
/// executable, a macro project, or an entry it encrypted: a password-
/// protected archive is how a payload gets past every scanner, and the
/// password is in the letter.
fn zip_hides_code(bytes: &[u8]) -> bool {
    let Some(entries) = central_directory(bytes) else {
        return false;
    };
    entries.iter().any(|(name, encrypted)| {
        *encrypted
            || is_executable_name(name)
            || name
                .rsplit('/')
                .next()
                .is_some_and(|base| base.eq_ignore_ascii_case("vbaProject.bin"))
    })
}

/// The names in a zip's central directory, and whether each is encrypted.
/// `None` when this is not a zip.
fn central_directory(b: &[u8]) -> Option<Vec<(String, bool)>> {
    if !b.starts_with(b"PK\x03\x04") || b.len() < 22 {
        return None;
    }
    // the end-of-directory record is in the last 64 KiB + 22 bytes
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
        out.push((String::from_utf8_lossy(name).to_string(), flags & 1 == 1));
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

/// An Office file from before 2007 (OLE compound file) whose directory
/// names the macro project stream.
fn ole_has_macros(b: &[u8]) -> bool {
    const OLE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
    if !b.starts_with(OLE) {
        return false;
    }
    let name: Vec<u8> = "_VBA_PROJECT"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    b.windows(name.len()).any(|w| w == name.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with these `(name, encrypted)` entries, contents empty.
    fn zip(entries: &[(&str, bool)]) -> Vec<u8> {
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
        let dir_at = out.len() as u32;
        let dir_len = dir.len() as u32;
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

    #[test]
    fn an_archive_holding_a_program_carries_code() {
        assert!(carries_code(
            "invoice.zip",
            &zip(&[("readme.txt", false), ("invoice.pdf.exe", false)])
        ));
        assert!(!carries_code(
            "report.zip",
            &zip(&[("report.xml", false), ("data/a.csv", false)])
        ));
    }

    #[test]
    fn an_archive_that_hides_what_it_holds_carries_code() {
        assert!(carries_code("scan.zip", &zip(&[("scan.pdf", true)])));
    }

    #[test]
    fn a_macro_project_counts_whatever_the_document_is_called() {
        let docx = zip(&[
            ("[Content_Types].xml", false),
            ("word/vbaProject.bin", false),
        ]);
        assert!(carries_code("contract.docx", &docx));
        let plain = zip(&[("[Content_Types].xml", false), ("word/document.xml", false)]);
        assert!(!carries_code("contract.docx", &plain));
        let mut doc = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec();
        doc.extend_from_slice(&[0u8; 64]);
        doc.extend("_VBA_PROJECT".encode_utf16().flat_map(u16::to_le_bytes));
        assert!(carries_code("order.doc", &doc));
        let mut clean = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec();
        clean.extend_from_slice(&[0u8; 64]);
        assert!(!carries_code("order.doc", &clean));
    }

    #[test]
    fn what_is_not_an_archive_is_judged_by_its_name() {
        assert!(!carries_code("photo.jpg", b"\xFF\xD8\xFF\xE0 not a zip"));
        assert!(carries_code("run.exe", b"MZ"));
        // a truncated zip is not read past its end
        let mut z = zip(&[("a.exe", false)]);
        z.truncate(z.len() - 10);
        let _ = carries_code("cut.zip", &z);
        assert!(!carries_code("cut.zip", b"PK\x03\x04"));
    }
}
