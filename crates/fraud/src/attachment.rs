//! An attachment the operating system would run.
//!
//! This mailbox's threat is phishing, not malware — and the
//! measurement says so plainly. Of 1,268 attachments across 35,962
//! production messages, **one** was executable:
//! `RFQ-5086-26 TENDER.cab`, 139 KB, SPF fail, dressed as a
//! quotation request from a Hong Kong supplier. The other 1,267 are
//! PDFs, images, spreadsheets, calendar invitations and gzipped
//! DMARC reports.
//!
//! So the rule fires about once a year, and when it does it is right.
//! That is worth its line: the cost of holding a legitimate `.cab` is
//! one click, and the cost of delivering a real one is the machine.
//!
//! **Extension, not content.** Sniffing the bytes would be a better
//! check and a much larger one — an unpacker is an attack surface of
//! its own, and the file this caught announced itself in its name.
//! A sender who renames the file to `.pdf` defeats this and is caught
//! by the scanner instead; the two are not substitutes.

/// Extensions an operating system will execute, or hand to something
/// that will.
///
/// Three families, and each is here because it is a live delivery
/// vector rather than because it is exotic:
///
/// - **Executables** — `exe`, `com`, `scr`, `pif`, `msi`, `bat`,
///   `cmd`, `ps1`.
/// - **Scripts a shell or Windows host runs on double-click** —
///   `js`, `jse`, `vbs`, `vbe`, `wsf`, `hta`, `jar`, `xll`.
/// - **Containers that mount or unpack to one of the above** —
///   `cab`, `iso`, `img`, `lnk`.
/// - **Office documents that carry macros** — `docm`, `xlsm`, `pptm`.
///   The `x` forms (`docx`) cannot and are not here.
///
/// Not `.zip` or `.rar`: 79 archives in the corpus and 69 of them are
/// DMARC aggregate reports. An archive is a container for anything,
/// which makes it a container for nothing in particular.
pub const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", "com", "scr", "pif", "msi", "bat", "cmd", "ps1", "js", "jse", "vbs", "vbe", "wsf",
    "hta", "jar", "xll", "cab", "iso", "img", "lnk", "docm", "xlsm", "pptm",
];

/// Whether `filename` ends in an extension the machine would run.
///
/// Case-insensitive, and it reads the **last** extension —
/// `invoice.pdf.exe` is an `exe`, which is the whole trick.
#[must_use]
pub fn is_executable_name(filename: &str) -> bool {
    let name = filename.trim().trim_end_matches(['"', '\'', ' ']);
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let ext = ext.trim().to_ascii_lowercase();
    EXECUTABLE_EXTENSIONS.contains(&ext.as_str())
}

/// Whether any attachment in a parsed message would be executed.
///
/// Takes the names rather than the message, so this crate stays free
/// of a MIME parser and the two hosts — the receiver and the sweep —
/// hand it the same list from the one they already run. Two parsers
/// of one message is how a folded header came to be visible on one
/// path and not the other.
#[must_use]
pub fn any_executable<'a>(filenames: impl IntoIterator<Item = &'a str>) -> bool {
    filenames.into_iter().any(is_executable_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one the corpus caught, and the family it belongs to.
    #[test]
    fn the_one_that_arrived_is_caught() {
        assert!(is_executable_name("RFQ-5086-26 TENDER.cab"));
        for f in [
            "invoice.exe",
            "Scan_2026.js",
            "payment.HTA",
            "order.docm",
            "receipt.LNK",
            "statement.iso",
        ] {
            assert!(is_executable_name(f), "missed: {f}");
        }
    }

    /// The double extension is the trick, so the last one is what
    /// counts.
    #[test]
    fn the_last_extension_is_the_one_that_runs() {
        assert!(is_executable_name("invoice.pdf.exe"));
        assert!(!is_executable_name("invoice.exe.pdf"));
    }

    /// The other 1,267 in the corpus. A rule that held these would be
    /// worse than no rule.
    #[test]
    fn the_ordinary_attachments_are_left_alone() {
        for f in [
            "contract.pdf",
            "photo.JPG",
            "budget.xlsx",
            "notes.docx",
            "meeting.ics",
            "smime.p7s",
            "google.com!golia.jp!1773014400.xml.gz",
            "招商银行交易流水.zip",
            "README",
        ] {
            assert!(!is_executable_name(f), "wrongly caught: {f}");
        }
    }
}
