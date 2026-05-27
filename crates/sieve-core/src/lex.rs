//! RFC 5228 §2 tokenizer.
//!
//! Single-pass, position-tracking lexer. Handles:
//!
//! - identifiers (`require`, `if`, `header`, …)
//! - tagged args (`:is`, `:contains`, `:domain`)
//! - quoted strings (with `\\` and `\"` escapes)
//! - multi-line strings (`text: … .CRLF`)
//! - numbers with K/M/G suffix (`100K`, `2M`)
//! - punctuation (`{`, `}`, `[`, `]`, `;`, `,`, `(`, `)`)
//! - comments (`# …` to EOL, `/* … */`)

use std::fmt;

/// One lexeme. `String` and `Number` carry the parsed value;
/// `Identifier` and `Tag` carry the slice content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// `require`, `if`, `header`, …
    Identifier(String),
    /// `:is`, `:domain`, … (tag character + identifier)
    Tag(String),
    /// Quoted or multi-line string content (after escape resolution).
    String(String),
    /// Integer with K/M/G suffix already applied (1K → 1024, etc.).
    Number(u64),
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
}

/// Lex failure mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenizeError {
    /// Unterminated quoted string starting at this byte offset.
    #[error("unterminated quoted string at byte {0}")]
    UnterminatedString(usize),
    /// Unterminated `/* … */` comment starting at this byte offset.
    #[error("unterminated block comment at byte {0}")]
    UnterminatedComment(usize),
    /// Unterminated multi-line `text:` literal starting at this byte offset.
    #[error("unterminated multi-line string at byte {0}")]
    UnterminatedMultiline(usize),
    /// Bad escape sequence inside a quoted string at this byte offset.
    #[error("bad escape at byte {0}")]
    BadEscape(usize),
    /// Tag character (`:`) not followed by an identifier at this byte offset.
    #[error("expected identifier after `:` at byte {0}")]
    BadTag(usize),
    /// Number literal followed by an unrecognised suffix at this byte offset.
    #[error("bad number suffix at byte {0}")]
    BadNumberSuffix(usize),
    /// Character outside the RFC 5228 lexical alphabet at this byte offset.
    #[error("unexpected character {1:?} at byte {0}")]
    Unexpected(usize, char),
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identifier(s) => write!(f, "{s}"),
            Self::Tag(s) => write!(f, ":{s}"),
            Self::String(s) => write!(f, "{s:?}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::LBrace => f.write_str("{"),
            Self::RBrace => f.write_str("}"),
            Self::LBracket => f.write_str("["),
            Self::RBracket => f.write_str("]"),
            Self::LParen => f.write_str("("),
            Self::RParen => f.write_str(")"),
            Self::Semicolon => f.write_str(";"),
            Self::Comma => f.write_str(","),
        }
    }
}

