//! Around the walk's decision: where a conversation is stored, which
//! pack judges it, and carrying out the action on a held one.

use super::*;

impl Sweep<'_> {
    /// The conversation as stored: held, and whether its bucket is Junk.
    pub(super) fn position(&self, user: &str, tid: &str) -> Position {
        let row = self
            .state
            .mailbox
            .get_thread_for_user(user, tid)
            .ok()
            .flatten();
        Position {
            held: row.as_ref().is_some_and(|t| t.quarantined),
            junk: row.is_some_and(|t| {
                mailrs_mailbox_kevy::keys::bucket_of(&t.category)
                    == mailrs_mailbox_kevy::keys::Bucket::Junk
            }),
        }
    }

    /// The candidate's verdict when there is one, else the active pack's.
    pub(super) fn scan(
        &mut self,
        facts: &mailrs_fraud::Facts<'_>,
    ) -> Result<mailrs_fraud_lua::Scan, String> {
        let Some(rules) = self.candidate.as_mut() else {
            return mailrs_fraud_lua::scan(facts, self.policy);
        };
        let e = rules.classify(facts, self.policy);
        if !e.errors.is_empty() {
            return Err(format!("{:?}", e.errors));
        }
        Ok(mailrs_fraud_lua::Scan {
            findings: e.findings,
            version: rules.version().to_string(),
            shadow: e.shadow.iter().map(|f| f.rule.clone()).collect(),
        })
    }

    /// Carry out the requested action on a held thread.
    pub(super) fn act(
        &mut self,
        user: &str,
        tid: &str,
        message_id: &str,
        raw: &[u8],
        findings: &mailrs_fraud::Findings,
        rules_version: &str,
    ) {
        let state = self.state;
        let t = &mut self.t;
        match self.q.action {
            Action::Junk => match move_to_junk(state, user, tid) {
                Some(true) => t.already_junk += 1,
                Some(false) => t.moved += 1,
                None => {}
            },
            Action::Hold => {
                let mut verdict = rescan_verdict(raw, findings);
                verdict.rules_version = rules_version.to_string();
                match holding::apply(
                    state,
                    user,
                    tid,
                    message_id,
                    &verdict,
                    crate::hold_facts::REJUDGE,
                ) {
                    Ok(changed) => t.held += u64::from(changed),
                    Err(error) => {
                        t.verdict_failed += 1;
                        tracing::error!(%error, %user, %tid, "fraud hold incomplete");
                    }
                }
            }
            Action::Delete => match state.mailbox.delete_thread(user, tid) {
                Ok((_, blobs)) => {
                    for b in &blobs {
                        crate::routes::message_ops::unlink_maildir_file(state, user, b);
                    }
                    t.deleted += 1;
                }
                Err(e) => {
                    tracing::warn!(err = %e, %user, %tid, "fraud rescan: delete failed");
                }
            },
        }
    }
}

/// Where the configured action puts a conversation the findings hold;
/// `None` when it deletes it.
pub(super) fn after_found(action: Action, before: Position) -> Option<Position> {
    match action {
        Action::Hold => Some(Position {
            held: true,
            ..before
        }),
        Action::Junk => Some(Position {
            junk: true,
            ..before
        }),
        Action::Delete => None,
    }
}
