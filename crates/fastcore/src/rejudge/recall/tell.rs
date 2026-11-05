//! What a round that took conversations back writes beside the hold: per
//! conversation the audit line and the `mail.held` webhook, per reader one
//! notice and one push for the whole round. Each is logged on failure: the
//! conversations are held whether or not they land.

use kevy_client::Connection;
use mailrs_core_sidestate::families::recall_notices::{self, Recalled};
use mailrs_core_sidestate::families::webhook_events;

use super::Candidate;

/// The audit line and the webhook for one conversation taken back.
pub(super) fn conversation(conn: &mut Connection, c: &Candidate, observation: &str, at: i64) {
    let detail = format!(
        "user={} observation={observation} rules={}",
        c.user,
        c.rules.join(",")
    );
    if let Err(e) = mailrs_core_sidestate::families::audit::append(
        conn,
        "rejudge",
        "rejudge.recall",
        &c.tid,
        &detail,
        at,
    ) {
        tracing::warn!(error = %e, tid = %c.tid, "rejudge: the recall was not audited");
    }
    let event = webhook_events::Event {
        kind: webhook_events::kinds::MAIL_HELD,
        user: &c.user,
        sender: Some(&c.from),
        thread_id: Some(&c.tid),
        data: serde_json::json!({
            "message_id": c.mid,
            "subject": c.subject,
            "snippet": "",
            "reason": "rejudged",
            "rules": c.rules,
            "observation": observation,
        }),
    };
    webhook_events::enqueue_logged(conn, &event, at);
}

/// One notice and one push telling `user` that `taken` (theirs, at least
/// one) were moved to Review by `observation`'s round.
pub(super) fn reader(
    conn: &mut Connection,
    user: &str,
    observation: &str,
    taken: &[&Candidate],
    at: i64,
) {
    let mut rules: Vec<String> = taken.iter().flat_map(|c| c.rules.clone()).collect();
    rules.sort();
    rules.dedup();
    let notice = Recalled {
        thread_ids: taken.iter().map(|c| c.tid.clone()).collect(),
        rules,
        observation: observation.to_string(),
        at,
    };
    if let Err(e) = recall_notices::push(conn, user, &notice) {
        tracing::warn!(error = %e, %user, %observation, "rejudge: the reader's notice was not written");
    }
    let pushed: Vec<mailrs_intake::push::RecalledConversation<'_>> = taken
        .iter()
        .map(|c| mailrs_intake::push::RecalledConversation {
            thread_id: &c.tid,
            sender: &c.from,
            subject: &c.subject,
        })
        .collect();
    mailrs_intake::push::notify_recalled(Some(conn), user, &pushed);
}
