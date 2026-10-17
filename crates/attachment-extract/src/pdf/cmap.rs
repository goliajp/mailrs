//! cmap programs: codespace ranges, bfchar/bfrange (ToUnicode) and cidchar/cidrange (encodings)

use std::collections::HashMap;

use super::cmap_lex::{Lexer, Token};

#[derive(Clone)]
enum Target {
    Text(String),
    Cid(u32),
}

#[derive(Clone)]
enum RangeTarget {
    /// utf-16 units; the last unit is incremented across the range
    Text(Vec<u16>),
    Array(Vec<String>),
    Cid(u32),
}

#[derive(Clone)]
struct Range {
    len: u8,
    lo: u32,
    hi: u32,
    target: RangeTarget,
}

#[derive(Default, Clone)]
pub(super) struct CMap {
    codespace: Vec<(u8, u32, u32)>,
    chars: HashMap<(u8, u32), Target>,
    ranges: Vec<Range>,
    /// name given to `usecmap`, e.g. `Identity-H`
    pub(super) use_cmap: Option<Vec<u8>>,
    pub(super) vertical: bool,
}

/// Big-endian integer of a 1–4 byte code.
fn code_of(bytes: &[u8]) -> Option<(u8, u32)> {
    if bytes.is_empty() || bytes.len() > 4 {
        return None;
    }
    let v = bytes.iter().fold(0u32, |acc, b| (acc << 8) | u32::from(*b));
    Some((bytes.len() as u8, v))
}

fn utf16_text(bytes: &[u8]) -> String {
    let units = utf16_units(bytes);
    char::decode_utf16(units).filter_map(Result::ok).collect()
}

