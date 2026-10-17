//! a font as the text extractor needs it: how to split a string into codes,
//! what each code means in unicode, and how far each code advances

use lopdf::{Dictionary, Document, Object};

use super::cjk::{self, Charset};
use super::cmap::CMap;
use super::objs;
use super::truetype::TrueType;

pub(super) enum CidCodes {
    Identity,
    Embedded(Box<CMap>),
    Charset(Charset),
}

pub(super) enum Kind {
    /// one byte per code; unicode per code resolved when the font is loaded
    Simple(Vec<Option<String>>),
    Composite {
        codes: CidCodes,
        /// glyph id per cid for an embedded truetype program, `None` = identity
        cid_to_gid: Option<Vec<u8>>,
        program: Option<Box<TrueType>>,
        /// cids are adobe-japan1 cids, so the collection's unicode table applies
        japan1: bool,
    },
}

pub(super) enum Widths {
    Simple {
        first: u32,
        widths: Vec<f64>,
        missing: f64,
    },
    Cid {
        default: f64,
        ranges: Vec<(u32, u32, f64)>,
    },
}

pub(super) struct Font {
    pub(super) kind: Kind,
    pub(super) to_unicode: Option<CMap>,
    pub(super) widths: Widths,
    /// glyph-space units to text-space units (0.001, or FontMatrix for type 3)
    pub(super) scale: f64,
    pub(super) vertical: bool,
}

/// One code taken off a shown string.
pub(super) struct Code {
    pub(super) value: u32,
    pub(super) len: usize,
}

impl Font {
    pub(super) fn load(doc: &Document, dict: &Dictionary) -> Font {
        let to_unicode = dict
            .get(b"ToUnicode")
            .ok()
            .and_then(|o| objs::stream_bytes(doc, o))
            .map(|b| CMap::parse(&b))
            .filter(|c| !c.is_empty());
        match objs::name(doc, dict, b"Subtype") {
            Some(b"Type0") => super::font_cid::load(doc, dict, to_unicode),
            _ => super::font_simple::load(doc, dict, to_unicode),
        }
    }

    pub(super) fn next_code(&self, bytes: &[u8]) -> Code {
        let (value, len) = match &self.kind {
            Kind::Simple(_) => (u32::from(bytes[0]), 1),
            Kind::Composite {
                codes: CidCodes::Identity,
                ..
            } => {
                let n = bytes.len().min(2);
                (
                    bytes[..n].iter().fold(0, |a, b| (a << 8) | u32::from(*b)),
                    n,
                )
            }
            Kind::Composite {
                codes: CidCodes::Embedded(cmap),
                ..
            } => cmap.next_code(bytes),
            Kind::Composite {
                codes: CidCodes::Charset(cs),
                ..
            } => {
                let n = cjk::code_len(*cs, bytes);
                (
                    bytes[..n]
                        .iter()
                        .fold(0u32, |a, b| (a << 8) | u32::from(*b)),
                    n,
                )
            }
        };
        Code { value, len }
    }

    fn cid(&self, code: &Code) -> Option<u32> {
        match &self.kind {
            Kind::Simple(_) => Some(code.value),
            Kind::Composite {
                codes: CidCodes::Identity,
                ..
            } => Some(code.value),
            Kind::Composite {
                codes: CidCodes::Embedded(cmap),
                ..
            } => cmap.cid(code.value, code.len as u8).or_else(|| {
                let identity = matches!(
                    cmap.use_cmap.as_deref(),
                    Some(b"Identity-H" | b"Identity-V")
                );
                identity.then_some(code.value)
            }),
            Kind::Composite {
                codes: CidCodes::Charset(_),
                ..
            } => None,
        }
    }

    /// Append the unicode text of `code` (whose bytes are `raw`) to `out`.
    pub(super) fn decode(&self, code: &Code, raw: &[u8], out: &mut String) {
        if let Some(tu) = &self.to_unicode
            && tu.write_text_any_len(code.value, code.len as u8, out)
        {
            return;
        }
        match &self.kind {
            Kind::Simple(table) => {
                if let Some(Some(s)) = table.get(code.value as usize) {
                    out.push_str(s);
                }
            }
            Kind::Composite {
                codes: CidCodes::Charset(cs),
                ..
            } => cjk::decode(*cs, raw, out),
            Kind::Composite {
                cid_to_gid,
                program,
                japan1,
                ..
            } => {
                let Some(cid) = self.cid(code) else { return };
                let gid = match cid_to_gid {
                    None => u16::try_from(cid).ok(),
                    Some(map) => {
                        let at = cid as usize * 2;
                        map.get(at..at + 2)
                            .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    }
                };
                if let Some(s) = program
                    .as_ref()
                    .zip(gid)
                    .and_then(|(tt, g)| tt.unicode_for_gid(g))
                {
                    out.push_str(&s);
                } else if *japan1 && let Some(c) = cjk::japan1(cid) {
                    out.push(c);
                }
            }
        }
    }

    /// Horizontal advance of `code` in text space for a font size of 1.
    pub(super) fn width(&self, code: &Code) -> f64 {
        let w = match &self.widths {
            Widths::Simple {
                first,
                widths,
                missing,
            } => code
                .value
                .checked_sub(*first)
                .and_then(|i| widths.get(i as usize))
                .copied()
                .unwrap_or(*missing),
            Widths::Cid { default, ranges } => self
                .cid(code)
                .and_then(|cid| {
                    let idx = ranges.partition_point(|r| r.0 <= cid);
                    ranges[..idx].last().filter(|r| cid <= r.1)
                })
                .map_or(*default, |r| r.2),
        };
        w * self.scale
    }
}

/// `/W` array of a cid font: `c [w1 w2 ...]` and `c_first c_last w` entries.
pub(super) fn cid_widths(doc: &Document, w: &[Object]) -> Vec<(u32, u32, f64)> {
    let mut out = Vec::new();
    let mut i = 0;
    let int = |o: &Object| objs::deref(doc, o).and_then(objs::num).map(|v| v as u32);
    while i < w.len() {
        let Some(first) = int(&w[i]) else { break };
        match w.get(i + 1).and_then(|o| objs::deref(doc, o)) {
            Some(Object::Array(list)) => {
                for (k, v) in list.iter().enumerate() {
                    if let Some(v) = objs::deref(doc, v).and_then(objs::num) {
                        let c = first.saturating_add(k as u32);
                        out.push((c, c, v));
                    }
                }
                i += 2;
            }
            Some(last) => {
                let (Some(last), Some(v)) = (
                    objs::num(last),
                    w.get(i + 2)
                        .and_then(|o| objs::deref(doc, o))
                        .and_then(objs::num),
                ) else {
                    break;
                };
                out.push((first, last as u32, v));
                i += 3;
            }
            None => break,
        }
    }
    out.sort_by_key(|r| r.0);
    out
}
