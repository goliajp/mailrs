//! Links whose text shows one web address while the link goes to another,
//! and links still carrying a phishing kit's unfilled template.
//!
//! Reported 2026-10-27: `IT Security Officer <doc@wbg-ae.com>`, subject
//! `Password Reminder for lihao@golia.jp`, SPF and DKIM passing for the
//! sender's own domain. The page reads `You may visit www. golia.jp
//! /lihao@golia.jp/check-activity/`, and that text is a link to
//! `https://webmail-oauth.wzldq.com/vrill/auth[[random_digits(8)]]/…`.
//! The spaces inside the shown address are there to get past filters
//! that look for a domain; a reader does not see them as a break.
//!
//! Over 43,854 production messages on 2026-10-27, both readings below
//! matched that one message and nothing else:
//!
//! - a link whose text, with whitespace removed, is an address *with a
//!   path* on one of our domains, while its target is elsewhere. The
//!   wider reading, any link text that names a different domain than its
//!   target, matched 3,077 messages: newsletters show `example.com` and
//!   link through a click tracker. A bare domain in the text matched 22,
//!   most of them Google's monthly report naming `golia.jp` as the
//!   business. A path is what turns the text into a claim about where
//!   the link goes;
//! - `[[name(...)]]` in a link target: a kit's template call that was
//!   never filled in. Wider template shapes (`{{x}}`, `*|X|*`, `${X}`)
//!   matched 170 messages, mostly broken newsletter templates.
//!
//! Which shown addresses count as ours is the rules' decision; this
//! module reports what each link shows and where it goes.

use crate::advance_fee::Body;

/// How much of each HTML body is read. A phishing page is a few KiB;
/// the bound keeps a 30 MB body as cheap as a letter.
const READ_LIMIT: usize = 512 * 1024;

/// At most this many links are reported per message.
pub const MAX_SHOWN_LINKS: usize = 16;

/// A link whose text reads as a web address with a path, and the host
/// it actually goes to. Both lowercased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownLink {
    /// The host the text shows: `www.golia.jp`.
    pub shown: String,
    /// The host of the `href`: `webmail-oauth.wzldq.com`.
    pub target: String,
}

/// Every link in the HTML bodies whose text shows an address with a
/// path on a host other than the one it goes to.
pub fn shown_links<'a>(bodies: impl IntoIterator<Item = Body<'a>>) -> Vec<ShownLink> {
    let mut out = Vec::new();
    for html in html_bodies(bodies) {
        for (href, text) in anchors(html) {
            let (Some(shown), Some(target)) = (shown_host(text), target_host(href)) else {
                continue;
            };
            if shown != target {
                out.push(ShownLink { shown, target });
                if out.len() == MAX_SHOWN_LINKS {
                    return out;
                }
            }
        }
    }
    out
}

/// Whether any link target carries an unfilled `[[name(...)]]` template
/// call, as written or percent-encoded.
pub fn has_kit_placeholder<'a>(bodies: impl IntoIterator<Item = Body<'a>>) -> bool {
    html_bodies(bodies).any(|html| {
        anchors(html).any(|(href, _)| {
            let href = href.replace("%5B", "[").replace("%5b", "[");
            let href = href.replace("%5D", "]").replace("%5d", "]");
            template_call(&href)
        })
    })
}

fn html_bodies<'a>(bodies: impl IntoIterator<Item = Body<'a>>) -> impl Iterator<Item = &'a str> {
    bodies.into_iter().filter_map(|b| match b {
        Body::Html(h) => Some(cut(h)),
        Body::Plain(_) => None,
    })
}

