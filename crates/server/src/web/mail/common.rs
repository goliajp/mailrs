//! Shared helpers used across multiple `mail/` sub-modules.
//!
//! Anything in this file is reachable from sibling sub-modules via
//! `use super::common::*`. Items that need to remain callable from outside
//! the `mail::` module (e.g. `mcp/mod.rs`, `web/rsvp.rs`, `web/auth.rs`,
//! `web/jmap.rs`) are `pub(crate)` so the `pub(crate) use common::*` re-export
//! in `mod.rs` lifts them to the `mail::` path.

use std::sync::Arc;

use axum::Json;
use base64::Engine;
use rand_core::RngCore;


use super::{SendResult, WebState};

/// Attachment payload used by the send pipeline (multipart upload + forwarded
/// attachments + MCP send-with-attachments). Lives in `common.rs` because it
/// is shared by `send.rs` (handlers) and `crate::mcp::mod` (MCP tools).
pub(crate) struct AttachmentData {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// check if a sender address is allowed for the authenticated user
/// returns Ok(()) if allowed, Err(message) if not
pub(crate) fn verify_sender(
    from: &str,
    user: &str,
    permissions: &crate::permission::EffectivePermissions,
) -> Result<(), &'static str> {
    if from == user {
        return Ok(());
    }
    // check if from is an alias address owned by this user
    if permissions
        .send_as()
        .iter()
        .any(|a| a.eq_ignore_ascii_case(from))
    {
        return Ok(());
    }
    // super user or user with accessible domains
    let accessible = permissions.accessible_domains();
    if !accessible.is_empty()
        && let Some(domain) = from.rsplit_once('@').map(|(_, d)| d)
            && (permissions.is_super()
                || accessible.iter().any(|sd| sd.eq_ignore_ascii_case(domain)))
            {
                return Ok(());
            }
    Err("sender must match authenticated user")
}

/// resolve reply_to_thread_id into in_reply_to message-id and references
/// returns (resolved_in_reply_to, references)
pub(crate) async fn resolve_thread_reply(
    reply_to_thread_id: Option<&str>,
    in_reply_to: Option<&str>,
    user: &str,
    mb_store: Option<&mailrs_mailbox::PgMailboxStore>,
) -> (Option<String>, Vec<String>) {
    // explicit in_reply_to takes precedence
    if let Some(reply_to) = in_reply_to
        && !reply_to.is_empty() {
            let refs = match mb_store {
                Some(store) => store
                    .get_thread_references(user, reply_to)
                    .await
                    .unwrap_or_default(),
                None => vec![],
            };
            return (Some(reply_to.to_string()), refs);
        }

    // resolve thread_id to last message's message-id
    if let (Some(thread_id), Some(store)) = (reply_to_thread_id, mb_store)
        && !thread_id.is_empty()
            && let Ok(Some(last_msg_id)) = store.get_last_message_id_in_thread(user, thread_id).await {
                let refs = store
                    .get_thread_message_ids(user, thread_id)
                    .await
                    .unwrap_or_default();
                return (Some(last_msg_id), refs);
            }

    (None, vec![])
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
) -> Json<SendResult> {
    deliver_message_ex(state, from, to, cc, bcc, raw, message_id, ts, None).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn deliver_message_ex(
    state: &Arc<WebState>,
    from: &str,
    to: &[String],
    cc: &[String],
    bcc: &[String],
    raw: &[u8],
    message_id: &str,
    ts: i64,
    scheduled_at: Option<i64>,
) -> Json<SendResult> {
    let all_recipients: Vec<String> = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .map(|s| extract_address(s))
        .collect();

    let local_domains: Vec<String> = if let Some(ref ds) = state.domain_store {
        ds.list_domains()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.name)
            .collect()
    } else {
        vec![]
    };

    let mut errors = Vec::new();

    // resolve group emails to individual members
    let mut resolved_recipients = Vec::new();
    for rcpt in &all_recipients {
        if let Some(ref ds) = state.domain_store {
            match ds.resolve_recipient(rcpt).await {
                crate::domain_store::ResolvedRecipient::Group(members) => {
                    resolved_recipients.extend(members);
                }
                _ => resolved_recipients.push(rcpt.clone()),
            }
        } else {
            resolved_recipients.push(rcpt.clone());
        }
    }

    // deduplicate recipients (e.g. user both in a group and directly CC'd)
    resolved_recipients.sort_unstable();
    resolved_recipients.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    for rcpt in &resolved_recipients {
        let domain = rcpt.rsplit_once('@').map(|(_, d)| d).unwrap_or("");
        let is_local = local_domains
            .iter()
            .any(|d: &String| d.eq_ignore_ascii_case(domain));

        if is_local {
            if let Some(ref mb_store) = state.mailbox_store {
                let _ = mb_store.ensure_default_mailboxes(rcpt).await;
                if let Err(e) = mb_store
                    .append_message(rcpt, "INBOX", &state.maildir_root, raw, 0, ts)
                    .await
                {
                    errors.push(format!("{rcpt}: {e}"));
                }
            }
        } else if let Some(ref pool) = state.outbound_queue {
            let enqueue_result = if let Some(sched) = scheduled_at {
                mailrs_outbound_queue::queue::enqueue_scheduled(
                    pool, from, rcpt, domain, raw, Some(message_id), ts, sched,
                )
                .await
            } else {
                mailrs_outbound_queue::queue::enqueue(
                    pool, from, rcpt, domain, raw, Some(message_id), ts,
                )
                .await
            };
            if let Err(e) = enqueue_result {
                errors.push(format!("{rcpt}: {e}"));
            } else if let Some(ref vk) = state.valkey {
                mailrs_outbound_queue::queue::notify(&mut vk.clone()).await;
            }
        } else {
            errors.push(format!("{rcpt}: outbound queue not configured"));
        }
    }

    // save copy to Sent folder
    if let Some(ref mb_store) = state.mailbox_store {
        let _ = mb_store.ensure_default_mailboxes(from).await;
        let _ = mb_store
            .append_message(
                from,
                "Sent",
                &state.maildir_root,
                raw,
                mailrs_mailbox::FLAG_SEEN,
                ts,
            )
            .await;
    }

    if errors.is_empty() {
        Json(SendResult {
            success: true,
            message: None,
            message_id: Some(message_id.to_string()),
        })
    } else {
        Json(SendResult {
            success: false,
            message: Some(errors.join("; ")),
            message_id: None,
        })
    }
}

