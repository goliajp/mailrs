//! Telling a reader their copy was taken back, and the audit lines.

use kevy_client::Connection;
use mailrs_core_sidestate::families::recall_notices::{self, Recalled};
use mailrs_core_sidestate::families::webhook_events::{self, Event, kinds};

use super::Target;

/// The reader's notice where the conversation was, the `mail.held`
/// webhook with `reason: "reported"`, and the push — each logged on
/// failure: the conversation is held whether or not they land.
pub(super) fn taken(conn: &mut Connection, t: &Target, batch: &str, raw: &[u8]) {
    let at = chrono::Utc::now().timestamp();
    let from = mailrs_inbound::from_header(raw);
    let subject = mailrs_inbound::subject_header(raw);
    let notice = Recalled {
        thread_id: t.tid.clone(),
        subject: subject.clone(),
        rules: vec!["reported".into()],
        observation: batch.to_string(),
        at,
    };
    if let Err(e) = recall_notices::push(conn, &t.user, &notice) {
        tracing::warn!(error = %e, tid = %t.tid, "report: the reader's notice was not written");
    }
    let event = Event {
        kind: kinds::MAIL_HELD,
        user: &t.user,
        sender: Some(&from),
        thread_id: Some(&t.tid),
        data: serde_json::json!({
            "message_id": t.mid,
            "subject": subject,
            "snippet": "",
            "reason": "reported",
            "rules": ["reported"],
            "observation": batch,
        }),
    };
    webhook_events::enqueue_logged(conn, &event, at);
    crate::push::notify_recalled(Some(conn), &t.user, &t.tid, &from, &subject);
}

/// The `quarantine.hold` line every hold writes, with `report` as its source.
pub(super) fn held(conn: &mut Connection, user: &str, tid: &str) {
    let detail = format!("user={user} source=report rules_version= rules=reported");
    audit(conn, "system", "quarantine.hold", tid, &detail);
}

pub(super) fn audit(conn: &mut Connection, actor: &str, action: &str, target: &str, detail: &str) {
    let at = chrono::Utc::now().timestamp();
    if let Err(e) =
        mailrs_core_sidestate::families::audit::append(conn, actor, action, target, detail, at)
    {
        tracing::warn!(error = %e, %action, "report: the audit line was not written");
    }
}