fn cut(s: &str) -> &str {
    if s.len() <= READ_LIMIT {
        return s;
    }
    let mut end = READ_LIMIT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `(href, inner text)` for each `<a ... href=...>...</a>`. The text keeps
/// its tags; [`shown_host`] drops them.
fn anchors(html: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut rest = html;
    std::iter::from_fn(move || {
        loop {
            let at = find_ci(rest, "<a")?;
            let after = &rest[at + 2..];
            if !after.starts_with(|c: char| c.is_ascii_whitespace()) {
                rest = after;
                continue;
            }
            let Some(close) = tag_end(after) else {
                rest = "";
                return None;
            };
            let attrs = &after[..close];
            let body = &after[close + 1..];
            let text_end = find_ci(body, "</a").unwrap_or(body.len());
            rest = &body[text_end..];
            if let Some(href) = attribute(attrs, "href") {
                return Some((href, &body[..text_end]));
            }
        }
    })
}

/// The `>` that closes a tag, skipping any inside quoted values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attrs;
    loop {
        let at = find_ci(rest, name)?;
        let before_ok = rest[..at]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_ascii_whitespace());
        let after = rest[at + name.len()..].trim_start();
        if !before_ok || !after.starts_with('=') {
            rest = &rest[at + name.len()..];
            continue;
        }
        let value = after[1..].trim_start();
        return Some(match value.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let v = &value[1..];
                &v[..v.find(q).unwrap_or(v.len())]
            }
            _ => {
                &value[..value
                    .find(|c: char| c.is_ascii_whitespace() || c == '>')
                    .unwrap_or(value.len())]
            }
        });
    }
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let (h, n) = (haystack.as_bytes(), needle.as_bytes());
    h.windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
}

/// The host the link text shows, when the text, read without tags,
/// entities' disguise or whitespace, contains an address with a path.
fn shown_host(text: &str) -> Option<String> {
    let squeezed = squeeze(text);
    let bytes = squeezed.as_bytes();
    for (i, _) in squeezed.match_indices('/') {
        if !bytes
            .get(i + 1)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'@' | b'_' | b'-' | b'.'))
        {
            continue;
        }
        let start = squeezed[..i]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'))
            .map_or(0, |p| p + 1);
        let host = squeezed[start..i].trim_matches(|c| c == '.' || c == '-');
        if is_host(host) {
            return Some(host.to_string());
        }
    }
    None
}

/// The link text as it reads: tags dropped, the few entities that hide
/// an address decoded, every kind of space removed, lowercased.
fn squeeze(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match (in_tag, c) {
            (false, '<') => in_tag = true,
            (true, '>') => in_tag = false,
            (false, c) => plain.push(c),
            _ => {}
        }
    }
    let plain = plain
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&#46;", ".")
        .replace("&period;", ".")
        .replace("&#47;", "/")
        .replace("&sol;", "/")
        .replace("&amp;", "&");
    plain
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '\u{200b}'..='\u{200d}' | '\u{feff}'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// `a.b` with a final label of two or more letters.
fn is_host(s: &str) -> bool {
    let Some((head, tld)) = s.rsplit_once('.') else {
        return false;
    };
    !head.is_empty() && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// The host an `http(s)` link goes to; the part after any `user@`, which
/// is where a browser goes and what `https://golia.jp@evil.example/`
/// hides.
fn target_host(href: &str) -> Option<String> {
    let href = href.trim();
    let lower = href.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = &rest[..rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len())];
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host.split(':').next().unwrap_or("").trim_end_matches('.');
    is_host(host).then(|| host.to_string())
}

