//! Apply the fraud checks to mail that arrived before they existed.
//!
//! `mailrs-fraud` runs at receive time, so a signal added on Tuesday
//! reaches Wednesday's mail and never touches Monday's. The wave this
//! was written for had been landing for months: fifty-odd messages
//! sitting in inboxes, every one of which the new checks would have
//! caught.
//!
//! # Dry by default
//!
//! The first run reports and changes nothing. That is not politeness —
//! it is the only way to see what a new signal would have done to a
//! real mailbox before it does it, and this repository has a rule about
//! it (`measure-before-you-cut-over`). Pass `dry_run=false` to move
//! them.
//!
//! # Bounded, and it pauses
//!
//! Same shape as `backfill-decode-headers` and for the same reason: an
//! unbounded sweep over this mailbox took the mail service down for
//! half an hour on 2026-08-26. `limit` threads per call, a pause every
//! `PAUSE_EVERY`, and `next_skip` in the answer.
//!
//! # What it reads
//!
//! The newest message of each thread, from that user's own maildir
//! file. The display name has to be decoded before it can be compared —
//! the names arrive base64'd — and the `X-Mailer` is read raw.

use std::collections::HashMap;

use super::prelude::*;

/// Threads between pauses.
const PAUSE_EVERY: u64 = 25;

/// What to do with a thread the checks flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Action {
    /// Move it to Junk. Reversible, and the default.
    #[default]
    Junk,
    /// Unlink its maildir files. **There is no trash and nothing to
    /// restore from** — the same warning `delete-thread-confirm` puts
    /// in front of a person.
    Delete,
    /// Hold it: out of every ordinary list, into the review screen,
    /// nothing deleted. Writes the verdict too, so the screen can say
    /// which layer convicted.
    ///
    /// The transport layer of a re-scan's verdict is read from the
    /// `Authentication-Results` header the receiver wrote at the time
    /// — the receipt, not a re-derivation. Where there is no such
    /// header the layer says so rather than claiming a pass.
    Hold,
}

#[derive(serde::Deserialize)]
pub(crate) struct RescanQuery {
    /// Report without moving anything. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default)]
    skip: u64,
    #[serde(default = "default_limit")]
    limit: u64,
    #[serde(default = "default_pause_ms")]
    pause_ms: u64,
    /// `junk` (default) or `delete`.
    #[serde(default)]
    action: Action,
}

fn yes() -> bool {
    true
}
fn default_limit() -> u64 {
    500
}
fn default_pause_ms() -> u64 {
    50
}

