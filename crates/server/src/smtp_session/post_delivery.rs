use hickory_resolver::TokioResolver;

use super::headers::extract_display_name;

/// async post-delivery processing: contact upsert, content extraction, importance scoring, BIMI
#[allow(clippy::too_many_arguments)]
pub(super) async fn post_delivery_process(
    mb_store: &mailrs_mailbox::PgMailboxStore,
    user: &str,
    sender: &str,
    maildir_id: &str,
    _maildir_root: &str,
    raw_headers: &str,
    full_message: &[u8],
    resolver: Option<&TokioResolver>,
) {
    use mailrs_clean as html_clean;
    use mailrs_intelligence::importance::{self, ImportanceSignals};

    // 1. contact upsert
    let display_name = extract_display_name(sender);
    let is_bulk = html_clean::detect_bulk_sender(raw_headers);
    let is_auto = html_clean::is_automated_sender(sender);

    if let Err(e) = mb_store
        .upsert_contact_inbound(user, sender, &display_name, is_bulk, is_auto)
        .await
    {
        tracing::warn!("contact upsert failed for {sender}: {e}");
    }

    // 2. parse and extract content
    let (text_body, html_body, _attachments) = crate::message_util::parse_message(full_message);

    // 3. deep html cleaning
    let (clean_text, has_tracking, is_template_heavy, link_count) =
        if let Some(ref html) = html_body {
            let result = html_clean::clean_email_html(html);
            (
                Some(result.clean_text),
                result.has_tracking_pixel,
                result.is_template_heavy,
                result.link_count,
            )
        } else {
            // plain text email: clean_text = text_body
            (text_body.clone(), false, false, 0)
        };

    // 4. split quoted content
    let new_content = clean_text.as_deref().map(|t| {
        let (new, _) = html_clean::split_quoted_content(t);
        new
    });

    // 5. importance scoring
    let contact_info = mb_store
        .get_contact_for_scoring(user, sender)
        .await
        .ok()
        .flatten();
    let is_reply = mb_store.has_sent_to(user, sender).await.unwrap_or(false);

    let signals = ImportanceSignals {
        is_mutual_contact: contact_info.as_ref().is_some_and(|c| c.is_mutual),
        is_direct_recipient: true, // inbound to this user = direct
        is_reply_to_my_email: is_reply,
        has_action_items: false, // will be updated by AI analysis later
        is_vip_sender: contact_info.as_ref().is_some_and(|c| c.is_vip),
        is_bulk_sender: is_bulk,
        is_mailing_list: contact_info.as_ref().map_or(is_bulk, |c| c.is_mailing_list),
        is_automated: is_auto,
        has_tracking_pixel: has_tracking,
        is_template_heavy,
        text_to_html_ratio: 1.0,
        link_count,
        contact_importance_bias: contact_info.as_ref().map_or(0.0, |c| c.importance_bias),
    };

    let (level, score) = importance::calculate_importance(&signals);

    // 6. persist to database
    if let Ok(Some(msg_id)) = mb_store.get_message_id_by_maildir(user, maildir_id).await {
        if let Err(e) = mb_store
            .update_message_content(
                msg_id,
                text_body.as_deref(),
                html_body.as_deref(),
                clean_text.as_deref(),
                new_content.as_deref(),
                is_bulk,
                has_tracking,
                level.as_str(),
                score,
            )
            .await
        {
            tracing::warn!("update_message_content failed for msg {msg_id}: {e}");
        }

        // 7. BIMI logo lookup
        if let Some(resolver) = resolver {
            let sender_domain = sender
                .rsplit_once('@')
                .or_else(|| {
                    // handle "Name <user@domain>" format
                    sender
                        .rsplit_once('<')
                        .and_then(|(_, rest)| rest.trim_end_matches('>').rsplit_once('@'))
                })
                .map(|(_, d)| d.trim_end_matches('>'));
            if let Some(domain) = sender_domain
                && let Some(logo_url) = mailrs_postmaster::lookup_bimi_logo(resolver, domain).await
                && let Err(e) = mb_store.update_bimi_logo(msg_id, &logo_url).await
            {
                tracing::warn!("BIMI update failed for msg {msg_id}: {e}");
            }
        }
    }
}
