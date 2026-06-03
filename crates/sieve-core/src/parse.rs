//! RFC 5228 §3-4 recursive-descent parser.

use crate::ast::{Argument, Command, Test};
use crate::lex::{Token, TokenizeError, tokenize};

/// Parse failure modes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// Tokenizer rejected the source before any parsing started.
    #[error("tokenize: {0}")]
    Tokenize(#[from] TokenizeError),
    /// Reached end-of-input while expecting more tokens.
    #[error("unexpected end of input")]
    UnexpectedEof,
    /// A specific token kind was required but a different one (or
    /// none) was found at this position.
    #[error("expected {expected} at token {at}, got {got:?}")]
    Expected {
        /// What the parser was looking for, as a human-readable label.
        expected: String,
        /// 0-based token index in the lexed stream.
        at: usize,
        /// What was actually found, or `None` for end-of-input.
        got: Option<Token>,
    },
    /// A test was expected (after `if`, inside `allof(…)`, etc.)
    /// but none could be parsed.
    #[error("expected test expression at token {at}")]
    ExpectedTest {
        /// 0-based token index.
        at: usize,
    },
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, want: &Token) -> Result<(), ParseError> {
        match self.peek() {
            Some(t) if t == want => {
                self.pos += 1;
                Ok(())
            }
            other => Err(ParseError::Expected {
                expected: format!("{want}"),
                at: self.pos,
                got: other.cloned(),
            }),
        }
    }

    fn parse_commands(&mut self) -> Result<Vec<Command>, ParseError> {
        let mut out = Vec::new();
        while let Some(t) = self.peek() {
            if matches!(t, Token::RBrace) {
                break;
            }
            out.push(self.parse_command()?);
        }
        Ok(out)
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        let name = match self.bump() {
            Some(Token::Identifier(n)) => n,
            other => {
                return Err(ParseError::Expected {
                    expected: "command identifier".into(),
                    at: self.pos.saturating_sub(1),
                    got: other,
                });
            }
        };

        let mut args = Vec::new();
        while let Some(t) = self.peek() {
            match t {
                Token::Semicolon | Token::LBrace => break,
                _ => args.push(self.parse_argument_or_test(&name)?),
            }
        }

        let block = if matches!(self.peek(), Some(Token::LBrace)) {
            self.bump();
            let inner = self.parse_commands()?;
            self.expect(&Token::RBrace)?;
            inner
        } else {
            self.expect(&Token::Semicolon)?;
            Vec::new()
        };

        Ok(Command { name, args, block })
    }

    /// Argument *or* test — used at the top level inside a command's
    /// argument list. `if`, `elsif`, `not`, `allof`, `anyof` always
    /// start a test; otherwise the parser checks whether the next
    /// identifier is a known test name.
    fn parse_argument_or_test(&mut self, parent: &str) -> Result<Argument, ParseError> {
        // For control-flow commands the next token MUST be a test.
        let starts_test = matches!(parent, "if" | "elsif" | "while");
        if starts_test {
            return Ok(Argument::Test(self.parse_test()?));
        }
        self.parse_argument()
    }

    fn parse_argument(&mut self) -> Result<Argument, ParseError> {
        match self.peek().cloned() {
            Some(Token::Tag(t)) => {
                self.bump();
                Ok(Argument::Tag(t))
            }
            Some(Token::Number(n)) => {
                self.bump();
                Ok(Argument::Number(n))
            }
            Some(Token::String(s)) => {
                self.bump();
                Ok(Argument::String(s))
            }
            Some(Token::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    match self.peek().cloned() {
                        Some(Token::String(s)) => {
                            self.bump();
                            items.push(s);
                        }
                        other => {
                            return Err(ParseError::Expected {
                                expected: "string inside list".into(),
                                at: self.pos,
                                got: other,
                            });
                        }
                    }
                    match self.peek() {
                        Some(Token::Comma) => {
                            self.bump();
                            continue;
                        }
                        Some(Token::RBracket) => {
                            self.bump();
                            break;
                        }
                        other => {
                            return Err(ParseError::Expected {
                                expected: ", or ]".into(),
                                at: self.pos,
                                got: other.cloned(),
                            });
                        }
                    }
                }
                Ok(Argument::StringList(items))
            }
            Some(Token::Identifier(_)) => {
                // identifier inside an arg slot must be a test
                // (this handles e.g. `not header :is …`)
                Ok(Argument::Test(self.parse_test()?))
            }
            other => Err(ParseError::Expected {
                expected: "argument".into(),
                at: self.pos,
                got: other,
            }),
        }
    }

    fn parse_test(&mut self) -> Result<Test, ParseError> {
        let name = match self.bump() {
            Some(Token::Identifier(n)) => n,
            other => {
                return Err(ParseError::Expected {
                    expected: "test identifier".into(),
                    at: self.pos.saturating_sub(1),
                    got: other,
                });
            }
        };

        // `allof(t1, t2)` / `anyof(t1, t2)` / `not test`
        let mut children = Vec::new();
        match name.as_str() {
            "not" => {
                children.push(self.parse_test()?);
                return Ok(Test {
                    name,
                    tags: Vec::new(),
                    args: Vec::new(),
                    children,
                });
            }
            "allof" | "anyof" => {
                self.expect(&Token::LParen)?;
                loop {
                    children.push(self.parse_test()?);
                    match self.peek() {
                        Some(Token::Comma) => {
                            self.bump();
                            continue;
                        }
                        Some(Token::RParen) => {
                            self.bump();
                            break;
                        }
                        other => {
                            return Err(ParseError::Expected {
                                expected: ", or )".into(),
                                at: self.pos,
                                got: other.cloned(),
                            });
                        }
                    }
                }
                return Ok(Test {
                    name,
                    tags: Vec::new(),
                    args: Vec::new(),
                    children,
                });
            }
            _ => {}
        }

        // Regular test: tags + args until we hit a delimiter that
        // unambiguously ends a test (`)`, `,`, `;`, `{`)
        let mut tags = Vec::new();
        let mut args = Vec::new();
        while let Some(t) = self.peek().cloned() {
            match t {
                Token::Tag(s) => {
                    self.bump();
                    tags.push(s);
                }
                Token::Number(n) => {
                    self.bump();
                    args.push(Argument::Number(n));
                }
                Token::String(s) => {
                    self.bump();
                    args.push(Argument::String(s));
                }
                Token::LBracket => {
                    let list = self.parse_argument()?;
                    args.push(list);
                }
                Token::RParen | Token::Comma | Token::Semicolon | Token::LBrace => break,
                Token::Identifier(_) => break, // start of next command — test is done
                other => {
                    return Err(ParseError::Expected {
                        expected: "test arg or delimiter".into(),
                        at: self.pos,
                        got: Some(other),
                    });
                }
            }
        }

        Ok(Test {
            name,
            tags,
            args,
            children,
        })
    }
}

