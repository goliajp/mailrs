//! `Email/get`: a message as RFC 8621 §4.1 describes it, read from the
//! core's row for its id and from the message file for headers and body.

use mailrs_core_api::method::message::MessageWire;
use mailrs_core_api::method::message_index::IndexMessage;
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Map, Value, json};

use super::ctx::{Ctx, requested_ids};
use super::ids::{self, Blob};
use super::view::View;

/// Bytes of a body value returned when the client sets no limit.
const DEFAULT_MAX_BODY: usize = 256 * 1024;

/// RFC 8621 §4.2: what `properties: null` returns. `headers` and
/// `bodyStructure` are asked for by name.
const DEFAULT_PROPERTIES: [&str; 24] = [
    "id",
    "blobId",
    "threadId",
    "mailboxIds",
    "keywords",
    "size",
    "receivedAt",
    "messageId",
    "inReplyTo",
    "references",
    "sender",
    "from",
    "to",
    "cc",
    "bcc",
    "replyTo",
    "subject",
    "sentAt",
    "hasAttachment",
    "preview",
    "bodyValues",
    "textBody",
    "htmlBody",
    "attachments",
];

fn rfc3339(epoch: i64) -> Value {
    chrono::DateTime::from_timestamp(epoch.max(0), 0)
        .map(|t| json!(t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)))
        .unwrap_or(Value::Null)
}

/// `Name <a@b>, c@d` as JMAP EmailAddress objects.
fn addresses(raw: Option<&str>) -> Value {
    let Some(raw) = raw else {
        return Value::Null;
    };
    let list: Vec<Value> = mailrs_rfc5322::split_list(raw)
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| {
            let email = mailrs_rfc5322::addr_spec(a).trim().to_string();
            let name = a
                .split_once('<')
                .map(|(n, _)| n.trim().trim_matches('"').trim())
                .filter(|n| !n.is_empty())
                .map(|n| mailrs_rfc2047::decode(n.as_bytes()).into_owned());
            json!({ "name": name, "email": email })
        })
        .collect();
    Value::Array(list)
}

/// `<a@b> <c@d>` as `["a@b", "c@d"]`.
fn message_ids(raw: Option<&str>) -> Value {
    let Some(raw) = raw else {
        return Value::Null;
    };
    let ids: Vec<&str> = raw
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|s| s.trim_matches(|c| c == '<' || c == '>'))
        .filter(|s| !s.is_empty())
        .collect();
    match ids.is_empty() {
        true => Value::Null,
        false => json!(ids),
    }
}

/// One leaf part as an EmailBodyPart.
fn body_part(uid: u32, n: usize, p: &mailrs_mime::Part<'_>) -> Value {
    json!({
        "partId": n.to_string(),
        "blobId": ids::blob(&Blob::Part(uid, n)),
        "size": p.body.len(),
        "type": p.content_type.mime_type(),
        "charset": (p.content_type.type_ == "text").then(|| p.content_type.charset().to_string()),
        "disposition": p.disposition.as_ref().map(|d| d.kind.to_string()),
        "cid": p.content_id.as_deref().map(|c| c.trim_matches(|c| c == '<' || c == '>').to_string()),
        "name": p.attachment_filename(),
        "language": null,
        "location": null,
    })
}

/// The leaf parts of a message in walk order: the numbering `partId` and
/// the part blob ids share.
pub fn leaves<'p, 'a>(root: &'p mailrs_mime::Part<'a>) -> Vec<&'p mailrs_mime::Part<'a>> {
    root.walk().filter(|p| p.children.is_empty()).collect()
}

struct Body {
    text: Vec<usize>,
    html: Vec<usize>,
    attachments: Vec<usize>,
}

