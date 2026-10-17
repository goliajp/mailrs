//! Links from HTML anchors and from bare URLs in plain text.

use std::collections::HashMap;

use crate::parsed::Link;

const MAX_LINKS: usize = 50;

// a url holding one of these is the link a signup test usually follows
const ACTION_WORDS: &[&str] = &[
    "verif", "confirm", "activat", "token", "magic", "login", "reset",
];

/// Every http(s) link in `html` anchors and in `text`, deduplicated by
/// URL with the first non-empty anchor text kept. Action links (verify,
/// confirm, activate, token, magic, login, reset) come first, the rest
/// keep document order: anchors before bare URLs. At most 50.
pub fn links(html: Option<&str>, text: &str) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut add = |url: String, text: String| {
        if !is_http(&url) {
            return;
        }
        match seen.get(&url) {
            Some(&i) => {
                if out[i].text.is_empty() {
                    out[i].text = text;
                }
            }
            None => {
                seen.insert(url.clone(), out.len());
                out.push(Link { url, text });
            }
        }
    };
    if let Some(html) = html {
        for (url, text) in anchors(html) {
            add(url, text);
        }
    }
    for url in bare_urls(text) {
        add(url.to_string(), String::new());
    }
    out.sort_by_key(|l| !is_action(&l.url));
    out.truncate(MAX_LINKS);
    out
}

fn is_http(url: &str) -> bool {
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn is_action(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ACTION_WORDS.iter().any(|w| lower.contains(w))
}

// (href, anchor text) of every `<a ... href=...>...</a>`; an anchor with
// no closing tag has no text
fn anchors(html: &str) -> Vec<(String, String)> {
    let lower = html.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(off) = lower[pos..].find("<a") {
        let after = pos + off + 2;
        if !bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
            pos = after;
            continue;
        }
        let Some(gt) = lower[after..].find('>') else {
            break;
        };
        let tag_end = after + gt;
        let body_start = tag_end + 1;
        let close = lower[body_start..].find("</a").map(|i| body_start + i);
        let body = close.map_or("", |end| &html[body_start..end]);
        if let Some(href) = attr(&html[after..tag_end], &lower[after..tag_end], "href") {
            out.push((decode_entities(href.trim()), anchor_text(body)));
        }
        pos = close.unwrap_or(body_start);
    }
    out
}

// the value of attribute `name` in the inside of a start tag; `lower`
// is the same text ascii-lowercased, so byte offsets agree
fn attr<'a>(tag: &'a str, lower: &str, name: &str) -> Option<&'a str> {
    let bytes = lower.as_bytes();
    let mut pos = 0;
    while let Some(off) = lower[pos..].find(name) {
        let at = pos + off;
        pos = at + name.len();
        if at > 0 && !bytes[at - 1].is_ascii_whitespace() {
            continue;
        }
        let mut i = skip_spaces(bytes, pos);
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i = skip_spaces(bytes, i + 1);
        return Some(match bytes.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                let start = i + 1;
                let end = lower[start..]
                    .find(q as char)
                    .map_or(tag.len(), |e| start + e);
                &tag[start..end]
            }
            _ => {
                let end = lower[i..]
                    .find(|c: char| c.is_ascii_whitespace())
                    .map_or(tag.len(), |e| i + e);
                &tag[i..end]
            }
        });
    }
    None
}

fn skip_spaces(bytes: &[u8], mut i: usize) -> usize {
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

fn anchor_text(body: &str) -> String {
    let mut plain = String::with_capacity(body.len());
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                plain.push(' ');
            }
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    decode_entities(&plain)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest
            .get(1..12)
            .or_else(|| rest.get(1..))
            .and_then(|head| head.find(';'))
            .and_then(|semi| entity(&rest[1..1 + semi]).map(|c| (c, semi + 2)));
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    let numeric = |digits: &str, radix: u32| {
        u32::from_str_radix(digits, radix)
            .ok()
            .and_then(char::from_u32)
    };
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => match name.strip_prefix('#') {
            Some(hex) if hex.starts_with(['x', 'X']) => numeric(&hex[1..], 16),
            Some(dec) => numeric(dec, 10),
            None => None,
        },
    }
}

