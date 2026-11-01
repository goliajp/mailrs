//! `EmailSubmission/set` (create only), and building a message from an
//! Email object for `Email/set` create.
//!
//! A submission sends the stored message as it is, Bcc header removed,
//! through the same queue, sender checks and Send record as the web
//! client's send.

use axum::http::HeaderMap;
use mailrs_jmap::error::JmapMethodError;
use serde_json::{Map, Value, json};

use super::ctx::Ctx;
use super::email_set;
use super::identity;
use super::ids;
use super::import::{NotFiled, recipients};
use super::view::View;
use crate::handlers::compose::{ComposeParts, build_rfc5322};

fn addresses(spec: &Value, key: &str) -> Vec<String> {
    spec.get(key)
        .and_then(|v| v.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let email = a.get("email")?.as_str()?;
                    match a
                        .get("name")
                        .and_then(|n| n.as_str())
                        .filter(|n| !n.is_empty())
                    {
                        Some(name) => Some(format!("{name} <{email}>")),
                        None => Some(email.to_string()),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The text of the first body part of `kind` (`textBody` / `htmlBody`),
/// from `bodyValues`.
fn body(spec: &Value, kind: &str) -> String {
    let part = spec
        .get(kind)
        .and_then(|v| v.as_array())
        .and_then(|a| a.first());
    let id = part.and_then(|p| p.get("partId")).and_then(|v| v.as_str());
    id.and_then(|id| spec.get("bodyValues")?.get(id)?.get("value")?.as_str())
        .unwrap_or_default()
        .to_string()
}

/// RFC 5322 bytes for an Email object: from, to, cc, bcc, subject,
/// inReplyTo and one text and/or HTML body.
pub fn compose(spec: &Value) -> Result<Vec<u8>, String> {
    if spec
        .get("attachments")
        .is_some_and(|a| a.as_array().is_some_and(|a| !a.is_empty()))
    {
        return Err("attachments: upload the whole message and use Email/import".into());
    }
    let from = addresses(spec, "from")
        .into_iter()
        .next()
        .ok_or("from is required")?;
    let bcc = addresses(spec, "bcc");
    let parts = ComposeParts {
        from: from.clone(),
        to: addresses(spec, "to"),
        cc: addresses(spec, "cc"),
        bcc: bcc.clone(),
        subject: spec
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        body: body(spec, "textBody"),
        html_body: body(spec, "htmlBody"),
        in_reply_to: spec
            .get("inReplyTo")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .map(|m| format!("<{m}>")),
        forward_message_id: None,
        forward_attachments_from: None,
        reply_to_thread_id: None,
        attachments: Vec::new(),
        scheduled_at: None,
    };
    // each of these becomes a header line as it is
    let header_values = parts
        .to
        .iter()
        .chain(&parts.cc)
        .chain(&parts.bcc)
        .chain([&parts.from, &parts.subject])
        .chain(parts.in_reply_to.as_ref());
    if header_values.into_iter().any(|v| v.contains(['\r', '\n'])) {
        return Err("a header value carries a line break".into());
    }
    if parts.body.is_empty() && parts.html_body.is_empty() {
        return Err("a textBody or htmlBody with its bodyValues is required".into());
    }
    let (_, mut bytes) = build_rfc5322(&parts, &from);
    // kept on the stored copy so a submission knows to send it there
    if !bcc.is_empty() {
        let line = format!("Bcc: {}\r\n", bcc.join(", "));
        bytes.splice(0..0, line.into_bytes());
    }
    Ok(bytes)
}

/// `bytes` without its Bcc header (RFC 8621 §7.5: it is removed before
/// delivery).
pub fn without_bcc(bytes: &[u8]) -> Vec<u8> {
    let end = mailrs_rfc5322::Message::new(bytes)
        .body_offset()
        .unwrap_or(bytes.len());
    let mut out = Vec::with_capacity(bytes.len());
    let mut skipping = false;
    for line in bytes[..end].split_inclusive(|b| *b == b'\n') {
        let folded = line.first().is_some_and(|b| *b == b' ' || *b == b'\t');
        if !folded {
            skipping = line.len() >= 4 && line[..4].eq_ignore_ascii_case(b"bcc:");
        }
        if !skipping {
            out.extend_from_slice(line);
        }
    }
    out.extend_from_slice(&bytes[end..]);
    out
}

fn refuse(kind: &'static str, why: impl Into<String>) -> NotFiled {
    NotFiled(kind, why.into())
}

/// Send one submission; its id.
async fn submit(ctx: &Ctx, view: &View, spec: &Value) -> Result<(String, u32), NotFiled> {
    let email_id = spec
        .get("emailId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let uid = ids::parse_email(email_id)
        .filter(|u| view.get(*u).is_some())
        .ok_or_else(|| refuse("invalidProperties", format!("no email {email_id}")))?;
    let identity_id = spec
        .get("identityId")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let identity = identity::address_of(ctx, identity_id)
        .await
        .ok_or_else(|| refuse("invalidProperties", format!("no identity {identity_id}")))?;
    let core = &ctx.state.core;
    let w = core
        .get_message_by_uid_for_user(&ctx.user, uid)
        .await
        .map_err(|e| refuse("serverFail", e.to_string()))?;
    let raw = super::email::raw_of(&ctx.user, &w)
        .await
        .ok_or_else(|| refuse("serverFail", "the message file cannot be read"))?;
    let head = mailrs_rfc5322::Message::new(&raw);
    let from = head
        .header_str("From")
        .map(|f| mailrs_rfc5322::addr_spec(f).trim().to_string())
        .unwrap_or_default();
    if !from.eq_ignore_ascii_case(&identity) {
        return Err(refuse(
            "forbiddenFrom",
            format!("From is {from}, the identity is {identity}"),
        ));
    }
    crate::handlers::compose::ensure_from_allowed(&ctx.state, &ctx.user, &from)
        .await
        .map_err(|_| {
            refuse(
                "forbiddenFrom",
                format!("{from} is not an address of this account"),
            )
        })?;
    crate::handlers::send_queue::ensure_not_paused(&ctx.user, &from).map_err(|_| {
        refuse(
            "forbiddenToSend",
            mailrs_auth_guard::password::outbound_paused_notice(),
        )
    })?;
    let rcpts: Vec<String> = match spec
        .get("envelope")
        .and_then(|e| e.get("rcptTo"))
        .and_then(|v| v.as_array())
    {
        Some(list) => list
            .iter()
            .filter_map(|r| r.get("email")?.as_str().map(|s| s.trim().to_string()))
            .collect(),
        None => recipients(&head),
    };
    if rcpts.is_empty()
        || rcpts
            .iter()
            .any(|r| !r.contains('@') || r.contains(['\r', '\n']))
    {
        return Err(refuse("noRecipients", "no valid recipients"));
    }
    let to = recipients_in(&head, "To");
    let cc = recipients_in(&head, "Cc");
    let message_id = match w.message_id.is_empty() {
        true => format!("jmap-{uid}"),
        false => w.message_id.clone(),
    };
    crate::handlers::send::record_key_send(
        ctx.key.as_ref(),
        &ctx.user,
        &HeaderMap::new(),
        &message_id,
    )
    .map_err(|_| refuse("serverFail", "the send could not be recorded"))?;
    let meta = crate::handlers::compose::SendMeta {
        message_id: &message_id,
        thread_id: &w.thread_id,
        subject: &w.subject,
        to: &to,
        cc: &cc,
        envelope_ref: &w.blob_ref,
        resent_from: None,
        key_id: ctx.key.as_ref().map(|k| k.id),
    };
    crate::handlers::send_queue::enqueue_outbound_at(
        &ctx.user,
        &rcpts,
        &without_bcc(&raw),
        None,
        Some(meta),
    )
    .map_err(|_| refuse("serverFail", "the message could not be queued"))?;
    Ok((format!("S{uid}"), uid))
}

fn recipients_in(head: &mailrs_rfc5322::Message<'_>, name: &str) -> Vec<String> {
    head.header_str(name)
        .map(|v| {
            mailrs_rfc5322::split_list(v)
                .map(|a| mailrs_rfc5322::addr_spec(a).trim().to_string())
                .filter(|a| a.contains('@'))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn set(
    ctx: &Ctx,
    view: &View,
    args: &Value,
    state: &str,
) -> Result<Vec<(String, Value)>, JmapMethodError> {
    ctx.check_account(args)?;
    if args
        .get("update")
        .is_some_and(|v| v.as_object().is_some_and(|m| !m.is_empty()))
        || args
            .get("destroy")
            .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
    {
        return Err(JmapMethodError::InvalidArguments(
            "submissions are sent at once and cannot be changed or cancelled".into(),
        ));
    }
    let creates = args
        .get("create")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    if creates.len() > super::MAX_OBJECTS {
        return Err(super::limit_error("maxObjectsInSet"));
    }
    let (mut created, mut not_created) = (Map::new(), Map::new());
    // creation id → the email it sent
    let mut sent: Vec<(String, u32)> = Vec::new();
    for (cid, spec) in creates {
        match submit(ctx, view, &spec).await {
            Ok((id, uid)) => {
                created.insert(cid.clone(), json!({ "id": id, "undoStatus": "final" }));
                sent.push((cid, uid));
            }
            Err(e) => {
                not_created.insert(cid, e.to_json());
            }
        }
    }
    let mut out = vec![(
        "EmailSubmission/set".to_string(),
        json!({
            "accountId": ctx.user,
            "oldState": state,
            "newState": state,
            "created": created,
            "notCreated": not_created,
        }),
    )];
    // RFC 8621 §7.5: the implicit Email/set, answered after this one
    let email_of = |key: &str| {
        let cid = key.strip_prefix('#')?;
        sent.iter()
            .find(|(c, _)| c == cid)
            .map(|(_, uid)| ids::email(*uid))
    };
    let mut update = Map::new();
    if let Some(patches) = args.get("onSuccessUpdateEmail").and_then(|v| v.as_object()) {
        for (key, patch) in patches {
            if let Some(id) = email_of(key) {
                update.insert(id, patch.clone());
            }
        }
    }
    let destroy: Vec<Value> = args
        .get("onSuccessDestroyEmail")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|k| k.as_str())
                .filter_map(email_of)
                .map(Value::String)
                .collect()
        })
        .unwrap_or_default();
    if !update.is_empty() || !destroy.is_empty() {
        let implicit = json!({ "accountId": ctx.user, "update": update, "destroy": destroy });
        out.push(email_set::set(ctx, view, &implicit, state).await?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcc_is_removed_with_its_continuation_lines() {
        let raw = b"From: a@x.test\r\nBcc: hidden@y.test,\r\n  more@y.test\r\nTo: b@y.test\r\n\r\nBcc: in the body stays\r\n";
        assert_eq!(
            without_bcc(raw),
            b"From: a@x.test\r\nTo: b@y.test\r\n\r\nBcc: in the body stays\r\n".to_vec()
        );
    }

    #[test]
    fn an_email_object_becomes_a_message() {
        let spec = json!({
            "from": [{ "name": "A", "email": "a@x.test" }],
            "to": [{ "email": "b@y.test" }],
            "bcc": [{ "email": "c@y.test" }],
            "subject": "hello",
            "textBody": [{ "partId": "1", "type": "text/plain" }],
            "bodyValues": { "1": { "value": "the body" } },
        });
        let bytes = compose(&spec).expect("built");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("Bcc: c@y.test\r\n"), "{text}");
        assert!(text.contains("Subject: hello"), "{text}");
        let parsed = mailrs_mime::parse(&bytes);
        let body = parsed
            .find_by_content_type("text/plain")
            .and_then(|p| p.body_text());
        assert_eq!(body.as_deref().map(str::trim), Some("the body"), "{text}");
        assert!(compose(&json!({ "from": [{ "email": "a@x.test" }] })).is_err());
        assert!(compose(&json!({ "textBody": [] })).is_err());
        let mut injected = spec.clone();
        injected["subject"] = json!("hi\r\nBcc: everyone@z.test");
        assert!(compose(&injected).is_err());
    }
}
