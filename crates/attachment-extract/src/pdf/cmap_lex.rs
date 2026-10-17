//! tokenizer for the postscript subset that cmap programs are written in

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Token {
    Hex(Vec<u8>),
    Name(Vec<u8>),
    Int(i64),
    Keyword(Vec<u8>),
    ArrayStart,
    ArrayEnd,
    Other,
}

pub(super) struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

impl<'a> Lexer<'a> {
    pub(super) fn new(data: &'a [u8]) -> Self {
        Lexer { data, pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn word(&mut self) -> &'a [u8] {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_whitespace() || is_delim(b) {
                break;
            }
            self.pos += 1;
        }
        &self.data[start..self.pos]
    }

    fn hex(&mut self) -> Token {
        let mut out = Vec::new();
        let mut hi: Option<u8> = None;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'>' {
                break;
            }
            let Some(d) = (b as char).to_digit(16) else {
                continue;
            };
            match hi.take() {
                Some(h) => out.push((h << 4) | d as u8),
                None => hi = Some(d as u8),
            }
        }
        if let Some(h) = hi {
            out.push(h << 4);
        }
        Token::Hex(out)
    }

    fn skip_string(&mut self) {
        let mut depth = 1usize;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'\\' => self.pos += 1,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let b = self.peek()?;
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if b == b'%' {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                    self.pos += 1;
                }
                continue;
            }
            self.pos += 1;
            return Some(match b {
                b'<' if self.peek() == Some(b'<') => {
                    self.pos += 1;
                    Token::Other
                }
                b'<' => self.hex(),
                b'>' => {
                    if self.peek() == Some(b'>') {
                        self.pos += 1;
                    }
                    Token::Other
                }
                b'[' => Token::ArrayStart,
                b']' => Token::ArrayEnd,
                b'(' => {
                    self.skip_string();
                    Token::Other
                }
                b'/' => Token::Name(self.word().to_vec()),
                b'{' | b'}' | b')' => Token::Other,
                _ => {
                    self.pos -= 1;
                    let w = self.word();
                    if w.is_empty() {
                        self.pos += 1;
                        Token::Other
                    } else if let Some(n) = std::str::from_utf8(w).ok().and_then(|s| s.parse().ok())
                    {
                        Token::Int(n)
                    } else {
                        Token::Keyword(w.to_vec())
                    }
                }
            });
        }
    }
}
