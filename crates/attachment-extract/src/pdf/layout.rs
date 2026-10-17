//! turns positioned glyphs into text: a space where a gap opens along the
//! writing direction, a newline where the baseline moves

/// text kept per document; the rest of a hostile or huge file is dropped
const MAX_TEXT: usize = 32 << 20;
/// gap along the line, in font sizes, that reads as a word break
const SPACE_GAP: f64 = 0.15;
/// baseline shift, in font sizes, that reads as a new line
const LINE_SHIFT: f64 = 0.5;

pub(super) struct Glyph {
    pub(super) start: (f64, f64),
    pub(super) end: (f64, f64),
    /// unit vector of the writing direction in device space
    pub(super) dir: (f64, f64),
    pub(super) size: f64,
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Break {
    None,
    Space,
    Line,
}

pub(super) struct Sink {
    pub(super) out: String,
    prev: Option<((f64, f64), f64)>,
    /// break seen at an unmapped glyph, applied before the next visible one
    pending: Break,
}

impl Sink {
    pub(super) fn new() -> Self {
        Sink {
            out: String::new(),
            prev: None,
            pending: Break::None,
        }
    }

    pub(super) fn full(&self) -> bool {
        self.out.len() >= MAX_TEXT
    }

    pub(super) fn page_break(&mut self) {
        self.out.push('\u{000C}');
        self.prev = None;
        self.pending = Break::None;
    }

    fn break_before(&self, g: &Glyph) -> Break {
        let Some((end, size)) = self.prev else {
            return Break::None;
        };
        let (dx, dy) = (g.start.0 - end.0, g.start.1 - end.1);
        let along = dx * g.dir.0 + dy * g.dir.1;
        let across = dy * g.dir.0 - dx * g.dir.1;
        let em = g.size.abs().max(size.abs());
        if across.abs() > LINE_SHIFT * em {
            Break::Line
        } else if along > SPACE_GAP * em || along < -3.0 * em {
            Break::Space
        } else {
            Break::None
        }
    }

    pub(super) fn glyph(&mut self, g: &Glyph, text: &str) {
        if self.full() {
            return;
        }
        let brk = self.break_before(g);
        let brk = if brk > self.pending {
            brk
        } else {
            self.pending
        };
        self.prev = Some((g.end, g.size));
        // an unmapped glyph still occupies its place on the line
        if text.is_empty() {
            self.pending = brk;
            return;
        }
        self.pending = Break::None;
        match brk {
            Break::Line => self.newline(),
            Break::Space => self.space(text),
            Break::None => {}
        }
        self.out.push_str(text);
    }

    fn newline(&mut self) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with(['\n', '\u{000C}']) {
            self.out.push('\n');
        }
    }

    fn space(&mut self, next: &str) {
        let ends_blank = self.out.chars().next_back().is_none_or(char::is_whitespace);
        if !ends_blank && !next.starts_with(char::is_whitespace) {
            self.out.push(' ');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(x: f64, y: f64, w: f64) -> Glyph {
        Glyph {
            start: (x, y),
            end: (x + w, y),
            dir: (1.0, 0.0),
            size: 10.0,
        }
    }

    #[test]
    fn spaces_and_lines() {
        let mut s = Sink::new();
        s.glyph(&g(0.0, 100.0, 5.0), "a");
        s.glyph(&g(5.0, 100.0, 5.0), "b");
        s.glyph(&g(13.0, 100.0, 5.0), "c");
        s.glyph(&g(0.0, 88.0, 5.0), "d");
        s.page_break();
        s.glyph(&g(0.0, 100.0, 5.0), "e");
        assert_eq!(s.out, "ab c\nd\u{000C}e");
    }

    #[test]
    fn vertical_columns() {
        let mut s = Sink::new();
        let v = |x: f64, y: f64| Glyph {
            start: (x, y),
            end: (x, y - 10.0),
            dir: (0.0, -1.0),
            size: 10.0,
        };
        s.glyph(&v(100.0, 100.0), "縦");
        s.glyph(&v(100.0, 90.0), "書");
        s.glyph(&v(85.0, 100.0), "き");
        assert_eq!(s.out, "縦書\nき");
    }
}
