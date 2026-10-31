//! What a file's first bytes say it is.

use super::Found;
use crate::Attachment;

/// The kind of program or image the bytes are, when they are one.
fn program_kind(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"MZ") {
        return Some("a Windows program");
    }
    if b.starts_with(b"\x7fELF") {
        return Some("a Linux program");
    }
    let macho: [&[u8]; 4] = [
        b"\xfe\xed\xfa\xce",
        b"\xfe\xed\xfa\xcf",
        b"\xcf\xfa\xed\xfe",
        b"\xce\xfa\xed\xfe",
    ];
    if macho.iter().any(|m| b.starts_with(m)) {
        return Some("a macOS program");
    }
    if b.starts_with(b"\x4c\x00\x00\x00\x01\x14\x02\x00") {
        return Some("a Windows shortcut");
    }
    // ISO 9660: the volume descriptor at 32 KiB
    if b.get(0x8001..0x8006) == Some(b"CD001") {
        return Some("a disk image");
    }
    None
}

/// The bytes are a program or a disk image while the name says something
/// else; a program called a program is `executable_name`'s.
pub(crate) fn mismatch(a: &Attachment<'_>) -> Option<Found> {
    let kind = program_kind(a.bytes)?;
    if super::names::executable(a.name).is_some() {
        return None;
    }
    Some(Found::new(format!("{kind} named {:?}", a.name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att<'a>(name: &'a str, bytes: &'a [u8]) -> Attachment<'a> {
        Attachment {
            name,
            declared_type: "",
            bytes,
        }
    }

    #[test]
    fn a_program_named_as_a_document() {
        assert!(mismatch(&att("invoice.pdf", b"MZ\x90\x00")).is_some());
        assert!(mismatch(&att("photo.jpg", b"\x7fELF\x02")).is_some());
        let mut iso = vec![0u8; 0x8010];
        iso[0x8001..0x8006].copy_from_slice(b"CD001");
        assert!(mismatch(&att("scan.pdf", &iso)).is_some());
        // honest names, and things that are not programs
        assert!(mismatch(&att("setup.exe", b"MZ")).is_none());
        assert!(mismatch(&att("a.pdf", b"%PDF-1.7")).is_none());
        assert!(mismatch(&att("short", b"M")).is_none());
    }
}