/// Tokenize + parse a full Sieve script.
pub fn parse_script(src: &str) -> Result<Vec<Command>, ParseError> {
    let tokens = tokenize(src)?;
    let mut p = Parser::new(tokens);
    p.parse_commands()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Argument;

    #[test]
    fn empty_script() {
        let cmds = parse_script("").unwrap();
        assert!(cmds.is_empty());
    }

    #[test]
    fn just_keep() {
        let cmds = parse_script("keep;").unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "keep");
        assert!(cmds[0].block.is_empty());
        assert!(cmds[0].args.is_empty());
    }

    #[test]
    fn require_string_list() {
        let cmds = parse_script(r#"require ["fileinto", "envelope"];"#).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "require");
        assert_eq!(
            cmds[0].args,
            vec![Argument::StringList(vec![
                "fileinto".into(),
                "envelope".into()
            ])]
        );
    }

    #[test]
    fn fileinto_with_string_arg() {
        let cmds = parse_script(r#"fileinto "Junk";"#).unwrap();
        assert_eq!(cmds[0].name, "fileinto");
        assert_eq!(cmds[0].args, vec![Argument::String("Junk".into())]);
    }

    #[test]
    fn if_header_is_then_block() {
        let src = r#"if header :is "Subject" "spam" { discard; }"#;
        let cmds = parse_script(src).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "if");
        assert_eq!(cmds[0].block.len(), 1);
        assert_eq!(cmds[0].block[0].name, "discard");
        // first arg must be the parsed Test
        match &cmds[0].args[0] {
            Argument::Test(t) => {
                assert_eq!(t.name, "header");
                assert_eq!(t.tags, vec!["is".to_string()]);
                assert_eq!(t.args.len(), 2);
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn if_else_chain() {
        let src = r#"
            if header :is "Subject" "spam" { discard; }
            elsif header :contains "Subject" "ad" { fileinto "Ads"; }
            else { keep; }
        "#;
        let cmds = parse_script(src).unwrap();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].name, "if");
        assert_eq!(cmds[1].name, "elsif");
        assert_eq!(cmds[2].name, "else");
    }

    #[test]
    fn allof_anyof() {
        let src = r#"if allof(header :is "X" "1", header :is "Y" "2") { keep; }"#;
        let cmds = parse_script(src).unwrap();
        match &cmds[0].args[0] {
            Argument::Test(t) => {
                assert_eq!(t.name, "allof");
                assert_eq!(t.children.len(), 2);
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn not_wrap() {
        let src = r#"if not header :is "Subject" "spam" { keep; }"#;
        let cmds = parse_script(src).unwrap();
        match &cmds[0].args[0] {
            Argument::Test(t) => {
                assert_eq!(t.name, "not");
                assert_eq!(t.children.len(), 1);
                assert_eq!(t.children[0].name, "header");
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn size_test() {
        let src = "if size :over 1M { discard; }";
        let cmds = parse_script(src).unwrap();
        match &cmds[0].args[0] {
            Argument::Test(t) => {
                assert_eq!(t.name, "size");
                assert_eq!(t.tags, vec!["over".to_string()]);
                assert_eq!(t.args, vec![Argument::Number(1024 * 1024)]);
            }
            other => panic!("expected Test, got {other:?}"),
        }
    }

    #[test]
    fn redirect() {
        let cmds = parse_script(r#"redirect "alice@example.com";"#).unwrap();
        assert_eq!(cmds[0].name, "redirect");
        assert_eq!(
            cmds[0].args,
            vec![Argument::String("alice@example.com".into())]
        );
    }
}