// extract bare email from "Display Name <addr>" or return as-is
pub(super) fn extract_address(s: &str) -> String {
    if let Some(start) = s.rfind('<')
        && let Some(end) = s[start..].find('>') {
            return s[start + 1..start + end].trim().to_string();
        }
    s.trim().to_string()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_rfc5322_message(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    html_body: Option<&str>,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    list_unsubscribe: Option<&str>,
) -> Vec<u8> {
    build_rfc5322_with_attachments(
        from,
        to,
        cc,
        subject,
        body,
        html_body,
        message_id,
        in_reply_to,
        references,
        date,
        &[],
        list_unsubscribe,
        &[],
        false,
    )
}

// build the text/plain + text/html alternative part
fn build_alternative_part(msg: &mut String, text: &str, html: &str) {
    let alt_boundary = format!("----=_Alt_{}", rand_core::OsRng.next_u64());
    msg.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{alt_boundary}\"\r\n\r\n"
    ));
    // text/plain
    msg.push_str(&format!("--{alt_boundary}\r\n"));
    msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    msg.push_str(text);
    msg.push_str("\r\n");
    // text/html
    msg.push_str(&format!("--{alt_boundary}\r\n"));
    msg.push_str("Content-Type: text/html; charset=utf-8\r\n");
    msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
    msg.push_str(html);
    msg.push_str("\r\n");
    msg.push_str(&format!("--{alt_boundary}--\r\n"));
}

