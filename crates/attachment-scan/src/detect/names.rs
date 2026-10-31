//! What a file name says.

/// Extensions an operating system runs, or hands to something that will:
/// programs, scripts a Windows host runs on double-click, containers that
/// mount or unpack to one, and Office documents that carry macros.
pub(crate) const EXECUTABLE: &[&str] = &[
    "exe", "com", "scr", "pif", "msi", "bat", "cmd", "ps1", "js", "jse", "vbs", "vbe", "wsf",
    "hta", "jar", "xll", "cab", "iso", "img", "lnk", "docm", "xlsm", "pptm", "vhd", "vhdx", "appx",
    "msix", "chm", "cpl", "reg", "scf", "url",
];

/// Extensions a reader takes for a document or a picture.
const DOCUMENT: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "rtf", "csv", "jpg", "jpeg", "png",
    "gif", "html", "htm", "zip",
];

fn last_extension(name: &str) -> Option<String> {
    let name = name.trim().trim_end_matches(['"', '\'', ' ', '.']);
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext.trim().to_ascii_lowercase())
}

/// The last extension, lowercased, or empty.
pub(crate) fn extension(name: &str) -> String {
    last_extension(name).unwrap_or_default()
}

/// The extension, when it is one the machine runs.
pub(crate) fn executable(name: &str) -> Option<String> {
    let ext = last_extension(name)?;
    EXECUTABLE
        .contains(&ext.as_str())
        .then(|| format!(".{ext}"))
}

/// A name built to hide its real extension: a document extension before
/// a program one (`invoice.pdf.exe`), a run of spaces before it, or a
/// right-to-left override that reverses what is shown.
pub(crate) fn double_extension(name: &str) -> Option<String> {
    if name.contains(['\u{202E}', '\u{202D}', '\u{200F}']) {
        return Some("a right-to-left override in the name".into());
    }
    let real = executable(name)?;
    let stem = name.trim().trim_end_matches(['"', '\'', ' ', '.']);
    let stem = &stem[..stem.rfind('.')?];
    if stem.ends_with("     ") {
        return Some(format!("spaces before {real}"));
    }
    let inner = stem.rsplit_once('.')?.1.trim().to_ascii_lowercase();
    DOCUMENT
        .contains(&inner.as_str())
        .then(|| format!(".{inner}{real}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_by_name() {
        for n in [
            "RFQ-5086-26 TENDER.cab",
            "a.EXE",
            "x.js ",
            "\"q.iso\"",
            "r.vhdx",
        ] {
            assert!(executable(n).is_some(), "{n}");
        }
        for n in ["a.pdf", "noext", "a.exe.pdf", "report.docx"] {
            assert_eq!(executable(n), None, "{n}");
        }
    }

    #[test]
    fn hidden_extensions() {
        assert_eq!(
            double_extension("invoice.pdf.exe").as_deref(),
            Some(".pdf.exe")
        );
        assert_eq!(
            double_extension("photo.JPG.scr").as_deref(),
            Some(".jpg.scr")
        );
        assert!(double_extension("scan.pdf          .exe").is_some());
        assert!(double_extension("inv\u{202E}fdp.exe").is_some());
        assert_eq!(double_extension("setup.exe"), None);
        assert_eq!(double_extension("v1.2.exe"), None);
        assert_eq!(double_extension("report.final.pdf"), None);
    }
}
