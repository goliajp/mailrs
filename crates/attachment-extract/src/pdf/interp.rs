//! content stream interpreter: tracks the graphics and text state and hands
//! every shown glyph, with its device-space position, to the layout sink

use std::collections::HashMap;
use std::rc::Rc;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

use super::font::Font;
use super::layout::{Glyph, Sink};
use super::matrix::{self, M};
use super::objs;

/// operators executed per document, counting each run of a reused form
const MAX_OPS: usize = 20_000_000;
const MAX_FORM_DEPTH: usize = 16;
const MAX_SAVE_DEPTH: usize = 1024;

#[derive(Clone)]
struct State {
    ctm: M,
    font: Option<Rc<Font>>,
    size: f64,
    tc: f64,
    tw: f64,
    th: f64,
    tl: f64,
    rise: f64,
}

pub(super) struct Interp<'a> {
    doc: &'a Document,
    fonts: HashMap<usize, Rc<Font>>,
    pub(super) sink: Sink,
    ops_left: usize,
    text: String,
}

fn n(ops: &[Object], i: usize) -> f64 {
    ops.get(i)
        .and_then(objs::num)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

impl<'a> Interp<'a> {
    pub(super) fn new(doc: &'a Document) -> Self {
        Interp {
            doc,
            fonts: HashMap::new(),
            sink: Sink::new(),
            ops_left: MAX_OPS,
            text: String::new(),
        }
    }

    pub(super) fn run_page(&mut self, page: ObjectId) {
        let doc = self.doc;
        let resources = objs::inherited_resources(doc, page);
        let Ok(content) = doc.get_page_content_with_limit(page, objs::MAX_STREAM) else {
            return;
        };
        let state = State {
            ctm: matrix::IDENTITY,
            font: None,
            size: 0.0,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            tl: 0.0,
            rise: 0.0,
        };
        self.run(&content, &resources, state, &mut Vec::new());
    }

    fn run(
        &mut self,
        content: &[u8],
        resources: &[&'a Dictionary],
        state: State,
        forms: &mut Vec<usize>,
    ) {
        let Ok(content) = Content::decode(content) else {
            return;
        };
        let mut gs = state;
        let mut stack: Vec<State> = Vec::new();
        let mut dropped_saves = 0usize;
        let (mut tm, mut tlm) = (matrix::IDENTITY, matrix::IDENTITY);
        for Operation {
            operator,
            operands: ops,
        } in &content.operations
        {
            if self.ops_left == 0 || self.sink.full() {
                return;
            }
            self.ops_left -= 1;
            match operator.as_str() {
                "q" if stack.len() < MAX_SAVE_DEPTH => stack.push(gs.clone()),
                "q" => dropped_saves += 1,
                "Q" if dropped_saves > 0 => dropped_saves -= 1,
                "Q" => gs = stack.pop().unwrap_or(gs),
                "cm" => {
                    gs.ctm = matrix::mul(
                        &[
                            n(ops, 0),
                            n(ops, 1),
                            n(ops, 2),
                            n(ops, 3),
                            n(ops, 4),
                            n(ops, 5),
                        ],
                        &gs.ctm,
                    )
                }
                "BT" => (tm, tlm) = (matrix::IDENTITY, matrix::IDENTITY),
                "Tf" => {
                    gs.font = ops
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .and_then(|f| self.font(resources, f));
                    gs.size = n(ops, 1);
                }
                "Tc" => gs.tc = n(ops, 0),
                "Tw" => gs.tw = n(ops, 0),
                "Tz" => gs.th = n(ops, 0) / 100.0,
                "TL" => gs.tl = n(ops, 0),
                "Ts" => gs.rise = n(ops, 0),
                "Td" => (tm, tlm) = next_line(&tlm, n(ops, 0), n(ops, 1)),
                "TD" => {
                    gs.tl = -n(ops, 1);
                    (tm, tlm) = next_line(&tlm, n(ops, 0), n(ops, 1));
                }
                "Tm" => {
                    tlm = [
                        n(ops, 0),
                        n(ops, 1),
                        n(ops, 2),
                        n(ops, 3),
                        n(ops, 4),
                        n(ops, 5),
                    ];
                    tm = tlm;
                }
                "T*" => (tm, tlm) = next_line(&tlm, 0.0, -gs.tl),
                "Tj" => self.show_op(&gs, &mut tm, ops.first()),
                "'" => {
                    (tm, tlm) = next_line(&tlm, 0.0, -gs.tl);
                    self.show_op(&gs, &mut tm, ops.first());
                }
                "\"" => {
                    gs.tw = n(ops, 0);
                    gs.tc = n(ops, 1);
                    (tm, tlm) = next_line(&tlm, 0.0, -gs.tl);
                    self.show_op(&gs, &mut tm, ops.get(2));
                }
                "TJ" => {
                    for item in ops
                        .first()
                        .and_then(|o| o.as_array().ok())
                        .map_or(&[][..], Vec::as_slice)
                    {
                        match item {
                            Object::String(s, _) => self.show(&gs, &mut tm, s),
                            other => adjust(&gs, &mut tm, objs::num(other).unwrap_or(0.0)),
                        }
                    }
                }
                "Do" => {
                    if let Some(name) = ops.first().and_then(|o| o.as_name().ok()) {
                        self.form(resources, name, &gs, forms);
                    }
                }
                _ => {}
            }
        }
    }

    fn font(&mut self, resources: &[&'a Dictionary], name: &[u8]) -> Option<Rc<Font>> {
        let doc = self.doc;
        let dict = resources.iter().find_map(|r| {
            let fonts = objs::dict(doc, r, b"Font")?;
            objs::get(doc, fonts, name)?.as_dict().ok()
        })?;
        let key = dict as *const Dictionary as usize;
        let font = self
            .fonts
            .entry(key)
            .or_insert_with(|| Rc::new(Font::load(doc, dict)));
        Some(Rc::clone(font))
    }

    fn form(
        &mut self,
        resources: &[&'a Dictionary],
        name: &[u8],
        gs: &State,
        forms: &mut Vec<usize>,
    ) {
        let doc = self.doc;
        let Some(stream) = resources.iter().find_map(|r| {
            let xobjects = objs::dict(doc, r, b"XObject")?;
            objs::get(doc, xobjects, name)?.as_stream().ok()
        }) else {
            return;
        };
        let key = stream as *const lopdf::Stream as usize;
        if objs::name(doc, &stream.dict, b"Subtype") != Some(b"Form")
            || forms.len() >= MAX_FORM_DEPTH
            || forms.contains(&key)
        {
            return;
        }
        let Ok(content) = stream.decompressed_content_with_limit(objs::MAX_STREAM) else {
            return;
        };
        let mut inner: Vec<&'a Dictionary> = objs::dict(doc, &stream.dict, b"Resources")
            .into_iter()
            .collect();
        inner.extend_from_slice(resources);
        let mut state = gs.clone();
        state.ctm = matrix::mul(&objs::matrix(doc, stream.dict.get(b"Matrix").ok()), &gs.ctm);
        forms.push(key);
        self.run(&content, &inner, state, forms);
        forms.pop();
    }

    fn show_op(&mut self, gs: &State, tm: &mut M, op: Option<&Object>) {
        if let Some(Object::String(s, _)) = op {
            self.show(gs, tm, s);
        }
    }

    fn show(&mut self, gs: &State, tm: &mut M, bytes: &[u8]) {
        let Some(font) = gs.font.clone() else { return };
        let mut rest = bytes;
        while !rest.is_empty() {
            let code = font.next_code(rest);
            let raw = &rest[..code.len.clamp(1, rest.len())];
            rest = &rest[raw.len()..];
            self.text.clear();
            font.decode(&code, raw, &mut self.text);
            let m = matrix::mul(tm, &gs.ctm);
            let w0 = font.width(&code);
            let spacing = gs.tc
                + if code.value == 32 && raw.len() == 1 {
                    gs.tw
                } else {
                    0.0
                };
            let (adv, dir, step) = if font.vertical {
                (
                    (0.0, -gs.size),
                    matrix::dir(&m, (0.0, -1.0)),
                    (0.0, -gs.size + spacing),
                )
            } else {
                let adv = w0 * gs.size * gs.th;
                (
                    (adv, 0.0),
                    matrix::dir(&m, (1.0, 0.0)),
                    ((w0 * gs.size + spacing) * gs.th, 0.0),
                )
            };
            let glyph = Glyph {
                start: matrix::apply(&m, 0.0, gs.rise),
                end: matrix::apply(&m, adv.0, gs.rise + adv.1),
                dir,
                size: gs.size * matrix::scale(&m),
            };
            self.sink.glyph(&glyph, &self.text);
            *tm = matrix::translate(tm, step.0, step.1);
        }
    }
}

fn adjust(gs: &State, tm: &mut M, amount: f64) {
    if !amount.is_finite() {
        return;
    }
    let shift = -amount / 1000.0 * gs.size;
    *tm = if gs.font.as_ref().is_some_and(|f| f.vertical) {
        matrix::translate(tm, 0.0, shift)
    } else {
        matrix::translate(tm, shift * gs.th, 0.0)
    };
}

fn next_line(tlm: &M, tx: f64, ty: f64) -> (M, M) {
    let m = matrix::translate(tlm, tx, ty);
    (m, m)
}
