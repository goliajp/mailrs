//! RAR and 7z attachments, judged through the public entry points.
//!
//! The archives are headers only: the shapes of the nine RAR attachments
//! in this deployment's history (one RAR 5 member each, a script or a
//! program, attached under `.rar`, `.cab`, `.taz` or `.ace`), synthesized
//! here with no member data.

use super::*;

/// A RAR 5 variable-length integer.
pub(super) fn vint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        match v {
            0 => {
                out.push(byte);
                return out;
            }
            _ => out.push(byte | 0x80),
        }
    }
}

/// One RAR 5 block: crc (not checked), size, then `body` (type, flags and
/// fields), then `data` bytes of member data.
pub(super) fn rar5_block(body: &[u8], data: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 4];
    out.extend(vint(body.len() as u64));
    out.extend_from_slice(body);
    out.extend_from_slice(data);
    out
}

/// A RAR 5 file header for `name`; `encrypted` adds an encryption record
/// to its extra area.
pub(super) fn rar5_file(name: &str, directory: bool, encrypted: bool) -> Vec<u8> {
    let extra: Vec<u8> = match encrypted {
        // size 2: type 1 (encryption), version 0
        true => vec![2, 1, 0],
        false => Vec::new(),
    };
    let mut fields = Vec::new();
    fields.extend(vint(u64::from(directory))); // file flags
    fields.extend(vint(0)); // unpacked size
    fields.extend(vint(0x20)); // attributes
    fields.extend(vint(0)); // compression
    fields.extend(vint(0)); // host os: windows
    fields.extend(vint(name.len() as u64));
    fields.extend_from_slice(name.as_bytes());
    let mut body = vint(2); // type: file
    let flags = 0x02 | u64::from(!extra.is_empty()); // data area, extra area
    body.extend(vint(flags));
    if !extra.is_empty() {
        body.extend(vint(extra.len() as u64));
    }
    body.extend(vint(0)); // data size
    body.extend(fields);
    body.extend(extra);
    rar5_block(&body, &[])
}

/// A whole RAR 5 archive of these members, headers only.
pub(super) fn rar5(members: &[(&str, bool)]) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x01\x00".to_vec();
    // main header: type 1, no flags, archive flags 0
    out.extend(rar5_block(&[1, 0, 0], &[]));
    for (name, encrypted) in members {
        out.extend(rar5_file(name, false, *encrypted));
    }
    // end of archive: type 5, no flags, end flags 0
    out.extend(rar5_block(&[5, 0, 0], &[]));
    out
}

/// A RAR 4 block: crc, type, flags, size (7 + `rest`), then `rest`.
fn rar4_block(kind: u8, flags: u16, rest: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0, kind];
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&((7 + rest.len()) as u16).to_le_bytes());
    out.extend_from_slice(rest);
    out
}

/// A RAR 4 file header for a stored name (`raw`, which may be the
/// `oem NUL encoded` form when `flags` has 0x0200), no member data.
fn rar4_file(raw: &[u8], flags: u16) -> Vec<u8> {
    let mut rest = Vec::new();
    rest.extend_from_slice(&0u32.to_le_bytes()); // packed size
    rest.extend_from_slice(&0u32.to_le_bytes()); // unpacked size
    rest.push(2); // host os: windows
    rest.extend_from_slice(&[0u8; 8]); // crc, time
    rest.extend_from_slice(&[29, 0x30]); // version, method
    rest.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    rest.extend_from_slice(&0x20u32.to_le_bytes()); // attributes
    rest.extend_from_slice(raw);
    rar4_block(0x74, 0x8000 | flags, &rest)
}

/// A whole RAR 4 archive: main header with `main_flags`, the members.
fn rar4(main_flags: u16, members: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"Rar!\x1a\x07\x00".to_vec();
    out.extend(rar4_block(0x73, main_flags, &[0u8; 6]));
    for m in members {
        out.extend_from_slice(m);
    }
    out.extend(rar4_block(0x7b, 0x4000, &[]));
    out
}

/// The nine real attachments: the name each was sent under and the one
/// member inside it.
const HISTORY: &[(&str, &str)] = &[
    ("RFQ.cab", "RFQ.com"),
    ("order.taz", "order.bat"),
    ("invoice.rar", "invoice.js"),
    ("invoice.rar", "invoice.bat"),
    ("PO.rar", "PO.vbs"),
    ("PO.rar", "PO.bat"),
    ("shipping.rar", "shipping.bat"),
    ("quote.ace", "quote.bat"),
    ("quote.ace", "quote.exe"),
];

/// Red on the code before RAR was read: the program inside was invisible
/// unless the attachment's own name was executable (`.cab` is).
#[test]
fn a_program_inside_a_rar_carries_code_whatever_the_rar_is_called() {
    for (outer, inner) in HISTORY {
        assert!(
            carries_code(outer, &rar5(&[(inner, false)])),
            "{outer} holding {inner}"
        );
    }
}

fn facts(name: &str, bytes: &[u8]) -> (bool, bool, bool) {
    let r = read([(name, bytes)]);
    (
        r.carries_code,
        r.encrypted_document_archive,
        r.unreadable_archive,
    )
}