fn utf16_units(bytes: &[u8]) -> Vec<u16> {
    if bytes.len() == 1 {
        return vec![u16::from(bytes[0])];
    }
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

fn text_target(tok: &Token) -> Option<String> {
    match tok {
        Token::Hex(b) => Some(utf16_text(b)),
        Token::Name(n) => super::glyphname::to_unicode(n),
        _ => None,
    }
}

impl CMap {
    pub(super) fn parse(data: &[u8]) -> CMap {
        let mut cmap = CMap::default();
        let toks: Vec<Token> = Lexer::new(data).collect();
        let mut i = 0;
        while i < toks.len() {
            let Token::Keyword(k) = &toks[i] else {
                i += 1;
                continue;
            };
            i += 1;
            match k.as_slice() {
                b"begincodespacerange" => i = cmap.codespace_section(&toks, i),
                b"beginbfchar" => i = cmap.char_section(&toks, i, b"endbfchar"),
                b"begincidchar" => i = cmap.char_section(&toks, i, b"endcidchar"),
                b"beginbfrange" => i = cmap.range_section(&toks, i, b"endbfrange"),
                b"begincidrange" => i = cmap.range_section(&toks, i, b"endcidrange"),
                b"usecmap" => {
                    if let Some(Token::Name(n)) = i.checked_sub(2).and_then(|j| toks.get(j)) {
                        cmap.use_cmap = Some(n.clone());
                    }
                }
                _ => {}
            }
        }
        cmap.vertical = has_wmode_1(&toks);
        cmap.ranges.sort_by_key(|r| (r.len, r.lo));
        cmap
    }

    fn codespace_section(&mut self, toks: &[Token], mut i: usize) -> usize {
        while let (Some(Token::Hex(lo)), Some(Token::Hex(hi))) = (toks.get(i), toks.get(i + 1)) {
            if let (Some((n, lo)), Some((m, hi))) = (code_of(lo), code_of(hi))
                && n == m
            {
                self.codespace.push((n, lo, hi));
            }
            i += 2;
        }
        i
    }

    fn char_section(&mut self, toks: &[Token], mut i: usize, end: &[u8]) -> usize {
        while let Some(tok) = toks.get(i) {
            if matches!(tok, Token::Keyword(k) if k == end) {
                return i + 1;
            }
            let (Token::Hex(src), Some(dst)) = (tok, toks.get(i + 1)) else {
                i += 1;
                continue;
            };
            let target = match dst {
                Token::Int(c) => u32::try_from(*c).ok().map(Target::Cid),
                other => text_target(other).map(Target::Text),
            };
            if let (Some(code), Some(t)) = (code_of(src), target) {
                self.chars.insert(code, t);
            }
            i += 2;
        }
        i
    }

    fn range_section(&mut self, toks: &[Token], mut i: usize, end: &[u8]) -> usize {
        while let Some(tok) = toks.get(i) {
            if matches!(tok, Token::Keyword(k) if k == end) {
                return i + 1;
            }
            let (Token::Hex(lo), Some(Token::Hex(hi))) = (tok, toks.get(i + 1)) else {
                i += 1;
                continue;
            };
            i += 2;
            let target = match toks.get(i) {
                Some(Token::Int(c)) => u32::try_from(*c).ok().map(RangeTarget::Cid),
                Some(Token::Hex(b)) => Some(RangeTarget::Text(utf16_units(b))),
                Some(Token::ArrayStart) => {
                    let mut items = Vec::new();
                    while let Some(t) = toks.get(i + 1) {
                        if matches!(t, Token::ArrayEnd) {
                            break;
                        }
                        items.push(text_target(t).unwrap_or_default());
                        i += 1;
                    }
                    i += 1;
                    Some(RangeTarget::Array(items))
                }
                _ => None,
            };
            i += 1;
            if let (Some((n, lo)), Some((m, hi)), Some(target)) = (code_of(lo), code_of(hi), target)
                && n == m
                && lo <= hi
            {
                self.ranges.push(Range {
                    len: n,
                    lo,
                    hi,
                    target,
                });
            }
        }
        i
    }

    pub(super) fn is_empty(&self) -> bool {
        self.chars.is_empty() && self.ranges.is_empty()
    }

    pub(super) fn has_codespace(&self) -> bool {
        !self.codespace.is_empty()
    }

    /// Split the next code off `bytes` by the codespace ranges; returns the
    /// code and its byte length (at least 1 when `bytes` is non-empty).
    pub(super) fn next_code(&self, bytes: &[u8]) -> (u32, usize) {
        for n in 1..=4usize.min(bytes.len()) {
            if let Some((len, code)) = code_of(&bytes[..n])
                && self
                    .codespace
                    .iter()
                    .any(|&(l, lo, hi)| l == len && lo <= code && code <= hi)
            {
                return (code, n);
            }
        }
        let n = self
            .codespace
            .iter()
            .map(|&(l, _, _)| usize::from(l))
            .min()
            .unwrap_or(1)
            .clamp(1, bytes.len().max(1));
        let code = code_of(bytes.get(..n).unwrap_or(&[])).map_or(0, |(_, c)| c);
        (code, n)
    }

    fn find(&self, len: u8, code: u32) -> Option<(&Range, u32)> {
        let idx = self
            .ranges
            .partition_point(|r| (r.len, r.lo) <= (len, code));
        self.ranges[..idx]
            .iter()
            .rev()
            .take(8)
            .take_while(|r| r.len == len)
            .find(|r| code <= r.hi)
            .map(|r| (r, code - r.lo))
    }

    /// Append the unicode text for a code of `len` bytes; false when unmapped.
    pub(super) fn write_text(&self, code: u32, len: u8, out: &mut String) -> bool {
        if let Some(Target::Text(s)) = self.chars.get(&(len, code)) {
            out.push_str(s);
            return true;
        }
        let Some((range, off)) = self.find(len, code) else {
            return false;
        };
        match &range.target {
            RangeTarget::Text(units) => {
                let mut units = units.clone();
                if let Some(last) = units.last_mut() {
                    *last = last.wrapping_add(off as u16);
                }
                out.extend(char::decode_utf16(units).filter_map(Result::ok));
                true
            }
            RangeTarget::Array(items) => match items.get(off as usize) {
                Some(s) => {
                    out.push_str(s);
                    true
                }
                None => false,
            },
            RangeTarget::Cid(_) => false,
        }
    }

    /// Like `write_text`, but also accepts entries written with another
    /// code length (producers sometimes write `<20>` for a two-byte code).
    pub(super) fn write_text_any_len(&self, code: u32, len: u8, out: &mut String) -> bool {
        self.write_text(code, len, out)
            || (1..=4u8)
                .filter(|&l| l != len)
                .any(|l| self.write_text(code, l, out))
    }

    pub(super) fn cid(&self, code: u32, len: u8) -> Option<u32> {
        if let Some(Target::Cid(c)) = self.chars.get(&(len, code)) {
            return Some(*c);
        }
        match self.find(len, code) {
            Some((
                Range {
                    target: RangeTarget::Cid(c),
                    ..
                },
                off,
            )) => c.checked_add(off),
            _ => None,
        }
    }
}

fn has_wmode_1(toks: &[Token]) -> bool {
    toks.windows(2)
        .any(|w| matches!(&w[0], Token::Name(n) if n == b"WMode") && matches!(w[1], Token::Int(1)))
}