/// Tokenize a Sieve source string. Returns the token sequence
/// without any positional metadata — parser line/column tracking
/// is a v0.2 nicety.
pub fn tokenize(src: &str) -> Result<Vec<Token>, TokenizeError> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        let b = bytes[i];

        // whitespace
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            i += 1;
            continue;
        }

        // line comment
        if b == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // block comment /* ... */
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            loop {
                if i + 1 >= bytes.len() {
                    return Err(TokenizeError::UnterminatedComment(start));
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // punctuation
        match b {
            b'{' => {
                out.push(Token::LBrace);
                i += 1;
                continue;
            }
            b'}' => {
                out.push(Token::RBrace);
                i += 1;
                continue;
            }
            b'[' => {
                out.push(Token::LBracket);
                i += 1;
                continue;
            }
            b']' => {
                out.push(Token::RBracket);
                i += 1;
                continue;
            }
            b'(' => {
                out.push(Token::LParen);
                i += 1;
                continue;
            }
            b')' => {
                out.push(Token::RParen);
                i += 1;
                continue;
            }
            b';' => {
                out.push(Token::Semicolon);
                i += 1;
                continue;
            }
            b',' => {
                out.push(Token::Comma);
                i += 1;
                continue;
            }
            _ => {}
        }

        // quoted string "..."
        if b == b'"' {
            let start = i;
            i += 1;
            let mut s = String::new();
            loop {
                if i >= bytes.len() {
                    return Err(TokenizeError::UnterminatedString(start));
                }
                let c = bytes[i];
                if c == b'"' {
                    i += 1;
                    break;
                }
                if c == b'\\' {
                    if i + 1 >= bytes.len() {
                        return Err(TokenizeError::BadEscape(i));
                    }
                    let esc = bytes[i + 1];
                    match esc {
                        b'"' => s.push('"'),
                        b'\\' => s.push('\\'),
                        _ => return Err(TokenizeError::BadEscape(i)),
                    }
                    i += 2;
                    continue;
                }
                // append the actual UTF-8 code point starting at i
                let ch_start = i;
                let ch_len = utf8_char_len(c);
                let ch_end = ch_start + ch_len;
                if ch_end > bytes.len() {
                    return Err(TokenizeError::UnterminatedString(start));
                }
                s.push_str(std::str::from_utf8(&bytes[ch_start..ch_end]).unwrap_or(""));
                i = ch_end;
            }
            out.push(Token::String(s));
            continue;
        }

        // multi-line string: text: ... .CRLF
        if b == b't'
            && bytes[i..].starts_with(b"text:")
            && (bytes[i..].starts_with(b"text:\r\n")
                || bytes[i..].starts_with(b"text:\n")
                || bytes[i..].starts_with(b"text: \r\n")
                || bytes[i..].starts_with(b"text: \n"))
        {
            let start = i;
            // skip "text:" and any trailing CR/space, plus the LF
            i += "text:".len();
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r') {
                i += 1;
            }
            if i >= bytes.len() || bytes[i] != b'\n' {
                return Err(TokenizeError::UnterminatedMultiline(start));
            }
            i += 1; // past the LF
            let mut s = String::new();
            loop {
                if i >= bytes.len() {
                    return Err(TokenizeError::UnterminatedMultiline(start));
                }
                // dot-stuffing terminator: ".\r\n" or ".\n" on its own line
                if bytes[i] == b'.'
                    && (bytes[i..].starts_with(b".\r\n") || bytes[i..].starts_with(b".\n"))
                {
                    if bytes[i..].starts_with(b".\r\n") {
                        i += 3;
                    } else {
                        i += 2;
                    }
                    break;
                }
                // dot-stuffed line ("..") becomes a single dot
                if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b'.' {
                    s.push('.');
                    i += 2;
                    continue;
                }
                s.push(bytes[i] as char);
                i += 1;
            }
            out.push(Token::String(s));
            continue;
        }

        // tag ":identifier"
        if b == b':' {
            i += 1;
            let id_start = i;
            while i < bytes.len() && is_ident_byte(bytes[i]) {
                i += 1;
            }
            if i == id_start {
                return Err(TokenizeError::BadTag(id_start - 1));
            }
            let id = std::str::from_utf8(&bytes[id_start..i]).unwrap_or("").to_string();
            out.push(Token::Tag(id));
            continue;
        }

        // number with optional K/M/G suffix
        if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let n_str = std::str::from_utf8(&bytes[start..i]).unwrap();
            let mut n: u64 = n_str.parse().unwrap_or(0);
            if i < bytes.len() {
                match bytes[i] {
                    b'K' | b'k' => {
                        n = n.saturating_mul(1024);
                        i += 1;
                    }
                    b'M' | b'm' => {
                        n = n.saturating_mul(1024 * 1024);
                        i += 1;
                    }
                    b'G' | b'g' => {
                        n = n.saturating_mul(1024 * 1024 * 1024);
                        i += 1;
                    }
                    c if c.is_ascii_alphabetic() => {
                        return Err(TokenizeError::BadNumberSuffix(i));
                    }
                    _ => {}
                }
            }
            out.push(Token::Number(n));
            continue;
        }

        // identifier
        if is_ident_start_byte(b) {
            let start = i;
            while i < bytes.len() && is_ident_byte(bytes[i]) {
                i += 1;
            }
            let id = std::str::from_utf8(&bytes[start..i]).unwrap_or("").to_string();
            out.push(Token::Identifier(id));
            continue;
        }

        let ch = src[i..].chars().next().unwrap_or('?');
        return Err(TokenizeError::Unexpected(i, ch));
    }

    Ok(out)
}

