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

/// Extensions an encrypted archive entry may carry and still be read as a
/// document: formats that cannot carry a macro under their plain name.
/// The legacy Office formats (`doc`, `xls`, `ppt`) and `rtf` can, and an
/// encrypted one cannot be looked into, so they are not here.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "xlsx", "pptx", "odt", "ods", "odp", "txt", "csv",
];

/// What one message's attachments hold, read once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inside {
    /// An attachment is named like something the machine runs, or hides
    /// one inside — [`carries_code`].
    pub carries_code: bool,
    /// An archive encrypts entries that are all documents, and nothing
    /// attached carries code: the password-in-the-letter habit banks and
    /// accountants share with malware.
    pub encrypted_document_archive: bool,
}

/// Whether this attachment carries code: named like something the machine
/// runs, or hiding one inside.
#[must_use]
pub fn carries_code(name: &str, bytes: &[u8]) -> bool {
    is_executable_name(name)
        || zip_reading(bytes).is_some_and(|z| z.hides_code)
        || ole_has_macros(bytes)
}

/// What `(name, bytes)` hold between them.
#[must_use]
pub fn read<'a>(parts: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Inside {
    let mut code = false;
    let mut encrypted_documents = false;
    for (name, bytes) in parts {
        code |= carries_code(name, bytes);
        encrypted_documents |= zip_reading(bytes).is_some_and(|z| z.encrypted_documents);
    }
    Inside {
        carries_code: code,
        encrypted_document_archive: encrypted_documents && !code,
    }
}

/// What a zip's directory says about it.
struct ZipReading {
    /// It names an executable or a macro project, or encrypts an entry
    /// whose name does not say it is a document. Entry names are
    /// readable whether or not the entry is encrypted; encryption hides
    /// the contents, so an encrypted archive inside, or an encrypted
    /// legacy Office file, could still hold anything.
    hides_code: bool,
    /// It encrypts at least one entry, and every encrypted entry is a
    /// document.
    encrypted_documents: bool,
}

/// A zip (or an OOXML document, which is one) read from its central
/// directory. `None` when this is not a zip.
fn zip_reading(bytes: &[u8]) -> Option<ZipReading> {
    let entries = central_directory(bytes)?;
    let mut hides_code = false;
    let mut encrypted = false;
    for (name, is_encrypted) in &entries {
        if name.ends_with('/') {
            continue;
        }
        let base = name.rsplit('/').next().unwrap_or(name);
        hides_code |= is_executable_name(name)
            || base.eq_ignore_ascii_case("vbaProject.bin")
            || (*is_encrypted && !is_document_name(base));
        encrypted |= *is_encrypted;
    }
    Some(ZipReading {
        hides_code,
        encrypted_documents: encrypted && !hides_code,
    })
}

fn is_document_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && DOCUMENT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
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

    /// The bank statement of 2026-03-04: one encrypted PDF, the password
    /// in the letter. The names are readable, and they name a document.
    #[test]
    fn an_encrypted_archive_of_documents_carries_no_code() {
        assert!(!carries_code(
            "statement.zip",
            &zip(&[("statement.pdf", true)])
        ));
        // not in the corpus: a Japanese name, an upper-case extension
        assert!(!carries_code("明細.zip", &zip(&[("取引明細書.PDF", true)])));
        assert!(!carries_code(
            "payroll.zip",
            &zip(&[
                ("2026/", false),
                ("2026/payroll.xlsx", true),
                ("readme.txt", false)
            ])
        ));
    }

    #[test]
    fn an_encrypted_archive_of_documents_is_its_own_fact() {
        let statement = zip(&[("statement.pdf", true)]);
        let both = |parts: &[(&str, &[u8])]| {
            let r = read(parts.iter().copied());
            (r.carries_code, r.encrypted_document_archive)
        };
        assert_eq!(both(&[("statement.zip", &statement)]), (false, true));
        // nothing encrypted: an ordinary archive, neither fact
        let open = zip(&[("statement.pdf", false)]);
        assert_eq!(both(&[("statement.zip", &open)]), (false, false));
        // code anywhere in the message is the stronger fact, and the only one
        assert_eq!(
            both(&[("statement.zip", &statement), ("run.exe", b"MZ")]),
            (true, false)
        );
        let script = zip(&[("statement.pdf", true), ("open.hta", true)]);
        assert_eq!(both(&[("statement.zip", &script)]), (true, false));
        // a document with no stem, or no extension, is no document
        let nameless = zip(&[(".pdf", true)]);
        assert_eq!(both(&[("x.zip", &nameless)]), (true, false));
        assert_eq!(both(&[]), (false, false));
    }

    /// Encryption hides the contents, never the names: a script or a
    /// program inside is code whether or not it is encrypted.
    #[test]
    fn an_encrypted_program_is_still_code() {
        assert!(carries_code(
            "invoice.zip",
            &zip(&[("invoice.hta", true), ("readme.txt", false)])
        ));
        assert!(carries_code("order.zip", &zip(&[("order.pdf.pif", true)])));
    }

    /// What encryption hides and a name cannot vouch for still counts:
    /// an archive inside, a legacy Office file that can carry macros
    /// under its plain name, a file with no extension, a script in a
    /// script not on the executable list.
    #[test]
    fn an_encrypted_entry_that_is_not_a_document_still_counts() {
        for name in [
            "inner.zip",
            "inner.7z",
            "report.doc",
            "sheet.xls",
            "payload",
            "a.rtf",
        ] {
            assert!(
                carries_code("x.zip", &zip(&[(name, true)])),
                "{name} should still count"
            );
        }
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