// a url ends at whitespace, a quote or bracket, or the first non-ascii
// character (japanese text often follows a link with no space); trailing
// punctuation belongs to the sentence
fn bare_urls(text: &str) -> Vec<&str> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(off) = lower[pos..].find("http") {
        let start = pos + off;
        let rest = &lower[start..];
        if !(rest.starts_with("http://") || rest.starts_with("https://")) {
            pos = start + 4;
            continue;
        }
        let len = rest
            .find(|c: char| {
                !c.is_ascii()
                    || c.is_ascii_whitespace()
                    || c.is_ascii_control()
                    || "<>\"'`".contains(c)
            })
            .unwrap_or(rest.len());
        let url = trim_trailing(&text[start..start + len]);
        if url.len() > "https://".len() {
            out.push(url);
        }
        pos = start + len.max(4);
    }
    out
}

fn trim_trailing(mut url: &str) -> &str {
    loop {
        let Some(last) = url.chars().last() else {
            return url;
        };
        let unbalanced = |open: char| url.matches(last).count() > url.matches(open).count();
        let strip = match last {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => unbalanced('('),
            ']' => unbalanced('['),
            '}' => unbalanced('{'),
            _ => false,
        };
        if !strip {
            return url;
        }
        url = &url[..url.len() - 1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(found: &[Link]) -> Vec<&str> {
        found.iter().map(|l| l.url.as_str()).collect()
    }

    #[test]
    fn anchors_with_text_and_entities() {
        let html = r#"<p>Hi</p><a class="btn" href="https://x.test/a?x=1&amp;y=2">Open <b>the</b>
            app</a> <A HREF='https://x.test/b'>B</A> <a href=https://x.test/c>C</a>"#;
        let found = links(Some(html), "");
        assert_eq!(
            found,
            [
                Link {
                    url: "https://x.test/a?x=1&y=2".into(),
                    text: "Open the app".into()
                },
                Link {
                    url: "https://x.test/b".into(),
                    text: "B".into()
                },
                Link {
                    url: "https://x.test/c".into(),
                    text: "C".into()
                },
            ]
        );
    }

    #[test]
    fn non_http_schemes_are_dropped() {
        let html = r##"<a href="mailto:a@x.test">mail</a><a href="tel:+8131234">call</a><a href="#top">top</a><abbr href="https://x.test/no">x</abbr>"##;
        assert!(links(Some(html), "write to mailto:a@x.test").is_empty());
    }

    #[test]
    fn bare_urls_lose_trailing_punctuation() {
        let text = "See https://x.test/a. Or (https://x.test/b), and https://x.test/w_(z)\n\
                    詳細はhttps://x.test/jpをご覧ください";
        assert_eq!(
            urls(&links(None, text)),
            [
                "https://x.test/a",
                "https://x.test/b",
                "https://x.test/w_(z)",
                "https://x.test/jp"
            ]
        );
    }

    #[test]
    fn dedupe_keeps_first_non_empty_text() {
        let html = r#"<a href="https://x.test/a"><img src="logo.png"></a><a href="https://x.test/a">Home</a>"#;
        let found = links(Some(html), "https://x.test/a");
        assert_eq!(
            found,
            [Link {
                url: "https://x.test/a".into(),
                text: "Home".into()
            }]
        );
    }

    #[test]
    fn action_links_first_and_stable() {
        let text = "https://x.test/home https://x.test/help https://x.test/confirm?t=1 https://x.test/blog https://x.test/reset";
        assert_eq!(
            urls(&links(None, text)),
            [
                "https://x.test/confirm?t=1",
                "https://x.test/reset",
                "https://x.test/home",
                "https://x.test/help",
                "https://x.test/blog",
            ]
        );
    }

    #[test]
    fn capped_at_fifty() {
        let text: String = (0..80).map(|i| format!("https://x.test/{i} ")).collect();
        assert_eq!(links(None, &text).len(), MAX_LINKS);
    }

    #[test]
    fn broken_markup_does_not_panic() {
        for html in [
            "<a ",
            "<a href=",
            "<a href=\"",
            "<a href='x>",
            "<a\thref=https://x.test/é>ü",
            "&#xZZ; &#99999999; &",
            "<a href=\"https://x.test/&#\">",
        ] {
            let _ = links(Some(html), html);
        }
    }
}
