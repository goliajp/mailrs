//! Maildir → PG reconciliation (read-repair for split-brain deliveries).
//!
//! A delivery writes the maildir file first, then indexes it into PG
//! (`append_message`). If the PG half fails — degraded database, engine
//! incident, crash between the two writes — the message exists on disk
//! but is invisible to IMAP/JMAP/web. This op walks the maildir tree,
//! finds files with no `messages.maildir_id` row, and indexes them
//! through the same `index_message` path the live pipeline uses (same
//! uid allocation, same threading), into INBOX (folder membership lives
//! only in PG, so disk cannot tell us more; inbound mail is INBOX
//! anyway). Idempotent: a second pass finds nothing to repair.

use crate::pg::PgMailboxStore;
use crate::threading;

/// Outcome of one reconciliation pass.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReconcileReport {
    /// maildir files seen (new/ + cur/ across all users).
    pub scanned: u64,
    /// files with no messages row (the gap).
    pub missing: u64,
    /// files actually indexed this pass (0 on dry runs).
    pub repaired: u64,
    /// per-user missing counts, for the operator's report.
    pub missing_by_user: std::collections::BTreeMap<String, u64>,
    /// repair errors (file kept on disk, row still absent).
    pub errors: Vec<String>,
}

fn extract_header_value(data: &[u8], name: &str) -> String {
    // mirror of message_ops::index's header extraction: first matching
    // header line, unfolded, trimmed
    let text = String::from_utf8_lossy(data);
    let mut value: Option<String> = None;
    for line in text.lines() {
        if line.is_empty() {
            break;
        }
        if let Some(v) = value.as_mut() {
            if line.starts_with(' ') || line.starts_with('\t') {
                v.push(' ');
                v.push_str(line.trim());
                continue;
            }
            break;
        }
        if let Some((h, v)) = line.split_once(':')
            && h.eq_ignore_ascii_case(name)
        {
            value = Some(v.trim().to_string());
        }
    }
    value.unwrap_or_default()
}

impl PgMailboxStore {
    /// Walk `maildir_root/<domain>/<local>/{new,cur}` and index every
    /// file that has no `messages` row. `dry_run` reports without
    /// writing. Returns the pass report.
    pub async fn reconcile_maildir(
        &self,
        maildir_root: &str,
        dry_run: bool,
    ) -> Result<ReconcileReport, String> {
        let mut report = ReconcileReport {
            scanned: 0,
            missing: 0,
            repaired: 0,
            missing_by_user: Default::default(),
            errors: Vec::new(),
        };

        let domains = std::fs::read_dir(maildir_root)
            .map_err(|e| format!("read maildir root {maildir_root}: {e}"))?;
        for domain in domains.flatten().filter(|d| d.path().is_dir()) {
            let domain_name = domain.file_name().to_string_lossy().to_string();
            let locals = match std::fs::read_dir(domain.path()) {
                Ok(l) => l,
                Err(e) => {
                    report.errors.push(format!("read {domain_name}: {e}"));
                    continue;
                }
            };
            for local in locals.flatten().filter(|d| d.path().is_dir()) {
                let local_name = local.file_name().to_string_lossy().to_string();
                let user = format!("{local_name}@{domain_name}");
                self.reconcile_one_maildir(&user, &local.path(), dry_run, &mut report)
                    .await;
            }
        }
        Ok(report)
    }

    async fn reconcile_one_maildir(
        &self,
        user: &str,
        dir: &std::path::Path,
        dry_run: bool,
        report: &mut ReconcileReport,
    ) {
        let md = mailrs_maildir::Maildir::open(dir);
        let entries = match (md.scan_new(), md.scan_cur()) {
            (Ok(n), Ok(c)) => n.into_iter().chain(c),
            (Err(e), _) | (_, Err(e)) => {
                report.errors.push(format!("{user}: scan: {e}"));
                return;
            }
        };
        for entry in entries {
            report.scanned += 1;
            let maildir_id = entry.id.to_string();
            let known: Option<(i64,)> =
                match sqlx::query_as("SELECT id FROM messages WHERE maildir_id = $1 LIMIT 1")
                    .bind(&maildir_id)
                    .fetch_optional(self.pool())
                    .await
                {
                    Ok(row) => row,
                    Err(e) => {
                        report
                            .errors
                            .push(format!("{user}/{maildir_id}: lookup: {e}"));
                        continue;
                    }
                };
            if known.is_some() {
                continue;
            }
            report.missing += 1;
            *report.missing_by_user.entry(user.to_string()).or_default() += 1;
            if dry_run {
                continue;
            }
            if let Err(e) = self.index_orphan(user, &maildir_id, &entry).await {
                report.errors.push(format!("{user}/{maildir_id}: {e}"));
            } else {
                report.repaired += 1;
            }
        }
    }

    async fn index_orphan(
        &self,
        user: &str,
        maildir_id: &str,
        entry: &mailrs_maildir::Entry,
    ) -> Result<(), String> {
        let data = std::fs::read(&entry.path).map_err(|e| format!("read: {e}"))?;
        let delivered_at = std::fs::metadata(&entry.path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .ok_or("mtime unavailable")?;

        let sender = extract_header_value(&data, "From");
        let recipients = extract_header_value(&data, "To");
        let subject = extract_header_value(&data, "Subject");
        let in_reply_to = threading::extract_in_reply_to(&data);
        // message_id extraction can fail (e.g. an Outlook fold this parser
        // misses); never let that yield an empty thread_id, which the
        // conversation list filters out — the message would silently
        // vanish from every view (incident 2026-06-13). synthesise a
        // stable id from the maildir_id when the header is missing.
        let raw_message_id = threading::extract_message_id(&data);
        let effective_message_id = if raw_message_id.is_empty() {
            format!("{maildir_id}@mailrs.local")
        } else {
            raw_message_id
        };
        // always resolve a thread: try the in_reply_to parent first
        // (threads a reply even when its own Message-ID was lost), then
        // fall back to the message's own id. resolve_thread_id never
        // returns empty given a non-empty own id.
        let parent = self
            .find_thread_id_by_message_id(user, &in_reply_to)
            .await
            .ok()
            .flatten();
        let resolved =
            threading::resolve_thread_id(&effective_message_id, &in_reply_to, |_| parent.clone());
        // Gmail subject rule — see apply_subject_gate.
        let thread_id = self
            .apply_subject_gate(user, &effective_message_id, &subject, parent, resolved)
            .await;

        // same PG half the live pipeline uses: uid allocation + insert
        self.index_message(
            user,
            "INBOX",
            maildir_id,
            &sender,
            &recipients,
            &subject,
            data.len() as u32,
            delivered_at,
            &effective_message_id,
            &in_reply_to,
            &thread_id,
        )
        .await
        .map_err(|e| format!("index: {e}"))?;
        Ok(())
    }
}
