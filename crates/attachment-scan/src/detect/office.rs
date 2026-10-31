//! What an Office or RTF file embeds.

use super::{Found, capped, contains_ci};
use crate::{Attachment, Limits};

const OLE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A macro project: `vbaProject.bin` in an OOXML file, whatever its name,
/// or the `_VBA_PROJECT` stream named in an OLE file.
pub(crate) fn macros(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if let Some(entries) = super::archive::zip_entries(a.bytes) {
        let e = entries.iter().find(|e| {
            e.name
                .rsplit('/')
                .next()
                .is_some_and(|b| b.eq_ignore_ascii_case("vbaProject.bin"))
        })?;
        return Some(Found::new(format!("{} in {:?}", e.name, a.name)));
    }
    if !a.bytes.starts_with(OLE) {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let name = utf16("_VBA_PROJECT");
    b.windows(name.len())
        .any(|w| w == name.as_slice())
        .then(|| Found {
            what: format!("a macro project in {:?}", a.name),
            truncated,
        })
}

/// An embedded OLE object in an OOXML file (`…/embeddings/oleObject*.bin`)
/// or an `Ole10Native` package in an OLE file.
pub(crate) fn object(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if let Some(entries) = super::archive::zip_entries(a.bytes) {
        let e = entries.iter().find(|e| {
            let lower = e.name.to_ascii_lowercase();
            lower.contains("/embeddings/oleobject") && lower.ends_with(".bin")
        })?;
        return Some(Found::new(format!("{} in {:?}", e.name, a.name)));
    }
    if !a.bytes.starts_with(OLE) {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let name = utf16("Ole10Native");
    b.windows(name.len())
        .any(|w| w == name.as_slice())
        .then(|| Found {
            what: format!("an embedded package in {:?}", a.name),
            truncated,
        })
}

/// An RTF file with an embedded object.
pub(crate) fn rtf_object(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if !a.bytes.starts_with(b"{\\rt") {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let marker = [b"\\objdata".as_slice(), b"\\objupdate"]
        .into_iter()
        .find(|m| contains_ci(b, m))?;
    Some(Found {
        what: format!("{} in {:?}", String::from_utf8_lossy(marker), a.name),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::archive::tests::zip;

    fn att<'a>(name: &'a str, bytes: &'a [u8]) -> Attachment<'a> {
        Attachment {
            name,
            declared_type: "",
            bytes,
        }
    }

    fn ole_with(stream: &str) -> Vec<u8> {
        let mut b = OLE.to_vec();
        b.extend_from_slice(&[0u8; 64]);
        b.extend(utf16(stream));
        b
    }

    #[test]
    fn macros_whatever_the_document_is_called() {
        let p = Limits::default();
        let docx = zip(&[
            ("[Content_Types].xml", false),
            ("word/vbaProject.bin", false),
        ]);
        assert!(macros(&att("contract.docx", &docx), &p).is_some());
        let plain = zip(&[("[Content_Types].xml", false), ("word/document.xml", false)]);
        assert!(macros(&att("contract.docx", &plain), &p).is_none());
        assert!(macros(&att("order.doc", &ole_with("_VBA_PROJECT")), &p).is_some());
        assert!(macros(&att("order.doc", &ole_with("WordDocument")), &p).is_none());
    }

    #[test]
    fn the_cap_is_where_reading_stops() {
        let p = Limits { max_bytes: 16 };
        assert!(macros(&att("order.doc", &ole_with("_VBA_PROJECT")), &p).is_none());
    }

    #[test]
    fn embedded_objects() {
        let p = Limits::default();
        let docx = zip(&[("word/embeddings/oleObject1.bin", false)]);
        assert!(object(&att("a.docx", &docx), &p).is_some());
        assert!(object(&att("a.doc", &ole_with("Ole10Native")), &p).is_some());
        assert!(
            rtf_object(
                &att("a.rtf", b"{\\rtf1 {\\object\\objemb {\\*\\objdata 0105}}}"),
                &p
            )
            .is_some()
        );
        assert!(rtf_object(&att("a.rtf", b"{\\rtf1 plain text}"), &p).is_none());
    }
}
