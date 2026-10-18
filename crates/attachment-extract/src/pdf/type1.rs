//! built-in encoding of an embedded type 1 font program (FontFile)

use super::cmap_lex::{Lexer, Token};

pub(super) enum Builtin {
    Standard,
    Custom(Vec<(u8, Vec<u8>)>),
}

/// Encoding vector from the cleartext part of the program, before `eexec`.
pub(super) fn builtin_encoding(program: &[u8]) -> Option<Builtin> {
    let clear = match program.windows(5).position(|w| w == b"eexec") {
        Some(end) => &program[..end],
        None => program,
    };
    let start = clear.windows(9).position(|w| w == b"/Encoding")?;
    let toks: Vec<Token> = Lexer::new(&clear[start + 9..]).collect();
    if matches!(toks.first(), Some(Token::Keyword(k)) if k == b"StandardEncoding") {
        return Some(Builtin::Standard);
    }
    let mut out = Vec::new();
    for w in toks.windows(4) {
        if let [
            Token::Keyword(dup),
            Token::Int(code),
            Token::Name(name),
            Token::Keyword(put),
        ] = w
            && dup == b"dup"
            && put == b"put"
            && let Ok(code) = u8::try_from(*code)
        {
            out.push((code, name.clone()));
        }
    }
    Some(Builtin::Custom(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_vector() {
        let prog = b"%!PS-AdobeFont-1.0\n/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 65 /Alpha put\ndup 66 /B put\nreadonly def\ncurrentfile eexec \x80\x81";
        let Some(Builtin::Custom(v)) = builtin_encoding(prog) else {
            panic!("no vector")
        };
        assert_eq!(v, vec![(65, b"Alpha".to_vec()), (66, b"B".to_vec())]);
    }

    #[test]
    fn standard_vector() {
        let prog = b"/FontName /X def /Encoding StandardEncoding def currentfile eexec";
        assert!(matches!(builtin_encoding(prog), Some(Builtin::Standard)));
    }
}
