//! composite (Type0) fonts: codes split by the /Encoding cmap, unicode from
//! ToUnicode, a predefined cjk charset, or the embedded truetype program

use lopdf::{Dictionary, Document, Object};

use super::cjk;
use super::cmap::CMap;
use super::font::{CidCodes, Font, Kind, Widths, cid_widths};
use super::objs;
use super::truetype::TrueType;

fn is_vertical_name(n: &[u8]) -> bool {
    n == b"V" || n.ends_with(b"-V")
}

fn codes(doc: &Document, dict: &Dictionary) -> (CidCodes, bool) {
    match dict.get(b"Encoding").ok().and_then(|o| objs::deref(doc, o)) {
        Some(Object::Name(n)) => {
            let vertical = is_vertical_name(n);
            match cjk::charset_for(n) {
                Some(cs) => (CidCodes::Charset(cs), vertical),
                None => (CidCodes::Identity, vertical),
            }
        }
        Some(obj @ Object::Stream(_)) => {
            let Some(bytes) = objs::stream_bytes(doc, obj) else {
                return (CidCodes::Identity, false);
            };
            let cmap = CMap::parse(&bytes);
            let vertical = cmap.vertical;
            if let Some(cs) = cmap.use_cmap.as_deref().and_then(cjk::charset_for) {
                return (CidCodes::Charset(cs), vertical);
            }
            if cmap.has_codespace() {
                (CidCodes::Embedded(Box::new(cmap)), vertical)
            } else {
                (CidCodes::Identity, vertical)
            }
        }
        _ => (CidCodes::Identity, false),
    }
}

fn program(doc: &Document, descriptor: Option<&Dictionary>) -> Option<TrueType> {
    let desc = descriptor?;
    let obj = desc.get(b"FontFile2").ok().or_else(|| {
        let ff3 = desc.get(b"FontFile3").ok()?;
        let s = objs::deref(doc, ff3)?.as_stream().ok()?;
        (objs::name(doc, &s.dict, b"Subtype") == Some(b"OpenType")).then_some(ff3)
    })?;
    TrueType::parse(&objs::stream_bytes(doc, obj)?)
}

pub(super) fn load(doc: &Document, dict: &Dictionary, to_unicode: Option<CMap>) -> Font {
    let (codes, vertical) = codes(doc, dict);
    let descendant = objs::array(doc, dict, b"DescendantFonts")
        .and_then(|a| a.first())
        .and_then(|o| objs::deref(doc, o))
        .and_then(|o| o.as_dict().ok());
    let descriptor = descendant.and_then(|d| objs::dict(doc, d, b"FontDescriptor"));
    let cid_to_gid = descendant
        .and_then(|d| d.get(b"CIDToGIDMap").ok())
        .and_then(|o| objs::stream_bytes(doc, o));
    let default = descendant
        .and_then(|d| objs::num_in(doc, d, b"DW"))
        .unwrap_or(1000.0);
    let ranges = descendant
        .and_then(|d| objs::array(doc, d, b"W"))
        .map(|w| cid_widths(doc, w))
        .unwrap_or_default();
    let ordering = descendant
        .and_then(|d| objs::dict(doc, d, b"CIDSystemInfo"))
        .and_then(|i| objs::get(doc, i, b"Ordering"))
        .and_then(|o| o.as_str().ok());
    let cff = descendant.and_then(|d| objs::name(doc, d, b"Subtype")) == Some(b"CIDFontType0");
    // with an identity cid-to-gid map a truetype cid is a glyph id, whatever the ordering says
    let japan1 = ordering == Some(b"Japan1") && (cff || cid_to_gid.is_some());
    Font {
        kind: Kind::Composite {
            codes,
            cid_to_gid,
            program: program(doc, descriptor).map(Box::new),
            japan1,
        },
        to_unicode,
        widths: Widths::Cid { default, ranges },
        scale: 0.001,
        vertical,
    }
}