/// `[[` letters/underscores `(` anything `)]]`.
fn template_call(s: &str) -> bool {
    s.match_indices("[[").any(|(i, _)| {
        let rest = &s[i + 2..];
        let name_len = rest
            .find(|c: char| !(c.is_ascii_alphabetic() || c == '_'))
            .unwrap_or(rest.len());
        name_len > 0
            && rest[name_len..].starts_with('(')
            && rest[name_len..].find(")]]").is_some_and(|close| {
                close > 0 && !rest[name_len + 1..name_len + close].contains("[[")
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported message's link, as it arrived.
    const REPORTED: &str = r#"You may visit&nbsp;<a style="color: rgb(17, 85, 204);" href="https://webmail-oauth.wzldq.com/vrill/auth[[random_digits(8)]]/cpses[[random_string(10)]]#bGloYW9AZ29saWEuanA=" target="_blank" rel="noreferrer" data-saferedirecturl="https://www.google.com/url?q=x">
www. golia.jp /lihao@golia.jp/check-activity/</a>&nbsp;to see email activity"#;

    fn links(html: &str) -> Vec<ShownLink> {
        shown_links([Body::Html(html)])
    }

    fn link(shown: &str, target: &str) -> ShownLink {
        ShownLink {
            shown: shown.into(),
            target: target.into(),
        }
    }

    #[test]
    fn the_reported_link_shows_our_address_and_goes_elsewhere() {
        assert_eq!(
            links(REPORTED),
            vec![link("www.golia.jp", "webmail-oauth.wzldq.com")]
        );
        assert!(has_kit_placeholder([Body::Html(REPORTED)]));
    }

    #[test]
    fn text_that_is_not_an_address_with_a_path_reports_nothing() {
        for html in [
            r#"<a href="https://track.example.net/c/1">golia.jp</a>"#,
            r#"<a href="https://track.example.net/c/1">Keep Current Password</a>"#,
            r#"<a href="https://track.example.net/c/1">Read on example.com</a>"#,
            r#"<a href="mailto:a@golia.jp">golia.jp/contact</a>"#,
            r#"<a name="top">golia.jp/x</a>"#,
        ] {
            assert_eq!(links(html), Vec::new(), "{html}");
        }
    }

    #[test]
    fn a_link_that_goes_where_it_says_reports_nothing() {
        assert_eq!(
            links(r#"<A HREF='https://www.golia.jp/news'>https://www.golia.jp/news</A>"#),
            Vec::new()
        );
    }

    #[test]
    fn the_shown_address_survives_tags_entities_and_odd_spaces() {
        for text in [
            "golia<b>.</b>jp/login",
            "golia&#46;jp&#47;login",
            "golia .\u{a0}jp / login",
            "GOLIA.JP/\u{200b}login",
            "https://golia.jp/login",
        ] {
            let html = format!(r#"<a class="x" href="https://evil.example/p">{text}</a>"#);
            assert_eq!(
                links(&html),
                vec![link("golia.jp", "evil.example")],
                "{text}"
            );
        }
    }

    #[test]
    fn the_target_is_the_host_a_browser_would_open() {
        assert_eq!(
            links(r#"<a href="https://golia.jp@evil.example:8443/x">golia.jp/x</a>"#),
            vec![link("golia.jp", "evil.example")]
        );
        assert_eq!(
            links(r#"<a target=_blank href=HTTP://Evil.Example/x>golia.jp/x</a>"#),
            vec![link("golia.jp", "evil.example")]
        );
    }

    #[test]
    fn a_quoted_angle_bracket_does_not_end_the_tag() {
        assert_eq!(
            links(r#"<a title="a > b" href="https://evil.example/">golia.jp/x</a>"#),
            vec![link("golia.jp", "evil.example")]
        );
    }

    #[test]
    fn plain_text_is_not_read() {
        assert_eq!(
            shown_links([Body::Plain(
                r#"<a href="https://evil.example/">golia.jp/x</a>"#
            )]),
            Vec::new()
        );
    }

    #[test]
    fn placeholders_are_template_calls_not_any_brackets() {
        for href in [
            "https://x.example/a[[random_digits(8)]]",
            "https://x.example/a%5B%5Brandom_string(10)%5D%5D",
            "https://x.example/a[[f()]]",
        ] {
            let html = format!(r#"<a href="{href}">x</a>"#);
            assert!(has_kit_placeholder([Body::Html(&html)]), "{href}");
        }
        for href in [
            "https://x.example/a[[name]]",
            "https://x.example/a{{email}}",
            "https://x.example/a?x=[[1]]",
            "https://x.example/a[[f(]]",
        ] {
            let html = format!(r#"<a href="{href}">x</a>"#);
            assert!(!has_kit_placeholder([Body::Html(&html)]), "{href}");
        }
    }

    #[test]
    fn the_report_is_bounded() {
        let html = r#"<a href="https://evil.example/">golia.jp/x</a>"#.repeat(100);
        assert_eq!(links(&html).len(), MAX_SHOWN_LINKS);
    }

    #[test]
    fn an_unclosed_tag_ends_the_scan() {
        assert_eq!(
            links(r#"<a href="https://evil.example/" golia.jp/x"#),
            Vec::new()
        );
        assert!(!has_kit_placeholder([Body::Html("<a href=")]));
    }
}
