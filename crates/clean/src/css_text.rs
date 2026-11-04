//! CSS left in text: what a sender's own HTML-to-text conversion, or a
//! plain part carrying markup, leaves behind. None of it is words.

/// Longest brace block taken for one CSS rule.
const MAX_RULE_LEN: usize = 4096;
/// Longest at-rule (`@media … { … }`) taken whole.
const MAX_AT_RULE_LEN: usize = 20_000;
/// Longest comment or `style="…"` value taken.
const MAX_SPAN: usize = 2000;

/// At-rules whose body is style, whatever is inside it.
const AT_RULES: [&str; 7] = [
    "media",
    "font-face",
    "supports",
    "page",
    "keyframes",
    "-webkit-keyframes",
    "-ms-viewport",
];

/// `text` without the CSS in it:
///
/// - `/* … */` comments
/// - at-rules with a style body (`@media … { … }`, `@font-face { … }`),
///   prelude and body together, and `@import …;`
/// - rule blocks: a `{…}` with no brace inside whose content is only
///   declarations (`name: value`, separated by `;`, the name letters and
///   `-`), run to a fixed point. Selectors stay; they are words
/// - `style="…"` and `style='…'` attribute values
pub fn strip_css_rules(text: &str) -> String {
    let mut cur = strip_spans(text, "/*", "*/");
    cur = strip_at_rules(&cur);
    loop {
        let next = strip_rule_blocks(&cur);
        if next.len() == cur.len() {
            break;
        }
        cur = next;
    }
    strip_style_attributes(&cur)
}

/// Remove every `open … close` span no longer than [`MAX_SPAN`].
fn strip_spans(text: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        match after.find(close) {
            Some(j) if j <= MAX_SPAN => {
                out.push_str(&rest[..i]);
                out.push(' ');
                rest = &after[j + close.len()..];
            }
            _ => {
                out.push_str(&rest[..i + open.len()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn strip_at_rules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('@') {
        let after = &rest[i + 1..];
        let lc_head: String = after
            .chars()
            .take(20)
            .collect::<String>()
            .to_ascii_lowercase();
        // an at-rule starts a word (`bob@media.example` is an address) and
        // its name ends at a space, `{` or `(`
        let word_start = i == 0 || !rest.as_bytes()[i - 1].is_ascii_alphanumeric();
        if word_start && lc_head.starts_with("import ") {
            match after.find(';') {
                Some(j) if j <= MAX_SPAN => {
                    out.push_str(&rest[..i]);
                    rest = &after[j + 1..];
                }
                _ => {
                    out.push_str(&rest[..=i]);
                    rest = after;
                }
            }
            continue;
        }
        let is_style_at = word_start
            && AT_RULES.iter().any(|a| {
                lc_head.starts_with(a)
                    && lc_head[a.len()..]
                        .starts_with(|c: char| c.is_whitespace() || c == '{' || c == '(')
            });
        let open = after.find('{').filter(|o| *o <= 300);
        match (is_style_at, open) {
            (true, Some(o)) => {
                out.push_str(&rest[..i]);
                out.push(' ');
                rest = match matching_close(&after[o..]) {
                    Some(c) => &after[o + c + 1..],
                    None => &after[o + 1..],
                };
            }
            _ => {
                out.push_str(&rest[..=i]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Offset of the `}` closing the `{` that `s` starts with.
fn matching_close(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate().take(MAX_AT_RULE_LEN) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_rule_blocks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(['{', '}']) else {
            break;
        };
        let inner = &after[..close];
        let is_rule =
            after.as_bytes()[close] == b'}' && close <= MAX_RULE_LEN && is_declarations(inner);
        if is_rule {
            out.push_str(&rest[..open]);
            out.push(' ');
            rest = &after[close + 1..];
        } else {
            out.push_str(&rest[..open + 1]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Empty, or every `;`-separated piece is `name: value`.
fn is_declarations(inner: &str) -> bool {
    inner.split(';').all(|piece| {
        let piece = piece.trim();
        if piece.is_empty() {
            return true;
        }
        let Some((name, _)) = piece.split_once(':') else {
            return false;
        };
        let name = name.trim();
        !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphabetic() || c == b'-')
    })
}

/// Remove the values of `style=` attributes, the attribute name with them.
fn strip_style_attributes(text: &str) -> String {
    let lc = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    let mut search = 0;
    while let Some(off) = lc[search..].find("style") {
        let at = search + off;
        search = at + "style".len();
        let word_start = at == 0 || !lc.as_bytes()[at - 1].is_ascii_alphanumeric();
        let rest = &lc[search..];
        let eq = rest.len() - rest.trim_start().len();
        if !word_start || !rest[eq..].starts_with('=') {
            continue;
        }
        let after_eq = search + eq + 1;
        let value = lc[after_eq..].trim_start();
        let q_at = lc.len() - value.len();
        let Some(q) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(end) = value[1..].find(q).filter(|e| *e <= MAX_SPAN) else {
            continue;
        };
        out.push_str(&text[from..at]);
        out.push(' ');
        from = q_at + 1 + end + 1;
        search = from;
    }
    out.push_str(&text[from..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_blocks_comments_and_media_go() {
        let t = "Hello\n\
table { border-collapse: collapse; border-spacing: 0 } td{mso-line-height-rule:exactly;}\n\
.header { background-color: #0070d5; /* brand blue */ padding: 20px; }\n\
@media only screen and (max-width:600px) { .wrap { width: 100% !important; } }\n\
@import url(x.css);\nSee you";
        let s = strip_css_rules(t);
        for css in [
            "border-collapse",
            "border-spacing",
            "mso-line",
            "background-color",
            "max-width",
            "important",
            "brand blue",
            "x.css",
        ] {
            assert!(!s.contains(css), "{css} left in {s}");
        }
        assert!(s.contains("Hello") && s.contains("See you"), "{s}");
    }

    #[test]
    fn an_unclosed_media_block_loses_its_prelude() {
        let s = strip_css_rules("@media screen and (max-width: 600px) { .container .hero");
        assert!(!s.contains("max-width"), "{s}");
        assert!(s.contains(".container"), "{s}");
    }

    #[test]
    fn style_attributes_left_in_text_go() {
        let s = strip_css_rules(
            "see <span style=\"font-size:small;\">this</span> and <p STYLE = 'color:#888'>that</p>",
        );
        assert!(!s.contains("font-size") && !s.contains("#888"), "{s}");
        assert!(s.contains("this") && s.contains("that"), "{s}");
    }

    /// Braces, at-signs and the word style that are not CSS are text.
    #[test]
    fn other_text_is_left_alone() {
        for t in [
            "the set {1, 2, 3}",
            "a template {name} here",
            "json {\"a\": 1}",
            "code fn x() { return 1; }",
            "write to bob@media.example { soon }",
            "@mediator meeting {at noon}",
            "lifestyle=good, style is personal",
            "a /* lone opener",
        ] {
            assert_eq!(strip_css_rules(t), t);
        }
    }
}
