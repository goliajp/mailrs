//! `SearchSnippet/get` (RFC 8621 §5.1): where a search's words are in each
//! message's subject and body, marked with `<mark>` in HTML-escaped text.
//!
//! The words are the filter's `text`, `subject` and `body` conditions, at
//! any depth of operators. A message that does not contain them gets null
//! for the part it is not in.

use mailrs_jmap::error::JmapMethodError;
use serde_json::{Value, json};

use super::ctx::Ctx;
use super::ids;
use super::view::View;

/// Longest preview, in characters (RFC 8621: at most 255).
const PREVIEW_CHARS: usize = 255;

/// The search words of a filter: whitespace-separated, lowercased, quotes
/// dropped.
pub fn terms(filter: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect(filter, &mut out);
    out.sort();
    out.dedup();
    out
}

fn collect(f: &Value, out: &mut Vec<String>) {
    if let Some(list) = f.get("conditions").and_then(|v| v.as_array()) {
        for c in list {
            collect(c, out);
        }
        return;
    }
    for key in ["text", "subject", "body"] {
        if let Some(s) = f.get(key).and_then(|v| v.as_str()) {
            out.extend(
                s.split_whitespace()
                    .map(|w| w.trim_matches('"').to_lowercase())
                    .filter(|w| !w.is_empty()),
            );
        }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Byte ranges of every term in `text`, case-insensitively, merged.
fn hits(text: &str, terms: &[String]) -> Vec<(usize, usize)> {
    let lower = text.to_lowercase();
    // lowercasing can change byte lengths; only search when it did not
    if lower.len() != text.len() {
        return Vec::new();
    }
    let mut found: Vec<(usize, usize)> = Vec::new();
    for t in terms {
        let mut at = 0;
        while let Some(i) = lower[at..].find(t.as_str()) {
            let start = at + i;
            found.push((start, start + t.len()));
            at = start + t.len().max(1);
        }
    }
    found.sort();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in found {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

/// `text` escaped, with each hit in `<mark>`; null when nothing matched.
pub fn marked(text: &str, terms: &[String]) -> Value {
    let found = hits(text, terms);
    if found.is_empty() {
        return Value::Null;
    }
    let mut out = String::new();
    let mut at = 0;
    for (s, e) in found {
        out.push_str(&escape(&text[at..s]));
        out.push_str("<mark>");
        out.push_str(&escape(&text[s..e]));
        out.push_str("</mark>");
        at = e;
    }
    out.push_str(&escape(&text[at..]));
    json!(out)
}

/// A window of the body around its first hit, marked; null when the body
/// does not contain any term.
pub fn preview(body: &str, terms: &[String]) -> Value {
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some((first, _)) = hits(&flat, terms).first().copied() else {
        return Value::Null;
    };
    let chars: Vec<(usize, char)> = flat.char_indices().collect();
    let at = chars.iter().position(|(b, _)| *b >= first).unwrap_or(0);
    let from = at.saturating_sub(PREVIEW_CHARS / 4);
    let to = (from + PREVIEW_CHARS).min(chars.len());
    let start = chars[from].0;
    let end = chars.get(to).map_or(flat.len(), |(b, _)| *b);
    marked(&flat[start..end], terms)
}

/// The body text a snippet is cut from: the first plain-text part, or the
/// first HTML part with its tags taken out.
fn body_text(raw: &[u8]) -> String {
    let parsed = mailrs_mime::parse(raw);
    if let Some(t) = parsed
        .find_by_content_type("text/plain")
        .and_then(|p| p.body_text())
    {
        return t;
    }
    let html = parsed
        .find_by_content_type("text/html")
        .and_then(|p| p.body_text())
        .unwrap_or_default();
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => in_tag = true,
            (true, '>') => {
                in_tag = false;
                out.push(' ');
            }
            (false, _) => out.push(c),
            (true, _) => {}
        }
    }
    out
}

pub async fn get(ctx: &Ctx, view: &View, args: &Value) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let Some(wanted) = args.get("emailIds").and_then(|v| v.as_array()) else {
        return Err(JmapMethodError::InvalidArguments("emailIds".into()));
    };
    if wanted.len() > super::MAX_OBJECTS {
        return Err(super::limit_error("maxObjectsInGet"));
    }
    let words = terms(args.get("filter").unwrap_or(&Value::Null));
    let (mut list, mut not_found) = (Vec::new(), Vec::new());
    for id in wanted.iter().filter_map(|v| v.as_str()) {
        let Some(m) = ids::parse_email(id).and_then(|uid| view.get(uid)) else {
            not_found.push(id.to_string());
            continue;
        };
        let Ok(w) = ctx
            .state
            .core
            .get_message_by_uid_for_user(&ctx.user, m.uid)
            .await
        else {
            not_found.push(id.to_string());
            continue;
        };
        let raw = super::email::raw_of(&ctx.user, &w).await;
        let subject = raw
            .as_deref()
            .and_then(|r| {
                mailrs_rfc5322::Message::new(r)
                    .header_str("Subject")
                    .map(|s| mailrs_rfc2047::decode(s.as_bytes()).into_owned())
            })
            .unwrap_or_else(|| w.subject.clone());
        let body = raw.as_deref().map(body_text).unwrap_or_default();
        list.push(json!({
            "emailId": id,
            "subject": marked(&subject, &words),
            "preview": preview(&body, &words),
        }));
    }
    Ok((
        "SearchSnippet/get".into(),
        json!({ "accountId": ctx.user, "list": list, "notFound": not_found }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_of_a_filter_are_marked_and_escaped() {
        let f = json!({ "operator": "AND", "conditions": [
            { "text": "Invoice" }, { "subject": "\"march\"" }, { "from": "x@y" }
        ]});
        let t = terms(&f);
        assert_eq!(t, ["invoice", "march"]);
        assert_eq!(
            marked("Your <March> invoice", &t),
            json!("Your &lt;<mark>March</mark>&gt; <mark>invoice</mark>")
        );
        assert_eq!(marked("nothing here", &t), Value::Null);
        let long = format!(
            "{} the invoice is due {}",
            "word ".repeat(200),
            "tail ".repeat(200)
        );
        let p = preview(&long, &t);
        let p = p.as_str().unwrap();
        assert!(p.contains("<mark>invoice</mark>"), "{p}");
        assert!(
            p.replace("<mark>", "")
                .replace("</mark>", "")
                .chars()
                .count()
                <= PREVIEW_CHARS
        );
        assert_eq!(preview("no hit", &t), Value::Null);
    }

    #[test]
    fn html_bodies_are_read_without_their_tags() {
        let raw = b"Content-Type: text/html\r\n\r\n<p>The <b>invoice</b> is here</p>";
        assert!(body_text(raw).contains("invoice"));
        assert!(!body_text(raw).contains('<'));
    }
}