/// `POST /v1/admin/maintenance:fraud-rescan?dry_run=false`
pub(crate) async fn fraud_rescan_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RescanQuery>,
) -> axum::response::Response {
    let policy = policy_from_env();
    let users = match state.mailbox.list_account_addresses() {
        Ok(u) => u,
        Err(e) => {
            tracing::error!(err = %e, "list_account_addresses failed");
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut stopped_early = false;
    let mut found = 0u64;
    let mut already_junk = 0u64;
    let mut moved = 0u64;
    let mut deleted = 0u64;
    let mut held = 0u64;
    let mut verdict_failed = 0u64;
    let mut no_file = 0u64;
    // Which check fired, because "12 found" does not say whether the
    // one that needs an allow-list entry is among them.
    let mut by_reason: HashMap<String, u64> = HashMap::new();
    let mut samples: Vec<serde_json::Value> = Vec::new();

    'walk: for user in &users {
        for tid in state
            .mailbox
            .all_thread_ids_for_user(user)
            .unwrap_or_default()
        {
            seen += 1;
            if seen <= q.skip {
                continue;
            }
            if walked >= q.limit {
                stopped_early = true;
                break 'walk;
            }
            walked += 1;
            if walked.is_multiple_of(PAUSE_EVERY) {
                tokio::time::sleep(std::time::Duration::from_millis(q.pause_ms)).await;
            }

            let Some((message_id, raw)) = newest_raw(&state, user, &tid) else {
                no_file += 1;
                continue;
            };
            let findings = mailrs_fraud::scan(
                &mailrs_inbound::from_header(&raw),
                x_mailer(&raw).as_deref(),
                &policy,
            );
            // The one definition of "this is held", shared with the
            // verdict this sweep is about to store. `findings.any()`
            // here and a score threshold there is what left 43 held
            // conversations carrying a verdict that said otherwise.
            if !mailrs_inbound::holds(findings) {
                continue;
            }
            found += 1;
            for r in mailrs_fraud::reasons(findings) {
                *by_reason.entry(r.to_string()).or_default() += 1;
            }
            if samples.len() < 25 {
                samples.push(serde_json::json!({
                    "user": user,
                    "thread": tid,
                    "from": mailrs_inbound::from_header(&raw),
                    "reasons": mailrs_fraud::reasons(findings),
                    "score": mailrs_fraud::score(findings),
                }));
            }
            if q.dry_run {
                continue;
            }
            match q.action {
                Action::Junk => {
                    // **Read the bucket first.** `set_junk` answers
                    // "did the row exist", not "did anything change" —
                    // so counting its `true` as a move made
                    // `already_junk` a number that could not come out
                    // other than zero, and a second run reported
                    // moving fifty threads that were already in Junk.
                    let was_junk = state
                        .mailbox
                        .get_thread_for_user(user, &tid)
                        .ok()
                        .flatten()
                        .is_some_and(|r| {
                            // `bucket_of`, not a literal: the category
                            // a Junk row carries is `spam`, and the
                            // first version of this compared against
                            // "junk" and was therefore never true.
                            mailrs_mailbox_kevy::keys::bucket_of(&r.category)
                                == mailrs_mailbox_kevy::keys::Bucket::Junk
                        });
                    match state.mailbox.set_junk(user, &tid, true) {
                        Ok(_) if was_junk => already_junk += 1,
                        Ok(_) => moved += 1,
                        Err(e) => {
                            tracing::warn!(err = %e, %user, %tid, "fraud rescan: set_junk failed");
                        }
                    }
                }
                Action::Hold => {
                    let verdict = rescan_verdict(&raw, findings);
                    match serde_json::to_string(&verdict) {
                        Ok(json) => {
                            if let Err(e) = state.mailbox.set_fraud_verdict(&message_id, &json) {
                                tracing::warn!(err = %e, %user, %tid, "storing the verdict failed");
                                verdict_failed += 1;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(err = %e, "the verdict did not serialise");
                            verdict_failed += 1;
                        }
                    }
                    match state.mailbox.set_quarantined(user, &tid, true) {
                        Ok(true) => held += 1,
                        Ok(false) => tracing::warn!(%user, %tid, "held nothing: no membership row"),
                        Err(e) => tracing::warn!(err = %e, %user, %tid, "holding failed"),
                    }
                }
                Action::Delete => match state.mailbox.delete_thread(user, &tid) {
                    Ok((_, blobs)) => {
                        for b in &blobs {
                            crate::routes::message_ops::unlink_maildir_file(user, b);
                        }
                        deleted += 1;
                    }
                    Err(e) => {
                        tracing::warn!(err = %e, %user, %tid, "fraud rescan: delete failed");
                    }
                },
            }
        }
    }

    tracing::info!(
        walked,
        found,
        moved,
        deleted,
        held,
        verdict_failed,
        already_junk,
        no_file,
        dry_run = q.dry_run,
        "fraud-rescan complete"
    );
    Json(serde_json::json!({
        "done": !stopped_early,
        "next_skip": q.skip + walked,
        "dry_run": q.dry_run,
        "threads_walked": walked,
        "found": found,
        "moved_to_junk": moved,
        "deleted": deleted,
        "held": held,
        // Held but unable to explain itself. Reported rather than
        // folded into `held`, because a hold nobody can see the
        // reasons for is the one that turns into "my mail vanished".
        "verdict_failed": verdict_failed,
        "already_junk": already_junk,
        "no_file": no_file,
        "by_reason": by_reason,
        "samples": samples,
    }))
    .into_response()
}

/// The fraud policy this process was configured with, and a warning
/// when half of it is missing.
///
/// A sweep with no org names cannot fire the impersonation rule, and
/// its `found` count comes back looking like an answer. Say so.
fn policy_from_env() -> mailrs_fraud::Policy {
    let policy = mailrs_fraud::Policy {
        org_names: csv_env("MAILRS_ORG_NAMES"),
        our_domains: csv_env("MAILRS_LOCAL_DOMAINS"),
        allowed_domains: csv_env("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
    };
    if policy.org_names.is_empty() {
        tracing::warn!(
            "fraud rescan: MAILRS_ORG_NAMES is empty — the impersonation check cannot fire, \
             so this sweep sees only the mailer fingerprint. Set it to the same value the \
             receiver has."
        );
    }
    policy
}

/// A comma-separated environment variable, or nothing.
///
/// **The receiver is a different container**, and the sentence that
/// used to be here said otherwise — "this is the same process the
/// receiver's policy is configured for". It is not, and on production
/// it never was: `MAILRS_ORG_NAMES` was set on the receiver only, so
/// every sweep this process ran had an empty org-name list and the
/// impersonation rule could not fire. Half the checks, on the only
/// lane that runs the sweep, with nothing to say so — the count came
/// back plausible because the other rule still worked.
///
/// Found on 2026-08-28 by running the sweep against a copy of
/// production and getting a number eight times too large, because the
/// copy had been given a *wider* policy than production has. The
/// compose file now gives this process the receiver's block verbatim.
///
/// Empty is therefore worth noticing: [`policy_from_env`] logs when a
/// sweep is about to run with no org names, because "found nothing"
/// and "could not look" are different answers.
fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// The raw bytes of a thread's newest message, from this user's copy.
fn newest_raw(state: &Arc<FastcoreState>, user: &str, tid: &str) -> Option<(String, Vec<u8>)> {
    let mut newest: Option<(i64, String, String)> = None;
    for mid in state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default()
    {
        let Ok(Some(bytes)) = state.mailbox.user_message_view(user, &mid) else {
            continue;
        };
        let Ok(wire) =
            serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&bytes)
        else {
            continue;
        };
        if wire.blob_ref.is_empty() {
            continue;
        }
        let better = match &newest {
            None => true,
            Some((d, _, _)) => wire.date >= *d,
        };
        if better {
            newest = Some((wire.date, wire.blob_ref, mid.clone()));
        }
    }
    // The row's own id, not one re-derived from the file. Three held
    // conversations on production carry a synthetic
    // `…@mailrs.local` id while the file underneath has a real
    // `Message-ID`, so a verdict keyed off the file was stored where
    // no reader looks: the screen asks with the id the message row
    // carries, and got null for a conversation it was showing as
    // held. Two ids for one message, the verdict under the one nobody
    // reads.
    let (_, blob_ref, message_id) = newest?;
    let raw = read_maildir_file(user, &blob_ref)?;
    Some((message_id, raw))
}

/// The `X-Mailer` header value, unfolded far enough to compare.
fn x_mailer(raw: &[u8]) -> Option<String> {
    header_value(raw, b"x-mailer:")
}

/// One header's value, by its lowercase name including the colon,
/// with its continuation lines joined.
///
/// Stops at the blank line, so a quoted header in the body is not a
/// header.
///
/// **Folded headers are joined**, and the comment here used to say
/// they were not — "the three that matter put theirs on the first
/// line". Production disproved it within a minute of the first sweep:
/// Exchange writes
///
/// ```text
/// Message-ID:
///  <SA5PR03MB8426…@…outlook.com>
/// ```
///
/// with nothing after the colon, so the id read as empty and the
/// held conversation could not explain itself. `X-Mailer` reads
/// through the same function, so a folded one would have been missed
/// by the scan itself — a fraud check that silently does not fire.
///
/// RFC 5322 §2.2.3: a line beginning with space or tab continues the
/// previous field. Joined with a single space, which is what
/// unfolding means for a structured value.
fn header_value(raw: &[u8], name_lower: &[u8]) -> Option<String> {
    let head = &raw[..raw.len().min(16 * 1024)];
    let text = String::from_utf8_lossy(head);
    let name = String::from_utf8_lossy(name_lower);
    let mut value: Option<String> = None;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if let Some(v) = &mut value {
            // Still inside the field while the line is folded.
            match line.starts_with([' ', '\t']) {
                true => {
                    if !v.is_empty() {
                        v.push(' ');
                    }
                    v.push_str(line.trim());
                    continue;
                }
                false => break,
            }
        }
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix(name.as_ref()) {
            value = Some(line[line.len() - rest.len()..].trim().to_string());
        }
    }
    value.filter(|v| !v.is_empty())
}

/// A verdict for mail that arrived before the checks existed.
///
/// Honest about what it does and does not know. The identity and
/// provenance layers are re-derived from the message, which is where
/// they came from in the first place. The transport layer is read from
/// the `Authentication-Results` header the receiver wrote at the time —
/// that is the receipt, and reading it is not the same as re-running
/// the check. Where the header is missing the layer reports
/// not-applicable, which is what it is.
fn rescan_verdict(raw: &[u8], findings: mailrs_fraud::Findings) -> mailrs_inbound::FraudVerdict {
    let mut input = mailrs_inbound::unexamined();
    input.fraud = findings;
    if let Some(value) = header_value(raw, b"authentication-results:") {
        for r in mailrs_inbound::auth_header::parse_auth_results(&value) {
            match r.method.as_str() {
                "spf" => input.auth.spf = r.result,
                "dkim" => input.auth.dkim = r.result,
                "dmarc" => input.auth.dmarc = r.result,
                "arc" => input.auth.arc = r.result,
                _ => {}
            }
        }
    }
    mailrs_inbound::assess(&input)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELD: &[u8] = b"Authentication-Results: mx.golia.jp; spf=pass; dkim=pass; dmarc=pass\r\n\
From: =?UTF-8?B?R09MSUEgSy5LLg==?= <billing@example.invalid>\r\n\
Message-ID: <m1@example.invalid>\r\n\
X-Mailer: 4.28.1.9\r\n\
Subject: Invoice\r\n\
\r\n\
body\r\n";

    fn findings() -> mailrs_fraud::Findings {
        mailrs_fraud::Findings {
            claims_our_name: true,
            generated_mailer: true,
        }
    }

    /// The receipt, read back. The campaign passed every check, so a
    /// re-scan that reported transport as failing — or as unchecked —
    /// would be telling a different story than the one the receiver
    /// recorded.
    #[test]
    fn a_rescan_reads_the_transport_result_the_receiver_wrote() {
        let v = rescan_verdict(HELD, findings());
        let t = v.layers.iter().find(|l| l.name == "transport").unwrap();
        assert_eq!(t.outcome, mailrs_inbound::Outcome::Pass);
        assert!(t.detail.contains("SPF pass"), "detail: {}", t.detail);
        assert!(
            v.quarantined,
            "score {} did not reach {}",
            v.score, v.threshold
        );
    }

    /// And where there is no receipt it says so. A default that read
    /// as verified would put a tick beside a check that never ran.
    #[test]
    fn without_that_header_transport_is_not_a_pass() {
        let raw = b"From: x <a@b.invalid>\r\nMessage-ID: <m2@b.invalid>\r\n\r\nbody\r\n";
        let v = rescan_verdict(raw, findings());
        let t = v.layers.iter().find(|l| l.name == "transport").unwrap();
        assert_eq!(t.outcome, mailrs_inbound::Outcome::NotApplicable);
    }

    /// The shape production had on the first sweep: Exchange puts
    /// nothing after `Message-ID:` and folds the value onto the next
    /// line. Read without joining, the id is empty, the verdict is
    /// never stored, and a held conversation cannot say why.
    #[test]
    fn a_folded_header_is_joined() {
        let raw = b"Received: from mail.golia.ai\r\n\tby mail.golia.ai\r\n\
Message-ID:\r\n <SA5PR03MB8426@namprd03.prod.outlook.com>\r\n\
Subject: hi\r\n\r\nbody\r\n";
        assert_eq!(
            header_value(raw, b"message-id:").as_deref(),
            Some("<SA5PR03MB8426@namprd03.prod.outlook.com>")
        );
    }

    /// And the same for the header the scan itself convicts on — a
    /// folded `X-Mailer` read as absent is a check that silently does
    /// not fire.
    #[test]
    fn a_folded_x_mailer_is_still_seen() {
        let raw = b"From: a <a@b.invalid>\r\nX-Mailer:\r\n 4.28.1.9\r\n\r\nbody\r\n";
        assert_eq!(x_mailer(raw).as_deref(), Some("4.28.1.9"));
    }

    /// Joining stops at the next field, or every header would be one
    /// long string.
    #[test]
    fn joining_stops_at_the_next_header() {
        let raw = b"Subject: one\r\n two\r\nX-Mailer: 9.9.9.9\r\n\r\nbody\r\n";
        assert_eq!(header_value(raw, b"subject:").as_deref(), Some("one two"));
        assert_eq!(x_mailer(raw).as_deref(), Some("9.9.9.9"));
    }

    /// A header quoted in the body is not a header.
    #[test]
    fn the_scan_stops_at_the_blank_line() {
        let raw = b"From: x <a@b.invalid>\r\n\r\nX-Mailer: 9.9.9.9\r\n";
        assert_eq!(x_mailer(raw), None);
    }

    /// An empty org-name list is a policy that cannot convict on the
    /// name claim, and the sweep's `found` count would come back
    /// looking like an answer. This is the assertion that the two are
    /// distinguishable at all: same message, two policies, two
    /// results.
    ///
    /// It is the defect that shipped — `MAILRS_ORG_NAMES` was set on
    /// the receiver and not on this process, so every sweep run here
    /// saw only the mailer fingerprint.
    #[test]
    fn without_org_names_the_impersonation_check_cannot_fire() {
        let from = "=?UTF-8?B?R09MSUEgSy5LLg==?= <billing@example.invalid>";
        let decoded = mailrs_inbound::from_header(format!("From: {from}\r\n\r\n").as_bytes());

        let configured = mailrs_fraud::Policy {
            org_names: vec!["GOLIA K.K.".into()],
            our_domains: vec!["golia.jp".into()],
            allowed_domains: Vec::new(),
        };
        let empty = mailrs_fraud::Policy {
            org_names: Vec::new(),
            our_domains: vec!["golia.jp".into()],
            allowed_domains: Vec::new(),
        };

        assert!(
            mailrs_fraud::scan(&decoded, None, &configured).claims_our_name,
            "the configured policy did not catch a message claiming to be us"
        );
        assert!(
            !mailrs_fraud::scan(&decoded, None, &empty).claims_our_name,
            "an empty policy convicted, so this test cannot tell the two apart"
        );
    }

    /// `hold` is a value the route accepts. Without this the action
    /// exists in Rust and not on the wire, which is the shape where a
    /// caller passes `action=hold`, serde rejects it, and the sweep
    /// quietly does the default thing instead.
    #[test]
    fn hold_is_reachable_from_the_query_string() {
        let a: Action = serde_json::from_str("\"hold\"").expect("hold parses");
        assert_eq!(a, Action::Hold);
        assert_eq!(
            serde_json::from_str::<Action>("\"junk\"").unwrap(),
            Action::Junk,
            "the default must still parse"
        );
    }
}
