//! What an HTML or SVG attachment is built to do, read as bytes.
//!
//! A page that assembles a file in the browser and hands it to the
//! reader — a `Blob` from a large base64 string, then a forced download
//! — gets an executable past every filter that looks at attachments,
//! because the attachment is "just HTML". So is a page whose only form
//! posts a password to somebody else's host. Both are structures, not
//! wording, and both are found by scanning the bytes once: nothing is
//! parsed and no script runs.

use crate::brand::registrable;

/// The longest run of base64 a page may carry without it being a payload.
const BIG_BASE64: usize = 64 * 1024;
/// A `data:` source this long, not an image, is a file inside the page.
const BIG_DATA_URI: usize = 16 * 1024;

/// What one attachment showed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HtmlFeatures {
    /// It builds a file in the page and makes the browser save it.
    pub builds_a_download: bool,
    /// An SVG that runs script.
    pub svg_runs_script: bool,
    /// A form with a password field that posts to another site than the
    /// sender's.
    pub offsite_login_form: bool,
}

impl HtmlFeatures {
    fn or(self, other: HtmlFeatures) -> HtmlFeatures {
        HtmlFeatures {
            builds_a_download: self.builds_a_download || other.builds_a_download,
            svg_runs_script: self.svg_runs_script || other.svg_runs_script,
            offsite_login_form: self.offsite_login_form || other.offsite_login_form,
        }
    }
}

/// Every attachment of `(name, content type, bytes)` that is a page or an
/// SVG, by name, type or first bytes, folded together; `sender_host` is
/// the `From:` host a login form is compared against.
#[must_use]
pub fn scan<'a>(
    parts: impl IntoIterator<Item = (&'a str, &'a str, &'a [u8])>,
    sender_host: &str,
) -> HtmlFeatures {
    parts
        .into_iter()
        .filter(|(name, ctype, bytes)| is_page(name, ctype, bytes))
        .map(|(_, _, bytes)| features(bytes, sender_host))
        .fold(HtmlFeatures::default(), HtmlFeatures::or)
}

fn is_page(name: &str, ctype: &str, bytes: &[u8]) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.trim_end_matches(['"', '\'', ' ']).to_ascii_lowercase())
        .unwrap_or_default();
    let ctype = ctype.to_ascii_lowercase();
    let head: Vec<u8> = bytes
        .iter()
        .skip_while(|b| b.is_ascii_whitespace() || **b == 0xef || **b == 0xbb || **b == 0xbf)
        .take(16)
        .map(u8::to_ascii_lowercase)
        .collect();
    matches!(
        ext.as_str(),
        "html" | "htm" | "shtml" | "xhtml" | "svg" | "mht"
    ) || ctype.starts_with("text/html")
        || ctype.starts_with("image/svg")
        || head.starts_with(b"<!doctype html")
        || head.starts_with(b"<html")
        || head.starts_with(b"<svg")
        || head.starts_with(b"<?xml")
}

fn features(bytes: &[u8], sender_host: &str) -> HtmlFeatures {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    let is_svg = text.contains("<svg");
    let blob = text.contains("new blob(") || text.contains("new file(");
    let saves = text.contains("url.createobjecturl(")
        || text.contains("mssaveoropenblob(")
        || text.contains("navigator.mssaveblob(");
    let triggers =
        text.contains(".download") || text.contains("download=") || text.contains(".click()");
    let payload = longest_base64(bytes) >= BIG_BASE64 || has_data_payload(&text);
    HtmlFeatures {
        builds_a_download: blob && saves && triggers && payload,
        svg_runs_script: is_svg && (text.contains("<script") || has_event_handler(&text)),
        offsite_login_form: offsite_password_form(&text, sender_host),
    }
}

/// The longest run of base64 characters, newlines inside it allowed.
fn longest_base64(bytes: &[u8]) -> usize {
    let (mut best, mut run) = (0, 0);
    for &b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => run += 1,
            b'\r' | b'\n' => {}
            _ => {
                best = best.max(run);
                run = 0;
            }
        }
    }
    best.max(run)
}

/// An `embed`, `iframe`, `object` or download link whose source is a long
/// `data:` URI that is not an image.
fn has_data_payload(text: &str) -> bool {
    let mut from = 0;
    while let Some(at) = text[from..].find("data:") {
        let start = from + at;
        from = start + 5;
        let before = &text[start.saturating_sub(200)..start];
        let in_a_carrier = ["<embed", "<iframe", "<object", "download"]
            .iter()
            .any(|t| before.contains(t));
        if !in_a_carrier || text[start..].starts_with("data:image/") {
            continue;
        }
        let len = text[start..]
            .find(['"', '\'', ' ', '>'])
            .unwrap_or(text.len() - start);
        if len >= BIG_DATA_URI {
            return true;
        }
    }
    false
}

/// An `on…=` attribute on an element: `onload=`, `onclick=`, …
fn has_event_handler(text: &str) -> bool {
    text.match_indices(" on").any(|(at, _)| {
        let rest = &text[at + 3..];
        let name_len = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        name_len >= 3 && rest[name_len..].trim_start().starts_with('=')
    })
}

/// A `<form>` posting to an http(s) host outside the sender's registrable
/// domain, with a password field somewhere after it.
fn offsite_password_form(text: &str, sender_host: &str) -> bool {
    let sender = registrable(sender_host.trim().trim_end_matches('.'));
    text.match_indices("<form").any(|(at, _)| {
        let tag_end = text[at..].find('>').map_or(text.len(), |e| at + e);
        let tag = &text[at..tag_end];
        let Some(action) = attr(tag, "action") else {
            return false;
        };
        let Some(rest) = action
            .strip_prefix("https://")
            .or_else(|| action.strip_prefix("http://"))
        else {
            return false;
        };
        let host = rest.split(['/', '?', '#', ':']).next().unwrap_or("");
        let form_end = text[tag_end..]
            .find("</form")
            .map_or(text.len(), |e| tag_end + e);
        let has_password = text[tag_end..form_end].contains("type=\"password\"")
            || text[tag_end..form_end].contains("type='password'")
            || text[tag_end..form_end].contains("type=password");
        has_password && !host.is_empty() && registrable(host) != sender
    })
}

/// The value of `name="…"` in a lowercased tag.
fn attr<'t>(tag: &'t str, name: &str) -> Option<&'t str> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    match rest.chars().next()? {
        q @ ('"' | '\'') => rest[1..].split(q).next(),
        _ => rest.split([' ', '>']).next(),
    }
}

#[cfg(test)]
mod tests;
