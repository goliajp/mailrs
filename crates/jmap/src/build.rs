//! Build JMAP Email object from a [`Message`] (+ optional parsed body).
//!
//! This is split into two parts:
//! - [`build_email_meta`] returns headers + metadata only; no I/O.
//! - [`extend_with_body`] mutates the object with body / attachment fields once
//!   the caller has parsed the raw bytes via [`crate::store::MailStore`].
//!
//! Handlers call them in sequence so a `properties:` selector that doesn't
//! ask for any body field can skip the disk read entirely.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::flags::flags_to_keywords;
use crate::ids::format_mailbox_id;
use crate::types::{Message, ParsedBody};

/// Body-related JMAP properties. If `properties` is `None` (include-all) or
/// intersects this set, the dispatcher will read + parse the raw message.
const BODY_PROPS: &[&str] = &["bodyValues", "textBody", "htmlBody", "attachments"];

/// Returns true when at least one body-related property is requested.
pub fn wants_body(properties: &Option<Vec<&str>>) -> bool {
    let Some(props) = properties.as_ref() else {
        return true;
    };
    BODY_PROPS.iter().any(|b| props.contains(b))
}

/// Build the header / metadata portion of an Email JMAP object.
///
/// `email_id` is the wire-visible id (`msg-{id}`).
pub fn build_email_meta(msg: &Message, email_id: &str, properties: &Option<Vec<&str>>) -> Value {
    let include_all = properties.is_none();
    let props = properties.as_ref();
    let wants = |name: &str| include_all || props.is_some_and(|p| p.contains(&name));

    let mut obj = Map::new();
    obj.insert("id".to_string(), Value::String(email_id.to_string()));

    if wants("threadId") {
        obj.insert("threadId".to_string(), Value::String(msg.thread_id.clone()));
    }

    if wants("mailboxIds") {
        let mb_key = format_mailbox_id(msg.mailbox_id);
        obj.insert(
            "mailboxIds".to_string(),
            serde_json::json!({ mb_key: true }),
        );
    }

    if wants("keywords") {
        obj.insert("keywords".to_string(), flags_to_keywords(msg.flags));
    }

    if wants("from") {
        obj.insert("from".to_string(), parse_address_list(&msg.sender));
    }

    if wants("to") {
        obj.insert("to".to_string(), parse_address_list(&msg.recipients));
    }

    if wants("subject") {
        obj.insert("subject".to_string(), Value::String(msg.subject.clone()));
    }

    if wants("receivedAt") {
        obj.insert(
            "receivedAt".to_string(),
            Value::String(epoch_to_utc_string(msg.internal_date)),
        );
    }

    if wants("sentAt") {
        obj.insert(
            "sentAt".to_string(),
            Value::String(epoch_to_utc_string(msg.date)),
        );
    }

    if wants("size") {
        obj.insert("size".to_string(), Value::Number(msg.size.into()));
    }

    if wants("messageId") {
        obj.insert("messageId".to_string(), serde_json::json!([msg.message_id]));
    }

    if wants("inReplyTo") {
        if msg.in_reply_to.is_empty() {
            obj.insert("inReplyTo".to_string(), Value::Null);
        } else {
            obj.insert(
                "inReplyTo".to_string(),
                serde_json::json!([msg.in_reply_to]),
            );
        }
    }

    if wants("preview") {
        let preview = msg
            .new_content
            .as_deref()
            .unwrap_or(&msg.subject)
            .chars()
            .take(256)
            .collect::<String>();
        obj.insert("preview".to_string(), Value::String(preview));
    }

    if wants("hasAttachment") {
        // Conservative default; `extend_with_body` upgrades it once we've
        // actually parsed the bytes.
        obj.insert("hasAttachment".to_string(), Value::Bool(false));
    }

    Value::Object(obj)
}

