//! the parts of an embedded truetype program that say which glyph is which
//! character: `cmap` subtables (formats 0, 4, 6, 12) and `post` glyph names

use std::collections::HashMap;

use super::macnames::MAC_GLYPH_NAMES;

/// cap on code points taken from one cmap subtable; a format 12 group can
/// claim the whole unicode range in twelve bytes
const MAX_SUBTABLE_ENTRIES: usize = 200_000;

pub(super) struct TrueType {
    /// (platform, encoding) -> code -> glyph id
    cmaps: Vec<((u16, u16), HashMap<u32, u16>)>,
    names: Vec<Option<String>>,
    reverse: HashMap<u16, char>,
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *d.get(at)?,
        *d.get(at + 1)?,
        *d.get(at + 2)?,
        *d.get(at + 3)?,
    ]))
}

fn table<'a>(d: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    let count = usize::from(u16_at(d, 4)?);
    (0..count).find_map(|i| {
        let rec = 12 + i * 16;
        if d.get(rec..rec + 4)? != tag {
            return None;
        }
        let off = u32_at(d, rec + 8)? as usize;
        let len = u32_at(d, rec + 12)? as usize;
        d.get(off..off.checked_add(len)?)
    })
}

fn subtable(d: &[u8]) -> Option<HashMap<u32, u16>> {
    let mut map = HashMap::new();
    match u16_at(d, 0)? {
        0 => {
            for c in 0..256usize {
                map.insert(c as u32, u16::from(*d.get(6 + c)?));
            }
        }
        4 => format4(d, &mut map)?,
        6 => {
            let first = u32::from(u16_at(d, 6)?);
            for i in 0..usize::from(u16_at(d, 8)?) {
                map.insert(first + i as u32, u16_at(d, 10 + 2 * i)?);
            }
        }
        12 => {
            for g in 0..u32_at(d, 12)? as usize {
                let at = 16 + g * 12;
                let (start, end, gid) = (u32_at(d, at)?, u32_at(d, at + 4)?, u32_at(d, at + 8)?);
                for c in start..=end.min(0x10FFFF) {
                    if map.len() >= MAX_SUBTABLE_ENTRIES {
                        return Some(map);
                    }
                    map.insert(c, (gid.wrapping_add(c - start)) as u16);
                }
            }
        }
        _ => return None,
    }
    Some(map)
}

fn format4(d: &[u8], map: &mut HashMap<u32, u16>) -> Option<()> {
    let seg_x2 = usize::from(u16_at(d, 6)?);
    let ends = 14;
    let starts = ends + seg_x2 + 2;
    let deltas = starts + seg_x2;
    let offsets = deltas + seg_x2;
    for s in 0..seg_x2 / 2 {
        let end = u16_at(d, ends + 2 * s)?;
        let start = u16_at(d, starts + 2 * s)?;
        let delta = u16_at(d, deltas + 2 * s)?;
        let ro_at = offsets + 2 * s;
        let ro = usize::from(u16_at(d, ro_at)?);
        if start > end || (start == 0xFFFF && end == 0xFFFF) {
            continue;
        }
        for c in start..=end {
            let gid = if ro == 0 {
                c.wrapping_add(delta)
            } else {
                let at = ro_at + ro + 2 * usize::from(c - start);
                match u16_at(d, at) {
                    Some(0) | None => continue,
                    Some(g) => g.wrapping_add(delta),
                }
            };
            if gid != 0 {
                map.insert(u32::from(c), gid);
            }
        }
    }
    Some(())
}

