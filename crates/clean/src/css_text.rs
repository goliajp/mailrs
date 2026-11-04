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
/// - runs of two or more `name: value;` declarations outside braces, what
///   a style attribute leaves when its quotes are lost
/// - words that start with a CSS vendor prefix (`mso-`, `-webkit-`,
///   `-moz-`, `-ms-`), alone or as a `.class` or `#id`
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
    let cur = strip_style_attributes(&cur);
    let cur = strip_declaration_runs(&cur);
    strip_vendor_words(&cur)
}

/// Longest value taken in a bare declaration.
const MAX_VALUE_LEN: usize = 120;

/// The length of a `name: value;` declaration at the start of `s`, if one
/// is there: the name letters and `-`, the value without braces, angle
/// brackets or line breaks.
fn declaration_len(s: &str) -> Option<usize> {
    let name = s
        .bytes()
        .take_while(|c| c.is_ascii_alphabetic() || *c == b'-')
        .count();
    if name == 0 {
        return None;
    }
    let rest = &s[name..];
    let colon = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    if !rest[colon..].starts_with(':') {
        return None;
    }
    let value = &rest[colon + 1..];
    let end = value.find(|c: char| matches!(c, ';' | '{' | '}' | '<' | '>' | '\n' | '\r'))?;
    if end == 0 || end > MAX_VALUE_LEN || !value[end..].starts_with(';') {
        return None;
    }
    Some(name + colon + 1 + end + 1)
}

fn strip_declaration_runs(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let starts_word = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'-');
        if starts_word && b[i].is_ascii_alphabetic() {
            let mut j = i;
            let mut n = 0;
            while let Some(len) = declaration_len(&text[j..]) {
                n += 1;
                j += len;
                j += text[j..].len() - text[j..].trim_start_matches([' ', '\t']).len();
            }
            if n >= 2 {
                out.push(' ');
                i = j;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap_or(' ');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

const VENDOR_PREFIXES: [&str; 4] = ["mso-", "-webkit-", "-moz-", "-ms-"];

fn strip_vendor_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (k, line) in text.split('\n').enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let kept: Vec<&str> = line
            .split(' ')
            .filter(|w| {
                let bare = w.trim_start_matches(['.', '#']).to_ascii_lowercase();
                !VENDOR_PREFIXES.iter().any(|p| bare.starts_with(p))
            })
            .collect();
        out.push_str(&kept.join(" "));
    }
    out
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

    /// A style attribute whose quotes were lost, and selector names that
    /// only CSS uses.
    #[test]
    fn bare_declarations_and_vendor_names_go() {
        let s = strip_css_rules(
            "font: yu gothic, sans-serif; margin:0; padding:0; color: #000; background-color: #fff;> Monthly news\n\
             .mso-no-bg .vml-p li Sale fares -webkit-text-size-adjust",
        );
        for css in [
            "margin",
            "padding",
            "background-color",
            "mso-no-bg",
            "-webkit-",
        ] {
            assert!(!s.contains(css), "{css} left in {s}");
        }
        assert!(
            s.contains("Monthly news") && s.contains("Sale fares"),
            "{s}"
        );
    }

    #[test]
    fn one_declaration_in_prose_stays() {
        for t in [
            "Note: the meeting moved; see below",
            "ratio: 3:1; then we go",
            "elevation prop has incorrect border-radius (fixed)",
        ] {
            assert_eq!(strip_css_rules(t), t);
        }
        // two declarations in shape are taken; prose written that way is
        // the cost
        assert_eq!(strip_css_rules("Time: 10am; Place: lobby;").trim(), "");
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