/// wrap editor html in a minimal email-safe template with inline styles
pub(super) fn wrap_email_html(html: &str) -> String {
    format!(
        "<!DOCTYPE html>\
<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<style>\
body{{margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;font-size:14px;line-height:1.6;color:#1a1a1a;background:#fff}}\
.wrapper{{max-width:600px;margin:0 auto;padding:16px}}\
pre{{background:#1e1e2e;color:#cdd6f4;padding:12px 16px;border-radius:6px;overflow-x:auto;font-family:'SF Mono',Monaco,Consolas,'Liberation Mono',monospace;font-size:13px;line-height:1.5}}\
code{{font-family:'SF Mono',Monaco,Consolas,'Liberation Mono',monospace;font-size:13px}}\
:not(pre)>code{{background:#f0f0f0;padding:2px 4px;border-radius:3px;font-size:0.9em}}\
blockquote{{border-left:3px solid #d4d4d8;padding-left:12px;margin:8px 0;color:#71717a}}\
img{{max-width:100%;height:auto}}\
table{{border-collapse:collapse;width:100%}}\
th,td{{border:1px solid #d4d4d8;padding:6px 12px;text-align:left}}\
th{{background:#f4f4f5}}\
a{{color:#2563eb}}\
ul[data-type=\"taskList\"]{{list-style:none;padding-left:0}}\
ul[data-type=\"taskList\"] li{{display:flex;align-items:flex-start;gap:4px}}\
h1{{font-size:1.5em}} h2{{font-size:1.3em}} h3{{font-size:1.1em}}\
</style></head><body><div class=\"wrapper\">{html}</div></body></html>"
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_rfc5322_with_attachments(
    from: &str,
    to: &[String],
    cc: &[String],
    subject: &str,
    body: &str,
    html_body: Option<&str>,
    message_id: &str,
    in_reply_to: Option<&str>,
    references: &[String],
    date: &chrono::DateTime<chrono::Utc>,
    attachments: &[AttachmentData],
    list_unsubscribe: Option<&str>,
    inline_images: &[crate::inline_image::InlineImage],
    request_read_receipt: bool,
) -> Vec<u8> {
    let date_str = date.format("%a, %d %b %Y %H:%M:%S %z").to_string();
    let mut msg = format!(
        "Date: {date_str}\r\n\
         From: {from}\r\n\
         To: {}\r\n",
        to.join(", ")
    );
    if !cc.is_empty() {
        msg.push_str(&format!("Cc: {}\r\n", cc.join(", ")));
    }
    let encoded_subject = mailrs_rfc2047::encode(subject);
    msg.push_str(&format!(
        "Subject: {encoded_subject}\r\n\
         Message-ID: <{message_id}>\r\n\
         MIME-Version: 1.0\r\n"
    ));
    if let Some(ref_id) = in_reply_to {
        msg.push_str(&format!("In-Reply-To: <{ref_id}>\r\n"));
    }
    if !references.is_empty() {
        let refs_str = references
            .iter()
            .map(|r| format!("<{r}>"))
            .collect::<Vec<_>>()
            .join(" ");
        msg.push_str(&format!("References: {refs_str}\r\n"));
    } else if let Some(ref_id) = in_reply_to {
        msg.push_str(&format!("References: <{ref_id}>\r\n"));
    }
    if let Some(unsub_url) = list_unsubscribe {
        msg.push_str(&format!("List-Unsubscribe: <{unsub_url}>\r\n"));
        msg.push_str("List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n");
    }
    if request_read_receipt {
        msg.push_str(&format!("Disposition-Notification-To: {from}\r\n"));
    }

    // derive full html with email template wrapper
    let wrapped_html = html_body.map(wrap_email_html);
    let has_html = wrapped_html.is_some();

    let has_inline = !inline_images.is_empty();

    // helper: build the "content" part (alternative or related or plain)
    // when inline images exist, wrap alternative in multipart/related
    let build_content_part = |msg: &mut String| {
        if has_html {
            let html = wrapped_html.as_deref().unwrap_or("");
            if has_inline {
                // multipart/related wrapping alternative + inline images
                let rel_boundary = format!("----=_Rel_{}", rand_core::OsRng.next_u64());
                msg.push_str(&format!(
                    "Content-Type: multipart/related; boundary=\"{rel_boundary}\"\r\n\r\n"
                ));
                msg.push_str(&format!("--{rel_boundary}\r\n"));
                build_alternative_part(msg, body, html);
                msg.push_str(&crate::inline_image::build_inline_parts(
                    inline_images,
                    &rel_boundary,
                ));
                msg.push_str(&format!("--{rel_boundary}--\r\n"));
            } else {
                build_alternative_part(msg, body, html);
            }
        } else {
            msg.push_str("Content-Type: text/plain; charset=utf-8\r\n");
            msg.push_str("Content-Transfer-Encoding: 8bit\r\n\r\n");
            msg.push_str(body);
            msg.push_str("\r\n");
        }
    };

    if attachments.is_empty() {
        build_content_part(&mut msg);
    } else {
        let boundary = format!("----=_Part_{}", rand_core::OsRng.next_u64());
        msg.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
        ));

        msg.push_str(&format!("--{boundary}\r\n"));
        build_content_part(&mut msg);

        // attachment parts
        for att in attachments {
            msg.push_str(&format!("--{boundary}\r\n"));
            let name_param = mailrs_rfc2231::encode_param("name", &att.filename);
            msg.push_str(&format!(
                "Content-Type: {}; {name_param}\r\n",
                att.content_type
            ));
            msg.push_str("Content-Transfer-Encoding: base64\r\n");
            let filename_param = mailrs_rfc2231::encode_param("filename", &att.filename);
            msg.push_str(&format!(
                "Content-Disposition: attachment; {filename_param}\r\n\r\n",
            ));

            let encoded = base64::engine::general_purpose::STANDARD.encode(&att.data);
            for chunk in encoded.as_bytes().chunks(76) {
                msg.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                msg.push_str("\r\n");
            }
        }

        msg.push_str(&format!("--{boundary}--\r\n"));
    }

    msg.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_address_bare() {
        assert_eq!(extract_address("user@example.com"), "user@example.com");
    }

    #[test]
    fn extract_address_display_name() {
        assert_eq!(
            extract_address("Chenyun Dai <chenyund@qti.qualcomm.com>"),
            "chenyund@qti.qualcomm.com"
        );
    }

    #[test]
    fn extract_address_angle_only() {
        assert_eq!(extract_address("<foo@bar.com>"), "foo@bar.com");
    }

    #[test]
    fn extract_address_with_spaces() {
        assert_eq!(extract_address("  alice@test.org  "), "alice@test.org");
    }

    // --- verify_sender tests ---

    fn make_super_perms(domains: &[&str]) -> crate::permission::EffectivePermissions {
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo, ALL_PERMISSIONS};
        let groups = vec![AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "super".into(),
                domain: None,
                description: String::new(),
                is_builtin: true,
                created_at: 0,
            },
            permissions: ALL_PERMISSIONS.iter().map(|s| s.to_string()).collect(),
        }];
        compute_effective_permissions(
            &groups,
            &[],
            &domains.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    }

    fn make_no_perms() -> crate::permission::EffectivePermissions {
        crate::permission::compute_effective_permissions(&[], &[], &[])
    }

    #[test]
    fn verify_sender_superadmin_matching_domain_allowed() {
        let perms = make_super_perms(&["golia.jp", "example.com"]);
        assert!(verify_sender("agent@golia.jp", "admin@golia.jp", &perms).is_ok());
        // different user but same domain
        assert!(verify_sender("other@example.com", "admin@golia.jp", &perms).is_ok());
    }

    #[test]
    fn verify_sender_superadmin_non_matching_domain_rejected() {
        // super user with only golia.jp domain — but super has all domains, so it should allow
        // let's test with a domain-scoped group instead
        use crate::permission::{compute_effective_permissions, AccountGroup, GroupInfo};
        let groups = vec![AccountGroup {
            group: GroupInfo {
                id: 1,
                name: "user".into(),
                domain: Some("golia.jp".into()),
                description: String::new(),
                is_builtin: false,
                created_at: 0,
            },
            permissions: vec!["mail.send".into(), "mail.read".into()],
        }];
        let perms = compute_effective_permissions(&groups, &[], &["golia.jp".into()]);
        assert_eq!(
            verify_sender("agent@evil.com", "admin@golia.jp", &perms),
            Err("sender must match authenticated user")
        );
    }

    #[test]
    fn verify_sender_non_superadmin_different_from_rejected() {
        let perms = make_no_perms();
        assert_eq!(
            verify_sender("other@golia.jp", "user@golia.jp", &perms),
            Err("sender must match authenticated user")
        );
    }

    #[test]
    fn verify_sender_non_superadmin_matching_from_allowed() {
        let perms = make_no_perms();
        assert!(verify_sender("user@golia.jp", "user@golia.jp", &perms).is_ok());
    }

    // --- resolve_thread_reply tests ---

    #[tokio::test]
    async fn resolve_thread_reply_thread_id_resolves_when_no_in_reply_to() {
        // when no mailbox store and no in_reply_to, thread_id cannot resolve (no DB)
        // but it should not panic
        let (reply, refs) = resolve_thread_reply(
            Some("thread-abc"),
            None,
            "user@test.com",
            None,
        ).await;
        // without a store, cannot resolve thread_id
        assert!(reply.is_none());
        assert!(refs.is_empty());
    }

    #[tokio::test]
    async fn resolve_thread_reply_explicit_in_reply_to_takes_precedence() {
        // explicit in_reply_to should be used even if reply_to_thread_id is present
        let (reply, _refs) = resolve_thread_reply(
            Some("thread-abc"),
            Some("explicit-msg-id@test.com"),
            "user@test.com",
            None,
        ).await;
        assert_eq!(reply.as_deref(), Some("explicit-msg-id@test.com"));
    }
}
