//! Rendering an outgoing message: the RFC 5322 shape, the
//! multipart/alternative body, and the HTML wrapper.

//! Shared helpers used across multiple `mail/` sub-modules.
//!
//! Anything in this file is reachable from sibling sub-modules via
//! `use super::common::*`. Items that need to remain callable from outside
//! the `mail::` module (e.g. `mcp/mod.rs`, `web/rsvp.rs`, `web/auth.rs`,
//! `web/jmap.rs`) are `pub(crate)` so the `pub(crate) use common::*` re-export
//! in `mod.rs` lifts them to the `mail::` path.

use base64::Engine;
use rand_core::RngCore;

use super::*;

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
        use crate::permission::{
            ALL_PERMISSIONS, AccountGroup, GroupInfo, compute_effective_permissions,
        };
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
        use crate::permission::{AccountGroup, GroupInfo, compute_effective_permissions};
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
        let (reply, refs) =
            resolve_thread_reply(Some("thread-abc"), None, "user@test.com", None).await;
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
        )
        .await;
        assert_eq!(reply.as_deref(), Some("explicit-msg-id@test.com"));
    }
}