fn post_names(d: &[u8], num_glyphs: usize) -> Vec<Option<String>> {
    let named = |i: usize| MAC_GLYPH_NAMES.get(i).map(|s| s.to_string());
    match u32_at(d, 0) {
        Some(0x0001_0000) => (0..num_glyphs.min(258)).map(named).collect(),
        Some(0x0002_0000) => {
            let n = u16_at(d, 32).map_or(0, usize::from);
            let mut custom = Vec::new();
            let mut at = 34 + 2 * n;
            while let Some(&len) = d.get(at) {
                let s = d.get(at + 1..at + 1 + usize::from(len));
                custom.push(s.map(|s| String::from_utf8_lossy(s).into_owned()));
                at += 1 + usize::from(len);
            }
            (0..n)
                .map(|g| {
                    let idx = usize::from(u16_at(d, 34 + 2 * g)?);
                    match idx.checked_sub(258) {
                        None => named(idx),
                        Some(k) => custom.get(k).cloned().flatten(),
                    }
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

impl TrueType {
    pub(super) fn parse(data: &[u8]) -> Option<TrueType> {
        let cmap = table(data, b"cmap");
        let mut cmaps = Vec::new();
        if let Some(cmap) = cmap {
            for i in 0..usize::from(u16_at(cmap, 2)?) {
                let rec = 4 + i * 8;
                let (Some(p), Some(e), Some(off)) = (
                    u16_at(cmap, rec),
                    u16_at(cmap, rec + 2),
                    u32_at(cmap, rec + 4),
                ) else {
                    break;
                };
                if let Some(map) = cmap.get(off as usize..).and_then(subtable) {
                    cmaps.push(((p, e), map));
                }
            }
        }
        let num_glyphs = table(data, b"maxp")
            .and_then(|m| u16_at(m, 4))
            .map_or(0, usize::from);
        let names = table(data, b"post")
            .map(|p| post_names(p, num_glyphs))
            .unwrap_or_default();
        let mut reverse = HashMap::new();
        for ((p, e), map) in &cmaps {
            if *p == 0 || (*p == 3 && (*e == 1 || *e == 10)) {
                for (&c, &g) in map {
                    if let Some(ch) = char::from_u32(c) {
                        reverse
                            .entry(g)
                            .and_modify(|old: &mut char| *old = (*old).min(ch))
                            .or_insert(ch);
                    }
                }
            }
        }
        Some(TrueType {
            cmaps,
            names,
            reverse,
        })
    }

    pub(super) fn has_cmap(&self, platform: u16, encoding: u16) -> bool {
        self.cmaps.iter().any(|(k, _)| *k == (platform, encoding))
    }

    pub(super) fn has_unicode_cmap(&self) -> bool {
        !self.reverse.is_empty()
    }

    pub(super) fn gid(&self, platform: u16, encoding: u16, code: u32) -> Option<u16> {
        self.cmaps
            .iter()
            .find(|(k, _)| *k == (platform, encoding))
            .and_then(|(_, m)| m.get(&code).copied())
    }

    /// Glyph id of a single-byte code in a font without a usable pdf
    /// encoding: the (3,0) symbol cmap at 0xF000+code, then the (1,0) mac cmap.
    pub(super) fn gid_for_symbol_code(&self, code: u8) -> Option<u16> {
        let c = u32::from(code);
        self.gid(3, 0, 0xF000 | c)
            .or_else(|| self.gid(3, 0, c))
            .or_else(|| self.gid(1, 0, c))
    }

    pub(super) fn glyph_name(&self, gid: u16) -> Option<&str> {
        self.names.get(usize::from(gid))?.as_deref()
    }

    pub(super) fn gid_by_name(&self, name: &str) -> Option<u16> {
        let g = self.names.iter().position(|n| n.as_deref() == Some(name))?;
        u16::try_from(g).ok()
    }

    /// Unicode text of a glyph: the unicode cmap read backwards, then the post name.
    pub(super) fn unicode_for_gid(&self, gid: u16) -> Option<String> {
        if let Some(c) = self.reverse.get(&gid) {
            return Some(c.to_string());
        }
        self.glyph_name(gid)
            .and_then(|n| super::glyphname::to_unicode(n.as_bytes()))
    }
}
