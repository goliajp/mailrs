//! One message of a thread, as the thread view renders it.

use std::collections::HashMap;

use mailrs_mailbox::{EmailAnalysisRow, MessageMeta, PgMailboxStore};

use crate::message_util;

use super::super::super::*;

pub(super) async fn thread_message_response(
    msg: &MessageMeta,
    user: &str,
    maildir_root: &str,
    mb_store: &PgMailboxStore,
    invite_methods: &HashMap<i64, String>,
) -> ThreadMessageResponse {
    // in supermode, use the message owner's maildir; otherwise use current user
    let maildir_user = if msg.user_address.is_empty() {
        user
    } else {
        msg.user_address.as_str()
    };
    let raw = message_util::read_message_raw(maildir_root, maildir_user, &msg.maildir_id).await;
    let parsed = raw
        .as_deref()
        .map(message_util::parse_message)
        .unwrap_or_default();

    let (sender, subject) = sender_and_subject(msg, raw.as_deref());

    // Cc header — not indexed in the DB, so parse it from the raw
    // bytes we already loaded. None when the header is absent or
    // empty (most one-to-one threads), so the wire payload skips
    // the field entirely.
    let cc = raw
        .as_deref()
        .map(|d| message_util::extract_header_from_raw(d, "Cc"))
        .map(|s| message_util::decode_header(&s))
        .filter(|s| !s.trim().is_empty());

    // try AI analysis first, fall back to rule-based
    let ai = mb_store.get_email_analysis(msg.id).await.ok().flatten();
    let analysis = MessageAnalysis::new(
        ai.as_ref(),
        &sender,
        &subject,
        parsed.0.as_deref(),
        parsed.1.as_deref(),
    );

    // extract structured data from HTML before moving it
    let structured_data = parsed.1.as_deref().and_then(|html| {
        let sd = mailrs_intelligence::structured::extract_structured_data(html);
        if sd.is_empty() { None } else { Some(sd) }
    });

    ThreadMessageResponse {
        id: msg.id,
        uid: msg.uid,
        sender,
        recipients: msg.recipients.clone(),
        cc,
        subject,
        flags: msg.flags,
        internal_date: msg.internal_date,
        message_id: msg.message_id.clone(),
        text_body: parsed.0,
        html_body: parsed.1,
        attachments: parsed.2,
        category: analysis.category,
        risk_score: analysis.risk_score,
        risk_reason: analysis.risk_reason,
        summary: analysis.summary,
        people: analysis.people,
        dates: analysis.dates,
        amounts: analysis.amounts,
        action_items: analysis.action_items,
        ai_analyzed: analysis.ai_analyzed,
        clean_text: analysis.clean_text,
        new_content: msg.new_content.clone(),
        importance_level: msg.importance_level.clone(),
        importance_score: msg.importance_score,
        is_bulk_sender: msg.is_bulk_sender,
        has_tracking_pixel: msg.has_tracking_pixel,
        requires_action: ai.as_ref().is_some_and(|a| a.requires_action),
        sender_intent: ai
            .as_ref()
            .map_or_else(|| "inform".into(), |a| a.sender_intent.clone()),
        action_deadline: ai.as_ref().and_then(|a| a.action_deadline.clone()),
        structured_data,
        invite_method: invite_methods.get(&msg.id).cloned(),
    }
}

fn sender_and_subject(msg: &MessageMeta, raw: Option<&[u8]>) -> (String, String) {
    // fallback: extract sender/subject from raw email if DB values are empty
    if msg.sender.is_empty() || msg.subject.is_empty() {
        let raw_sender = raw
            .map(|d| message_util::extract_header_from_raw(d, "From"))
            .unwrap_or_default();
        let raw_subject = raw
            .map(|d| message_util::extract_header_from_raw(d, "Subject"))
            .unwrap_or_default();
        (
            if msg.sender.is_empty() {
                message_util::decode_header(&raw_sender)
            } else {
                message_util::decode_header(&msg.sender)
            },
            if msg.subject.is_empty() {
                message_util::decode_header(&raw_subject)
            } else {
                message_util::decode_header(&msg.subject)
            },
        )
    } else {
        (
            message_util::decode_header(&msg.sender),
            message_util::decode_header(&msg.subject),
        )
    }
}

/// The analysis fields of a thread message: the stored AI analysis when
/// there is one, the rule-based category otherwise.
struct MessageAnalysis {
    category: String,
    risk_score: u8,
    risk_reason: String,
    summary: String,
    people: serde_json::Value,
    dates: serde_json::Value,
    amounts: serde_json::Value,
    action_items: serde_json::Value,
    ai_analyzed: bool,
    clean_text: Option<String>,
}

impl MessageAnalysis {
    fn new(
        ai: Option<&EmailAnalysisRow>,
        sender: &str,
        subject: &str,
        text: Option<&str>,
        html: Option<&str>,
    ) -> Self {
        if let Some(a) = ai {
            let ct = if a.clean_text.is_empty() {
                None
            } else {
                Some(a.clean_text.clone())
            };
            Self {
                category: a.category.clone(),
                risk_score: a.risk_score as u8,
                risk_reason: a.risk_reason.clone(),
                summary: a.summary.clone(),
                people: a.people.clone(),
                dates: a.dates.clone(),
                amounts: a.amounts.clone(),
                action_items: a.action_items.clone(),
                ai_analyzed: true,
                clean_text: ct,
            }
        } else {
            let (cat, score) = crate::web::classify_email(sender, subject, text, html);
            Self {
                category: cat,
                risk_score: score,
                risk_reason: String::new(),
                summary: String::new(),
                people: serde_json::json!([]),
                dates: serde_json::json!([]),
                amounts: serde_json::json!([]),
                action_items: serde_json::json!([]),
                ai_analyzed: false,
                clean_text: None,
            }
        }
    }
}