fn classify(parts: &[&mailrs_mime::Part<'_>]) -> Body {
    let mut b = Body {
        text: Vec::new(),
        html: Vec::new(),
        attachments: Vec::new(),
    };
    for (n, p) in parts.iter().enumerate() {
        let attached = p.is_attachment() || p.is_inline_resource();
        match (attached, p.content_type.mime_type().as_str()) {
            (false, "text/plain") => b.text.push(n),
            (false, "text/html") => b.html.push(n),
            _ => b.attachments.push(n),
        }
    }
    b
}

fn truncate(s: String, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// The object for one message. `m` is where the view has it, `raw` the
/// message file when it could be read.
fn object(
    view: &View,
    m: &IndexMessage,
    w: &MessageWire,
    raw: Option<&[u8]>,
    args: &Value,
    props: &[String],
) -> Value {
    let mailbox_ids: Map<String, Value> = view
        .places(m)
        .into_iter()
        .map(|p| (p.id(), Value::Bool(true)))
        .collect();
    let mut o = Map::new();
    o.insert("id".into(), json!(ids::email(w.uid)));
    let head = raw.map(mailrs_rfc5322::Message::new);
    let header = |name: &str| head.as_ref().and_then(|m| m.header_str(name));
    let parsed = raw.map(mailrs_mime::parse);
    let parts = parsed.as_ref().map(leaves).unwrap_or_default();
    let body = classify(&parts);
    let max = args
        .get("maxBodyValueBytes")
        .and_then(|v| v.as_u64())
        .filter(|m| *m > 0)
        .map_or(DEFAULT_MAX_BODY, |m| m as usize);
    let all_values = args.get("fetchAllBodyValues").and_then(|v| v.as_bool()) == Some(true);
    let text_values =
        all_values || args.get("fetchTextBodyValues").and_then(|v| v.as_bool()) == Some(true);
    let html_values =
        all_values || args.get("fetchHTMLBodyValues").and_then(|v| v.as_bool()) == Some(true);
    for p in props {
        let v = match p.as_str() {
            "id" => continue,
            "blobId" => json!(ids::blob(&Blob::Message(w.uid))),
            "threadId" => json!(ids::thread(view.thread_key(m))),
            "mailboxIds" => Value::Object(mailbox_ids.clone()),
            "keywords" => mailrs_jmap::flags::flags_to_keywords(m.flags),
            "size" => json!(w.size),
            "receivedAt" => rfc3339(w.internal_date),
            "sentAt" => rfc3339(w.date),
            "messageId" => message_ids(header("Message-ID")),
            "inReplyTo" => message_ids(header("In-Reply-To")),
            "references" => message_ids(header("References")),
            "sender" => addresses(header("Sender")),
            "from" => addresses(header("From").or(Some(w.sender.as_str()))),
            "to" => addresses(header("To")),
            "cc" => addresses(header("Cc")),
            "bcc" => addresses(header("Bcc")),
            "replyTo" => addresses(header("Reply-To")),
            "subject" => json!(
                header("Subject")
                    .map(|s| mailrs_rfc2047::decode(s.as_bytes()).into_owned())
                    .unwrap_or_else(|| w.subject.clone())
            ),
            "hasAttachment" => json!(!body.attachments.is_empty()),
            "preview" => {
                let text = body
                    .text
                    .first()
                    .and_then(|n| parts[*n].body_text())
                    .unwrap_or_default();
                let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
                json!(truncate(flat, 256).0)
            }
            "textBody" => json!(
                body.text
                    .iter()
                    .map(|n| body_part(w.uid, *n, parts[*n]))
                    .collect::<Vec<_>>()
            ),
            "htmlBody" => json!(
                body.html
                    .iter()
                    .map(|n| body_part(w.uid, *n, parts[*n]))
                    .collect::<Vec<_>>()
            ),
            "attachments" => json!(
                body.attachments
                    .iter()
                    .map(|n| body_part(w.uid, *n, parts[*n]))
                    .collect::<Vec<_>>()
            ),
            "bodyStructure" => json!({
                "partId": null,
                "type": parsed.as_ref().map(|r| r.content_type.mime_type()),
                "subParts": parts.iter().enumerate().map(|(n, p)| body_part(w.uid, n, p)).collect::<Vec<_>>(),
            }),
            "bodyValues" => {
                let mut values = Map::new();
                let wanted = body
                    .text
                    .iter()
                    .filter(|_| text_values)
                    .chain(body.html.iter().filter(|_| html_values));
                for n in wanted {
                    let (value, cut) = truncate(parts[*n].body_text().unwrap_or_default(), max);
                    values.insert(
                        n.to_string(),
                        json!({ "value": value, "isEncodingProblem": false, "isTruncated": cut }),
                    );
                }
                Value::Object(values)
            }
            "headers" => {
                let list: Vec<Value> = head
                    .as_ref()
                    .map(|m| {
                        m.headers()
                            .map(|h| json!({ "name": h.name, "value": h.value_str().unwrap_or_default() }))
                            .collect()
                    })
                    .unwrap_or_default();
                Value::Array(list)
            }
            _ => continue,
        };
        o.insert(p.clone(), v);
    }
    Value::Object(o)
}

/// The message file, by the row's reference.
pub async fn raw_of(user: &str, w: &MessageWire) -> Option<Vec<u8>> {
    crate::handlers::messages::read_maildir_bytes_pub(user, &w.blob_ref)
        .await
        .ok()
}

pub async fn get(
    ctx: &Ctx,
    view: &View,
    args: &Value,
    state: &str,
) -> Result<(String, Value), JmapMethodError> {
    ctx.check_account(args)?;
    let Some(wanted) = requested_ids(args) else {
        return Err(JmapMethodError::Other {
            kind: "requestTooLarge".into(),
            description: "Email/get needs ids".into(),
        });
    };
    if wanted.len() > super::MAX_OBJECTS {
        return Err(super::limit_error("maxObjectsInGet"));
    }
    let props: Vec<String> = match args.get("properties").and_then(|v| v.as_array()) {
        Some(list) => list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        None => DEFAULT_PROPERTIES.iter().map(|s| s.to_string()).collect(),
    };
    let mut rows = Vec::new();
    let mut not_found = Vec::new();
    // only what the view holds exists: a held message, or another
    // account's uid, is not found
    for id in &wanted {
        let Some(m) = ids::parse_email(id).and_then(|uid| view.get(uid)) else {
            not_found.push(id.clone());
            continue;
        };
        match ctx
            .state
            .core
            .get_message_by_uid_for_user(&ctx.user, m.uid)
            .await
        {
            Ok(w) => rows.push((*m, w)),
            Err(_) => not_found.push(id.clone()),
        }
    }
    let needs_file = props.iter().any(|p| {
        !matches!(
            p.as_str(),
            "id" | "blobId"
                | "threadId"
                | "mailboxIds"
                | "keywords"
                | "size"
                | "receivedAt"
                | "sentAt"
        )
    });
    let mut list = Vec::new();
    for (m, w) in &rows {
        let raw = match needs_file {
            true => raw_of(&ctx.user, w).await,
            false => None,
        };
        list.push(object(view, m, w, raw.as_deref(), args, &props));
    }
    Ok((
        "Email/get".into(),
        json!({ "accountId": ctx.user, "state": state, "list": list, "notFound": not_found }),
    ))
}
