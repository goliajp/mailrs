//! simple fonts (Type1, TrueType, Type3): one byte per code, unicode from the
//! encoding, its /Differences, and the embedded program when the names fail

use lopdf::{Dictionary, Document, Object};

use super::cmap::CMap;
use super::encodings::{
    MAC_ROMAN_ENCODING, PDF_DOC_ENCODING, STANDARD_ENCODING, SYMBOL_ENCODING, WIN_ANSI_ENCODING,
};
use super::font::{Font, Kind, Widths};
use super::glyphname;
use super::objs;
use super::truetype::TrueType;
use super::type1::{self, Builtin};

/// width assumed when a font gives none (the standard 14 fonts may omit /Widths)
const FALLBACK_WIDTH: f64 = 500.0;

type Table = Vec<Option<String>>;

fn base_table(name: &[u8]) -> Option<Table> {
    let t = match name {
        b"StandardEncoding" => &STANDARD_ENCODING,
        b"WinAnsiEncoding" => &WIN_ANSI_ENCODING,
        b"MacRomanEncoding" => &MAC_ROMAN_ENCODING,
        b"PDFDocEncoding" => &PDF_DOC_ENCODING,
        b"SymbolEncoding" => &SYMBOL_ENCODING,
        _ => return None,
    };
    Some(
        t.iter()
            .map(|&u| {
                char::from_u32(u32::from(u))
                    .filter(|_| u != 0)
                    .map(String::from)
            })
            .collect(),
    )
}

struct Program {
    truetype: Option<TrueType>,
    builtin: Option<Builtin>,
}

fn program(doc: &Document, descriptor: Option<&Dictionary>) -> Program {
    let mut p = Program {
        truetype: None,
        builtin: None,
    };
    let Some(desc) = descriptor else { return p };
    if let Some(bytes) = desc
        .get(b"FontFile2")
        .ok()
        .and_then(|o| objs::stream_bytes(doc, o))
    {
        p.truetype = TrueType::parse(&bytes);
    } else if let Ok(obj) = desc.get(b"FontFile3") {
        let open_type = objs::deref(doc, obj)
            .and_then(|o| o.as_stream().ok())
            .is_some_and(|s| objs::name(doc, &s.dict, b"Subtype") == Some(b"OpenType"));
        if open_type && let Some(bytes) = objs::stream_bytes(doc, obj) {
            p.truetype = TrueType::parse(&bytes);
        }
    } else if let Some(bytes) = desc
        .get(b"FontFile")
        .ok()
        .and_then(|o| objs::stream_bytes(doc, o))
    {
        p.builtin = type1::builtin_encoding(&bytes);
    }
    p
}

fn implicit_table(subtype: &[u8], base_font: &[u8], symbolic: bool, prog: &Program) -> Table {
    match &prog.builtin {
        Some(Builtin::Custom(names)) => {
            let mut t = vec![None; 256];
            for (code, name) in names {
                t[usize::from(*code)] = glyphname::to_unicode(name);
            }
            return t;
        }
        Some(Builtin::Standard) => return base_table(b"StandardEncoding").unwrap_or_default(),
        None => {}
    }
    let named = |n: &[u8]| base_font.windows(n.len()).any(|w| w == n);
    if named(b"Symbol") {
        return base_table(b"SymbolEncoding").unwrap_or_default();
    }
    match subtype {
        b"Type3" => vec![None; 256],
        b"TrueType" if symbolic && prog.truetype.is_some() => vec![None; 256],
        b"TrueType" => base_table(b"WinAnsiEncoding").unwrap_or_default(),
        _ => base_table(b"StandardEncoding").unwrap_or_default(),
    }
}

fn apply_differences(doc: &Document, diffs: &[Object], table: &mut Table, tt: Option<&TrueType>) {
    let mut code: usize = 0;
    for o in diffs {
        match objs::deref(doc, o) {
            Some(Object::Integer(i)) => code = usize::try_from(*i).unwrap_or(usize::MAX),
            Some(Object::Name(n)) => {
                if let Some(slot) = table.get_mut(code) {
                    *slot =
                        glyphname::to_unicode(n).or_else(|| tt.and_then(|t| by_post_name(t, n)));
                }
                code = code.saturating_add(1);
            }
            _ => {}
        }
    }
}

fn by_post_name(tt: &TrueType, name: &[u8]) -> Option<String> {
    tt.gid_by_name(std::str::from_utf8(name).ok()?)
        .and_then(|g| tt.unicode_for_gid(g))
}

pub(super) fn load(doc: &Document, dict: &Dictionary, to_unicode: Option<CMap>) -> Font {
    let subtype = objs::name(doc, dict, b"Subtype").unwrap_or(b"Type1");
    let base_font = objs::name(doc, dict, b"BaseFont").unwrap_or(b"");
    let descriptor = objs::dict(doc, dict, b"FontDescriptor");
    let flags = descriptor
        .and_then(|d| objs::num_in(doc, d, b"Flags"))
        .unwrap_or(0.0) as i64;
    let symbolic = flags & 4 != 0;
    let prog = program(doc, descriptor);

    let enc = objs::get(doc, dict, b"Encoding");
    let enc_dict = match enc {
        Some(Object::Dictionary(d)) => Some(d),
        _ => None,
    };
    let base_name = match enc {
        Some(Object::Name(n)) => Some(n.as_slice()),
        _ => enc_dict.and_then(|d| objs::name(doc, d, b"BaseEncoding")),
    };
    let mut table = base_name
        .and_then(base_table)
        .unwrap_or_else(|| implicit_table(subtype, base_font, symbolic, &prog));
    if let Some(diffs) = enc_dict.and_then(|d| objs::array(doc, d, b"Differences")) {
        apply_differences(doc, diffs, &mut table, prog.truetype.as_ref());
    }
    if let Some(tt) = &prog.truetype
        && (symbolic || !tt.has_unicode_cmap() || tt.has_cmap(3, 0))
    {
        for (code, slot) in table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = tt
                    .gid_for_symbol_code(code as u8)
                    .and_then(|g| tt.unicode_for_gid(g));
            }
        }
    }

    let first = objs::num_in(doc, dict, b"FirstChar")
        .unwrap_or(0.0)
        .max(0.0) as u32;
    let widths: Vec<f64> = objs::array(doc, dict, b"Widths")
        .map(|a| {
            a.iter()
                .map(|o| objs::deref(doc, o).and_then(objs::num).unwrap_or(0.0))
                .collect()
        })
        .unwrap_or_default();
    let missing = descriptor
        .and_then(|d| objs::num_in(doc, d, b"MissingWidth"))
        .filter(|w| *w > 0.0 || !widths.is_empty())
        .unwrap_or(if widths.is_empty() {
            FALLBACK_WIDTH
        } else {
            0.0
        });
    let scale = match subtype {
        b"Type3" => objs::array(doc, dict, b"FontMatrix")
            .and_then(|m| m.first())
            .and_then(|o| objs::deref(doc, o))
            .and_then(objs::num)
            .map_or(0.001, f64::abs),
        _ => 0.001,
    };
    Font {
        kind: Kind::Simple(table),
        to_unicode,
        widths: Widths::Simple {
            first,
            widths,
            missing,
        },
        scale,
        vertical: false,
    }
}
