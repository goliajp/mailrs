//! The detectors. Each reads at most `params.max_bytes` of a part and
//! returns what it found, or nothing.

mod archive;
mod html;
mod magic;
mod names;
mod office;
mod pdf;

use sha2::{Digest, Sha256};

use crate::{Attachment, Limits, PartFacts};

/// What a detector found.
pub(crate) struct Found {
    pub what: String,
    pub truncated: bool,
}

impl Found {
    pub fn new(what: impl Into<String>) -> Found {
        Found {
            what: what.into(),
            truncated: false,
        }
    }
}

/// The part's bytes up to the cap, and whether they were cut.
pub(crate) fn capped<'a>(a: &Attachment<'a>, p: &Limits) -> (&'a [u8], bool) {
    match a.bytes.len() > p.max_bytes {
        true => (&a.bytes[..p.max_bytes], true),
        false => (a.bytes, false),
    }
}

/// Every detector over one part.
pub(crate) fn facts(a: &Attachment<'_>, limits: &Limits) -> PartFacts {
    let mut truncated = false;
    let mut take = |f: Option<Found>| {
        f.map(|f| {
            truncated |= f.truncated;
            f.what
        })
    };
    let (html_base64_run, html_writer, html_cut) = html::smuggling_parts(a, limits);
    let mut out = PartFacts {
        name: a.name.to_string(),
        extension: names::extension(a.name),
        executable_name: names::executable(a.name),
        double_extension: names::double_extension(a.name),
        type_mismatch: take(magic::mismatch(a)),
        archive_executable: take(archive::holds_code(a)),
        encrypted_archive: take(archive::encrypted(a)),
        unlistable_archive: archive::unlistable(a),
        office_macros: take(office::macros(a, limits)),
        office_object: take(office::object(a, limits)),
        rtf_object: take(office::rtf_object(a, limits)),
        pdf_script: take(pdf::script(a, limits)),
        pdf_embedded: take(pdf::embedded(a, limits)),
        html_base64_run,
        html_writer,
        html_form: take(html::form(a, limits)),
        svg_script: take(html::svg_script(a, limits)),
        ..PartFacts::default()
    };
    out.truncated = truncated || html_cut;
    out
}

/// Lowercase hex sha256.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether `needle` occurs in `hay`, ignoring ASCII case.
pub(crate) fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && hay
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle))
}