/// Append body / attachment fields to a previously-built Email object using
/// a [`ParsedBody`]. `parsed` of `None` means the raw bytes weren't readable;
/// the handler still emits empty body fields rather than dropping the email.
pub fn extend_with_body(
    obj: &mut Value,
    parsed: Option<&ParsedBody>,
    properties: &Option<Vec<&str>>,
) {
    let include_all = properties.is_none();
    let props = properties.as_ref();
    let wants = |name: &str| include_all || props.is_some_and(|p| p.contains(&name));

    let Some(map) = obj.as_object_mut() else {
        return;
    };

    if let Some(parsed) = parsed {
        if wants("bodyValues") {
            let mut bv = Map::new();
            if let Some(ref text) = parsed.text {
                bv.insert(
                    "t1".to_string(),
                    serde_json::json!({"value": text, "isEncodingProblem": false, "isTruncated": false}),
                );
            }
            if let Some(ref html) = parsed.html {
                bv.insert(
                    "h1".to_string(),
                    serde_json::json!({"value": html, "isEncodingProblem": false, "isTruncated": false}),
                );
            }
            map.insert("bodyValues".to_string(), Value::Object(bv));
        }

        if wants("textBody") {
            if parsed.text.is_some() {
                map.insert(
                    "textBody".to_string(),
                    serde_json::json!([{"partId": "t1", "type": "text/plain"}]),
                );
            } else {
                map.insert("textBody".to_string(), serde_json::json!([]));
            }
        }

        if wants("htmlBody") {
            if parsed.html.is_some() {
                map.insert(
                    "htmlBody".to_string(),
                    serde_json::json!([{"partId": "h1", "type": "text/html"}]),
                );
            } else {
                map.insert("htmlBody".to_string(), serde_json::json!([]));
            }
        }

        if wants("attachments") {
            let att_list: Vec<Value> = parsed
                .attachments
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "name": a.filename,
                        "type": a.content_type,
                        "size": a.size
                    })
                })
                .collect();
            map.insert("attachments".to_string(), Value::Array(att_list));
            map.insert(
                "hasAttachment".to_string(),
                Value::Bool(!parsed.attachments.is_empty()),
            );
        }
    } else {
        if wants("bodyValues") {
            map.insert("bodyValues".to_string(), serde_json::json!({}));
        }
        if wants("textBody") {
            map.insert("textBody".to_string(), serde_json::json!([]));
        }
        if wants("htmlBody") {
            map.insert("htmlBody".to_string(), serde_json::json!([]));
        }
        if wants("attachments") {
            map.insert("attachments".to_string(), serde_json::json!([]));
        }
    }
}

/// Parse a comma-separated header address list into an array of `{name, email}`.
///
/// JMAP requires `EmailAddress` objects per RFC 8621 §4.1.2. This is a
/// best-effort split that handles the `Name <addr>` form; full RFC 5322
/// parsing belongs in the storage layer.
pub fn parse_address_list(raw: &str) -> Value {
    if raw.is_empty() {
        return Value::Array(vec![]);
    }
    let addrs: Vec<Value> = raw
        .split(',')
        .map(|s| {
            let s = s.trim();
            if let Some(pos) = s.find('<') {
                let name = s[..pos].trim().trim_matches('"').to_string();
                let email = s[pos + 1..].trim_end_matches('>').to_string();
                serde_json::json!({"name": name, "email": email})
            } else {
                serde_json::json!({"name": null, "email": s})
            }
        })
        .collect();
    Value::Array(addrs)
}