fn is_ident_start_byte(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(src: &str) -> Vec<Token> {
        tokenize(src).expect("tokenize")
    }

    #[test]
    fn empty_input() {
        assert!(lex("").is_empty());
    }

    #[test]
    fn whitespace_only() {
        assert!(lex("  \r\n\t  \n").is_empty());
    }

    #[test]
    fn line_comment() {
        assert!(lex("# this is a comment\n").is_empty());
        assert_eq!(lex("# comment\nkeep;"), vec![
            Token::Identifier("keep".into()),
            Token::Semicolon,
        ]);
    }

    #[test]
    fn block_comment() {
        assert_eq!(lex("/* one */ /* two */ ;"), vec![Token::Semicolon]);
    }

    #[test]
    fn unterminated_block_comment_fails() {
        assert!(matches!(
            tokenize("/* never closes"),
            Err(TokenizeError::UnterminatedComment(_))
        ));
    }

    #[test]
    fn identifiers() {
        assert_eq!(lex("require"), vec![Token::Identifier("require".into())]);
        assert_eq!(lex("if elsif else"), vec![
            Token::Identifier("if".into()),
            Token::Identifier("elsif".into()),
            Token::Identifier("else".into()),
        ]);
    }

    #[test]
    fn tags() {
        assert_eq!(lex(":is :contains :matches"), vec![
            Token::Tag("is".into()),
            Token::Tag("contains".into()),
            Token::Tag("matches".into()),
        ]);
    }

    #[test]
    fn quoted_strings_simple() {
        assert_eq!(lex(r#""hello""#), vec![Token::String("hello".into())]);
    }

    #[test]
    fn quoted_strings_escape() {
        assert_eq!(
            lex(r#""he said \"hi\"""#),
            vec![Token::String(r#"he said "hi""#.into())]
        );
        assert_eq!(lex(r#""a\\b""#), vec![Token::String(r"a\b".into())]);
    }

    #[test]
    fn unterminated_quoted_fails() {
        assert!(matches!(
            tokenize(r#""no closing"#),
            Err(TokenizeError::UnterminatedString(_))
        ));
    }

    #[test]
    fn multiline_strings() {
        let src = "text:\nline 1\nline 2\n.\n";
        let toks = lex(src);
        assert_eq!(toks, vec![Token::String("line 1\nline 2\n".into())]);
    }

    #[test]
    fn multiline_dot_stuffed() {
        let src = "text:\n..startswithdot\n.\n";
        let toks = lex(src);
        assert_eq!(toks, vec![Token::String(".startswithdot\n".into())]);
    }

    #[test]
    fn numbers_plain() {
        assert_eq!(lex("0 1 100 9999"), vec![
            Token::Number(0),
            Token::Number(1),
            Token::Number(100),
            Token::Number(9999),
        ]);
    }

    #[test]
    fn numbers_with_suffix() {
        assert_eq!(lex("1K 1M 1G"), vec![
            Token::Number(1024),
            Token::Number(1024 * 1024),
            Token::Number(1024 * 1024 * 1024),
        ]);
    }

    #[test]
    fn bad_number_suffix() {
        assert!(matches!(
            tokenize("100x"),
            Err(TokenizeError::BadNumberSuffix(_))
        ));
    }

    #[test]
    fn punctuation() {
        assert_eq!(lex("{}[](),;"), vec![
            Token::LBrace,
            Token::RBrace,
            Token::LBracket,
            Token::RBracket,
            Token::LParen,
            Token::RParen,
            Token::Comma,
            Token::Semicolon,
        ]);
    }

    #[test]
    fn full_keep_script() {
        assert_eq!(lex("keep;"), vec![
            Token::Identifier("keep".into()),
            Token::Semicolon,
        ]);
    }

    #[test]
    fn header_test_script() {
        let src = r#"if header :is "Subject" "spam" { discard; }"#;
        let toks = lex(src);
        assert_eq!(toks, vec![
            Token::Identifier("if".into()),
            Token::Identifier("header".into()),
            Token::Tag("is".into()),
            Token::String("Subject".into()),
            Token::String("spam".into()),
            Token::LBrace,
            Token::Identifier("discard".into()),
            Token::Semicolon,
            Token::RBrace,
        ]);
    }

    #[test]
    fn require_with_list() {
        let src = r#"require ["fileinto", "envelope"];"#;
        let toks = lex(src);
        assert_eq!(toks, vec![
            Token::Identifier("require".into()),
            Token::LBracket,
            Token::String("fileinto".into()),
            Token::Comma,
            Token::String("envelope".into()),
            Token::RBracket,
            Token::Semicolon,
        ]);
    }
}