#[test]
fn a_rar_of_documents_passes_and_an_encrypted_one_scores() {
    assert_eq!(
        facts(
            "docs.rar",
            &rar5(&[("見積書.PDF", false), ("a.xlsx", false)])
        ),
        (false, false, false)
    );
    assert_eq!(
        facts("docs.rar", &rar5(&[("statement.pdf", true)])),
        (false, true, false)
    );
    // encrypted and not a document: anything could be in there
    assert_eq!(
        facts("x.rar", &rar5(&[("inner.7z", true)])),
        (true, false, false)
    );
}

#[test]
fn a_rar_4_is_read_like_a_rar_5() {
    let program = rar4(0, &[rar4_file(b"docs\\invoice.pdf.exe", 0)]);
    assert_eq!(facts("x.rar", &program), (true, false, false));
    let documents = rar4(0, &[rar4_file(b"invoice.pdf", 0), rar4_file(b"b.txt", 0)]);
    assert_eq!(facts("x.rar", &documents), (false, false, false));
    let encrypted = rar4(0, &[rar4_file(b"statement.pdf", 0x0004)]);
    assert_eq!(facts("x.rar", &encrypted), (false, true, false));
}

/// The name archivers show is the Unicode one; an OEM name that says
/// `.pdf` in front of a Unicode name that says `.exe` is a program.
#[test]
fn a_rar_4_unicode_name_is_the_one_judged() {
    // high byte 0, then op 0 (one low byte) four at a time
    let mut raw = b"a.pdf\0".to_vec();
    raw.extend_from_slice(&[0, 0x00, b'a', b'.', b'e', b'x', 0x00, b'e']);
    assert_eq!(
        facts("x.rar", &rar4(0, &[rar4_file(&raw, 0x0200)])),
        (true, false, false)
    );
    // not in the corpus: a Japanese name, every unit given in full (op 2)
    let name: Vec<u16> = "見積.exe".encode_utf16().collect();
    let mut raw = b"??.exe\0".to_vec();
    raw.push(0);
    for chunk in name.chunks(4) {
        raw.push(0xaa);
        for u in chunk {
            raw.extend_from_slice(&u.to_le_bytes());
        }
    }
    let rar = rar4(0, &[rar4_file(&raw, 0x0200)]);
    let listed = listing(&rar).expect("a rar");
    assert_eq!(listed.entries[0].name, "見積.exe");
    assert!(listed.whole);
}

#[test]
fn a_rar_with_encrypted_headers_scores_and_does_not_hold() {
    // RAR 5: an archive encryption header (type 4) before anything else
    let mut hp = b"Rar!\x1a\x07\x01\x00".to_vec();
    hp.extend(rar5_block(&[4, 0, 0, 0], &[0xee; 48]));
    assert_eq!(facts("invoice.rar", &hp), (false, false, true));
    // RAR 4: the main header's password flag
    let hp4 = rar4(0x0080, &[]);
    assert_eq!(facts("invoice.rar", &hp4), (false, false, true));
}

#[test]
fn a_rar_that_breaks_off_scores_and_what_it_named_still_counts() {
    let mut cut = b"Rar!\x1a\x07\x01\x00".to_vec();
    cut.extend(rar5_block(&[1, 0, 0], &[]));
    cut.extend(rar5_file("statement.pdf", false, false));
    // a file header declaring more data than there is
    let mut body = vint(2);
    body.extend(vint(0x02));
    body.extend(vint(1 << 40));
    cut.extend(rar5_block(&body, &[0; 16]));
    assert_eq!(facts("x.rar", &cut), (false, false, true));

    let mut after_a_program = rar5(&[("run.exe", false)]);
    after_a_program.truncate(after_a_program.len() - 4);
    assert_eq!(facts("x.rar", &after_a_program), (true, false, false));

    // nothing but the signature
    assert_eq!(
        facts("x.ace", b"Rar!\x1a\x07\x01\x00\xff"),
        (false, false, true)
    );
    // a rar attached among other files still says it could not be read
    let both = read([("x.rar", &b"Rar!\x1a\x07\x00"[..]), ("a.pdf", b"%PDF-1.7")]);
    assert!(both.unreadable_archive && !both.carries_code);
}

/// The names of a 7z are in a compressed header that is not decoded
/// here, so even a 7z carrying a program says nothing, and scores.
#[test]
fn a_7z_scores_and_does_not_hold_whatever_it_holds() {
    let mut seven = b"7z\xbc\xaf\x27\x1c\x00\x04".to_vec();
    seven.extend_from_slice(&[0u8; 24]);
    seven.extend_from_slice(b"invoice.exe");
    assert_eq!(facts("invoice.7z", &seven), (false, false, true));
    // called something else, it is still a 7z
    let r = read([("scan.pdf", &seven[..]), ("other.cab", &seven[..])]);
    assert_eq!(r.seven_z_archives, 2);
    // its own name is still judged
    assert_eq!(facts("invoice.7z.exe", &seven), (true, false, false));
    let rar = rar5(&[("a.pdf", false)]);
    assert_eq!(read([("a.rar", &rar[..])]).seven_z_archives, 0);
}

#[test]
fn what_is_not_an_archive_is_not_unreadable() {
    assert_eq!(
        facts("photo.jpg", b"\xff\xd8\xff\xe0"),
        (false, false, false)
    );
    assert_eq!(facts("note.rar", b"plain text"), (false, false, false));
    assert_eq!(facts("empty.rar", b""), (false, false, false));
}