/// Render an epoch-seconds timestamp as an RFC 3339 UTC string. Empty when
/// the epoch is out of range — JMAP timestamps are required, but `0` happens
/// often enough during ingestion bugs that we don't want to panic.
pub fn epoch_to_utc_string(epoch: i64) -> String {
    DateTime::<Utc>::from_timestamp(epoch, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_message() -> Message {
        Message {
            id: 1,
            mailbox_id: 2,
            uid: 3,
            sender: "Alice <alice@example.com>".into(),
            recipients: "bob@example.com".into(),
            subject: "hi".into(),
            date: 1_700_000_000,
            size: 42,
            flags: crate::types::FLAG_SEEN,
            internal_date: 1_700_000_001,
            message_id: "abc@example.com".into(),
            in_reply_to: String::new(),
            thread_id: "t1".into(),
            user_address: "bob@example.com".into(),
            new_content: Some("snippet".into()),
            blob_id: "blob-1".into(),
        }
    }

    #[test]
    fn parse_address_with_display_name() {
        let v = parse_address_list("Alice <alice@example.com>");
        assert_eq!(v[0]["name"], "Alice");
        assert_eq!(v[0]["email"], "alice@example.com");
    }

    #[test]
    fn parse_address_bare() {
        let v = parse_address_list("alice@example.com");
        assert_eq!(v[0]["name"], Value::Null);
        assert_eq!(v[0]["email"], "alice@example.com");
    }

    #[test]
    fn parse_address_empty() {
        assert_eq!(parse_address_list(""), serde_json::json!([]));
    }

    #[test]
    fn parse_address_multiple() {
        let v = parse_address_list("alice@example.com, bob@example.com");
        assert_eq!(v.as_array().unwrap().len(), 2);
    }

    #[test]
    fn epoch_zero_renders() {
        let s = epoch_to_utc_string(0);
        assert!(s.starts_with("1970-01-01"));
    }

    #[test]
    fn epoch_out_of_range_is_empty() {
        assert_eq!(epoch_to_utc_string(i64::MAX), "");
    }

    #[test]
    fn build_email_meta_minimal_with_id() {
        let msg = sample_message();
        let v = build_email_meta(&msg, "msg-1", &Some(vec!["subject"]));
        assert_eq!(v["id"], "msg-1");
        assert_eq!(v["subject"], "hi");
        assert!(v.get("threadId").is_none());
    }

    #[test]
    fn build_email_meta_include_all() {
        let msg = sample_message();
        let v = build_email_meta(&msg, "msg-1", &None);
        assert_eq!(v["threadId"], "t1");
        assert_eq!(v["mailboxIds"]["mb-2"], true);
        assert_eq!(v["keywords"]["$seen"], true);
        assert_eq!(v["preview"], "snippet");
    }

    #[test]
    fn extend_with_body_text_and_html() {
        let msg = sample_message();
        let mut obj = build_email_meta(&msg, "msg-1", &None);
        let parsed = ParsedBody {
            text: Some("hello".into()),
            html: Some("<p>hi</p>".into()),
            attachments: vec![],
        };
        extend_with_body(&mut obj, Some(&parsed), &None);
        assert_eq!(obj["bodyValues"]["t1"]["value"], "hello");
        assert_eq!(obj["bodyValues"]["h1"]["value"], "<p>hi</p>");
        assert_eq!(obj["textBody"][0]["partId"], "t1");
        assert_eq!(obj["htmlBody"][0]["partId"], "h1");
    }

    #[test]
    fn extend_with_body_no_raw_yields_empty() {
        let msg = sample_message();
        let mut obj = build_email_meta(&msg, "msg-1", &None);
        extend_with_body(&mut obj, None, &None);
        assert_eq!(obj["bodyValues"], serde_json::json!({}));
        assert_eq!(obj["textBody"], serde_json::json!([]));
    }

    #[test]
    fn wants_body_logic() {
        assert!(wants_body(&None));
        assert!(wants_body(&Some(vec!["bodyValues"])));
        assert!(!wants_body(&Some(vec!["subject", "from"])));
    }

    #[test]
    fn in_reply_to_null_when_empty() {
        let msg = sample_message();
        let v = build_email_meta(&msg, "msg-1", &Some(vec!["inReplyTo"]));
        assert_eq!(v["inReplyTo"], Value::Null);
    }

    #[test]
    fn in_reply_to_array_when_set() {
        let mut msg = sample_message();
        msg.in_reply_to = "parent@example.com".into();
        let v = build_email_meta(&msg, "msg-1", &Some(vec!["inReplyTo"]));
        assert_eq!(v["inReplyTo"], serde_json::json!(["parent@example.com"]));
    }
}
