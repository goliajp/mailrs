//! Notices held during maintenance: written to the store as they are held,
//! read back for the summary when maintenance ends.

use super::{Dispatcher, MessageKind, Sent};
use crate::store::HeldRow;

impl Sent {
    pub(super) fn held_row(&self, at: i64) -> HeldRow {
        HeldRow {
            rule: self.rule.to_string(),
            kind: self.kind.to_string(),
            severity: self.severity.to_string(),
            object: self.object.clone(),
            value: self.value.clone(),
            threshold: self.threshold.clone(),
            since: self.since,
            at,
        }
    }

    /// A held notice read back; `None` for a rule or kind this build does
    /// not have (renamed by an upgrade while maintenance was on).
    fn from_held(h: &HeldRow) -> Option<Sent> {
        let rule = crate::rules::all().into_iter().find(|r| r.name == h.rule)?;
        let kind = crate::engine::machine::NoticeKind::ALL
            .into_iter()
            .map(|k| k.as_str())
            .find(|k| *k == h.kind)?;
        let severity = [
            crate::rules::Severity::Warning,
            crate::rules::Severity::Critical,
        ]
        .into_iter()
        .map(|s| s.as_str())
        .find(|s| *s == h.severity)?;
        Some(Sent {
            rule: rule.name,
            kind,
            severity,
            object_kind: rule.object_kind,
            object: h.object.clone(),
            value: h.value.clone(),
            threshold: h.threshold.clone(),
            since: h.since,
            text: rule.text,
            under: None,
        })
    }
}

impl Dispatcher {
    /// End of the maintenance stretch that started at `started_at`: one
    /// message summing up what was held during it, read from the store.
    pub async fn maintenance_ended(&mut self, started_at: i64, now: i64) {
        let rows = self.store.held_since(started_at).unwrap_or_else(|e| {
            tracing::error!(error = %e, "held notices not read");
            Vec::new()
        });
        let held: Vec<Sent> = rows
            .iter()
            .filter_map(|h| {
                let sent = Sent::from_held(h);
                if sent.is_none() {
                    tracing::warn!(rule = %h.rule, kind = %h.kind, "held notice of a rule this build does not have");
                }
                sent
            })
            .collect();
        let msg = self.message(
            MessageKind::MaintenanceEnded { held: held.len() },
            held,
            now,
        );
        self.route(msg, now).await;
    }
}
