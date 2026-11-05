//! What an attachment hides inside it: an archive holding something the
//! machine would run, an archive that will not say what it holds, an
//! Office document carrying macros whatever its name says.
//!
//! **Listing, not unpacking.** A zip says what it holds in its central
//! directory, at the end of the file, uncompressed; a RAR in a header in
//! front of each member ([`rar`]); reading either is reading a table of
//! names. Nothing is decompressed, so a zip bomb is a table like any
//! other and the work is bounded by the attachment's size. An Office
//! file from before 2007 names its streams in UTF-16 inside the file,
//! and the macro stream's name is looked for, not parsed.
//!
//! **The format is the signature, not the name.** Of the nine RAR
//! attachments in this deployment's history (2026-11-05), four were
//! called `.cab`, `.taz` or `.ace`.
//!
//! **An archive whose names cannot be read scores and never holds.** A
//! 7z keeps its names in a compressed header, which is not decoded here:
//! the maintained pure-Rust reader sizes that decode from what the
//! archive declares, and history holds no 7z to measure a capped reader
//! against. A RAR made with header encryption, or one whose headers break
//! off before their end, is in the same place: something is in there and
//! nothing says what, which is worth a score and not a conviction.

use crate::attachment::is_executable_name;

mod rar;

/// Entries read from one archive's directory, at most.
const ENTRIES_MAX: usize = 10_000;

const SEVEN_Z: &[u8] = b"7z\xbc\xaf\x27\x1c";

/// Extensions an encrypted archive entry may carry and still be read as a
/// document. The legacy Office formats (`doc`, `xls`, `ppt`) and `rtf`
/// can carry a macro or an embedded object under their plain name, and an
/// encrypted one cannot be looked into; they are documents all the same,
/// so they score with the rest rather than hold. The macro-enabled forms
/// (`docm`, …) are executable names and hold.
const DOCUMENT_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "xlsx", "pptx", "odt", "ods", "odp", "txt", "csv", "doc", "xls", "ppt", "rtf",
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
    /// An archive whose entry names cannot all be read — a 7z, a RAR with
    /// header encryption, a RAR whose headers break off — and nothing
    /// attached carries code.
    pub unreadable_archive: bool,
    /// Attachments that are 7z archives by their signature, whatever
    /// they are called. None has arrived yet; the count is how the first
    /// one gets noticed.
    pub seven_z_archives: u32,
}

/// Whether this attachment carries code: named like something the machine
/// runs, or hiding one inside.
#[must_use]
pub fn carries_code(name: &str, bytes: &[u8]) -> bool {
    is_executable_name(name)
        || listing(bytes).is_some_and(|l| judge(&l).hides_code)
        || ole_has_macros(bytes)
}

/// Whether `bytes` is a RAR archive by its signature, whatever it is
/// called.
#[must_use]
pub fn is_rar(bytes: &[u8]) -> bool {
    rar::is_rar(bytes)
}

/// What `(name, bytes)` hold between them.
#[must_use]
pub fn read<'a>(parts: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Inside {
    let mut code = false;
    let mut encrypted_documents = false;
    let mut unreadable = false;
    let mut seven_z = 0u32;
    for (name, bytes) in parts {
        let judged = listing(bytes).map(|l| judge(&l));
        code |= is_executable_name(name)
            || judged.as_ref().is_some_and(|j| j.hides_code)
            || ole_has_macros(bytes);
        encrypted_documents |= judged.as_ref().is_some_and(|j| j.encrypted_documents);
        unreadable |= judged.as_ref().is_some_and(|j| j.unreadable);
        if bytes.starts_with(SEVEN_Z) {
            seven_z = seven_z.saturating_add(1);
        }
    }
    Inside {
        carries_code: code,
        encrypted_document_archive: encrypted_documents && !code,
        unreadable_archive: unreadable && !code,
        seven_z_archives: seven_z,
    }
}

/// One member of an archive, as its directory names it.
struct Entry {
    name: String,
    directory: bool,
    encrypted: bool,
}

/// What an archive's directory says, as far as it could be read.
#[derive(Default)]
struct Listing {
    entries: Vec<Entry>,
    /// The directory was read to its end. False when the names are
    /// encrypted, not decoded here, or the headers break off.
    whole: bool,
}

/// An archive's listing, or `None` when `bytes` is no archive read here.
fn listing(bytes: &[u8]) -> Option<Listing> {
    if let Some(entries) = central_directory(bytes) {
        let entries = entries
            .into_iter()
            .map(|(name, encrypted)| Entry {
                directory: name.ends_with('/'),
                name,
                encrypted,
            })
            .collect();
        return Some(Listing {
            entries,
            whole: true,
        });
    }
    if bytes.starts_with(SEVEN_Z) {
        return Some(Listing::default());
    }
    rar::listing(bytes)
}

/// What an archive's listing says about it.
struct Judged {
    /// It names an executable or a macro project, or encrypts an entry
    /// whose name does not say it is a document. Entry names are
    /// readable whether or not the entry is encrypted; encryption hides
    /// the contents, so an encrypted archive inside, or an encrypted file
    /// with no extension, could still hold anything.
    hides_code: bool,
    /// It encrypts at least one entry, and every encrypted entry is a
    /// document.
    encrypted_documents: bool,
    /// Not every name could be read.
    unreadable: bool,
}

fn judge(listing: &Listing) -> Judged {
    let mut hides_code = false;
    let mut encrypted = false;
    for entry in listing.entries.iter().filter(|e| !e.directory) {
        let name = entry.name.as_str();
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        hides_code |= is_executable_name(name)
            || base.eq_ignore_ascii_case("vbaProject.bin")
            || (entry.encrypted && !is_document_name(base));
        encrypted |= entry.encrypted;
    }
    Judged {
        hides_code,
        encrypted_documents: encrypted && !hides_code,
        unreadable: !listing.whole,
    }
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
#[path = "archive_tests.rs"]
mod archive_tests;

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

    /// A legacy Office file or an RTF, encrypted: it may carry a macro or
    /// an embedded object that nothing can look at, which is worth a
    /// score, and it is a document, which is not worth a hold.
    #[test]
    fn an_encrypted_legacy_office_file_scores_and_does_not_hold() {
        for name in [
            "report.doc",
            "sheet.XLS",
            "deck.ppt",
            "letter.rtf",
            "見積書.xls",
        ] {
            let r = read([("x.zip", &zip(&[(name, true)])[..])]);
            assert!(!r.carries_code, "{name} should not hold");
            assert!(r.encrypted_document_archive, "{name} should score");
        }
    }

    /// What encryption hides and a name cannot vouch for still counts:
    /// an archive inside, a file with no extension; and what runs counts
    /// encrypted or not — scripts, programs, the macro-enabled Office
    /// formats.
    #[test]
    fn an_encrypted_entry_that_is_not_a_document_still_counts() {
        for name in [
            "inner.zip",
            "inner.7z",
            "inner.rar",
            "payload",
            "invoice.hta",
            "invoice.pif",
            "tender.cab",
            "report.docm",
            "sheet.xlsm",
            "deck.pptm",
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
