//! lenient object access: every lookup on attacker-supplied structure is an `Option`

use lopdf::{Dictionary, Document, Object, ObjectId};

/// largest decoded size accepted for any one stream (content, font, cmap)
pub(super) const MAX_STREAM: usize = 64 << 20;
const MAX_TREE_DEPTH: usize = 64;

pub(super) fn deref<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    doc.dereference(obj).ok().map(|(_, o)| o)
}

pub(super) fn get<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    deref(doc, dict.get(key).ok()?)
}

pub(super) fn dict<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a Dictionary> {
    match get(doc, dict, key)? {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

pub(super) fn name<'a>(doc: &'a Document, dict: &'a Dictionary, key: &[u8]) -> Option<&'a [u8]> {
    get(doc, dict, key)?.as_name().ok()
}

pub(super) fn array<'a>(
    doc: &'a Document,
    dict: &'a Dictionary,
    key: &[u8],
) -> Option<&'a [Object]> {
    get(doc, dict, key)?.as_array().ok().map(Vec::as_slice)
}

pub(super) fn num(obj: &Object) -> Option<f64> {
    match obj {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(f64::from(*r)),
        _ => None,
    }
}

pub(super) fn num_in(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<f64> {
    num(get(doc, dict, key)?)
}

/// Decoded bytes of a stream object, `None` when it is not a stream, uses an
/// unsupported filter, or would exceed `MAX_STREAM`.
pub(super) fn stream_bytes(doc: &Document, obj: &Object) -> Option<Vec<u8>> {
    match deref(doc, obj)? {
        Object::Stream(s) => s.decompressed_content_with_limit(MAX_STREAM).ok(),
        _ => None,
    }
}

/// `[a b c d e f]` of a matrix array, identity when malformed.
pub(super) fn matrix(doc: &Document, obj: Option<&Object>) -> [f64; 6] {
    let parsed = obj
        .and_then(|o| deref(doc, o))
        .and_then(|o| o.as_array().ok())
        .filter(|a| a.len() == 6)
        .and_then(|a| {
            let mut m = [0.0; 6];
            for (slot, o) in m.iter_mut().zip(a) {
                *slot = num(deref(doc, o)?)?;
            }
            Some(m)
        });
    parsed.unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
}

/// /Resources of the page and of every ancestor, nearest first; the walk
/// stops at a repeated or overly deep node.
pub(super) fn inherited_resources(doc: &Document, page: ObjectId) -> Vec<&Dictionary> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let mut node = doc.get_dictionary(page).ok();
    while let Some(dict) = node {
        out.extend(self::dict(doc, dict, b"Resources"));
        let parent = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
        node = match parent {
            Some(id) if !seen.contains(&id) && seen.len() < MAX_TREE_DEPTH => {
                seen.push(id);
                doc.get_dictionary(id).ok()
            }
            _ => None,
        };
    }
    out
}
