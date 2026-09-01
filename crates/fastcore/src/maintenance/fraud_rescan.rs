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

pub(super) mod reading;
use reading::*;

/// Threads between pauses.
const PAUSE_EVERY: u64 = 25;

pub(crate) use decision::*;

mod decision;

/// `POST /v1/admin/maintenance:fraud-rescan?dry_run=false`
pub(crate) async fn fraud_rescan_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RescanQuery>,
) -> axum::response::Response {
    let policy = policy_from_env(&state);
    // One connection for the whole sweep. The brand check needs to
    // know how familiar each sender's domain is, and connecting per
    // thread would be 34,000 connections.
    let mut hist = crate::live_sync::network_kevy_url()
        .and_then(|u| kevy_client::Connection::connect(&u).ok());
    if hist.is_none() {
        tracing::warn!(
            "fraud rescan: no network kevy — the brand check cannot tell a \
             familiar domain from a fresh one, so it will not fire"
        );
    }
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
    // Held by a rule that no longer holds. Counted separately from
    // `released` so a dry run says how many *would* be let go — a
    // single number could not be told apart from "none were".
    let mut held_but_unreadable = 0u64;
    let mut releasable = 0u64;
    let mut released = 0u64;
    let mut release_samples: Vec<serde_json::Value> = Vec::new();
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
                // A conversation with no file cannot be re-judged, so
                // it can never be released either — it stays hidden
                // whatever the rules say afterwards. Counted apart
                // from `no_file`, which otherwise folds that together
                // with the ordinary case of a thread that was not
                // held in the first place.
                if state
                    .mailbox
                    .get_thread_for_user(user, &tid)
                    .ok()
                    .flatten()
                    .is_some_and(|t| t.quarantined)
                {
                    held_but_unreadable += 1;
                }
                continue;
            };
            let from = mailrs_inbound::from_header(&raw);
            // The count as it stands, **not** minus this message.
            //
            // It was minus one, reasoning that the sweep re-plays the
            // moment the mail arrived and its own arrival should not
            // make its sender look familiar. That is the wrong job:
            // a sweep is not a re-enactment, it is a fresh judgement
            // with everything known today. The subtraction only made
            // every domain one message less familiar than it is, and
            // on 2026-08-30 that pushed a legitimate sender off the
            // edge — `three` messages from `rooms-online.jp`, the
            // 三井住友銀行 appointment confirmations, read as `two`
            // and were held.
            //
            // A threshold with a subtraction under it is a different
            // threshold, and not the one the corpus was measured
            // against.
            let seen = domain_seen(hist.as_mut(), &from);
            // The same facts the receive path assembles, from the
            // same extractors. Two assemblies of one message is how
            // a folded header came to be visible on one path and not
            // the other.
            let host = from
                .rfind('@')
                .map(|at| from[at + 1..].trim_end_matches('>').trim().to_string())
                .unwrap_or_default();
            let registrable = mailrs_fraud::brand::registrable(&host);
            let x = mailrs_inbound::x_mailer_header(&raw);
            // The same reading the receive path takes, from the same
            // function. Without it the sweep cannot see a display
            // name that renders as something other than it says —
            // and that is how the one the user asked about survived
            // a full re-scan.
            let deception = mailrs_inbound::deception_in_identity(&raw);
            let name_deception = mailrs_inbound::deception_in_display_name(&raw);
            let parsed = mailrs_mime::parse(&raw);
            let attachments: Vec<&str> = parsed
                .attachments()
                .filter_map(|p| p.attachment_filename())
                .collect();
            let subject = mailrs_inbound::subject_header(&raw);
            let to_display = mailrs_inbound::identity::to_display_name(&raw);
            let reply_to = mailrs_inbound::identity::reply_to_address(&raw);
            // Read, not recorded. The sweep walks the same message
            // more than once over its life, and a set does not care
            // — but the live path is the one that owns the writing,
            // and a sweep that also wrote would make "how many
            // domains" depend on how often the sweep had run.
            let reply_rotation = reading::reply_rotation(&mut hist, &host, &reply_to);
            let facts = mailrs_fraud::Facts {
                from: &from,
                subject: &subject,
                domain: &host,
                registrable: &registrable,
                domain_seen: seen,
                x_mailer: x.as_deref(),
                has_zero_width: deception.unjustified_zero_width,
                has_bidi_override: deception.bidi_override,
                has_zero_width_in_name: name_deception.unjustified_zero_width,
                has_executable_attachment: mailrs_fraud::attachment::any_executable(
                    attachments.iter().copied(),
                ),
                has_zero_width_inside_a_word: deception.zero_width_inside_a_word,
                to_display: &to_display,
                reply_rotation,
                ..mailrs_fraud::Facts::default()
            };
            let findings = mailrs_fraud::scan(&facts, &policy);
            // The one definition of "this is held", shared with the
            // verdict this sweep is about to store. `findings.any()`
            // here and a score threshold there is what left 43 held
            // conversations carrying a verdict that said otherwise.
            if !mailrs_inbound::holds(&findings) {
                // **And release it if it is still held.** The sweep
                // that only ever adds is a sweep a rule change
                // cannot reach: when the two brand rules were
                // demoted to suspicion on 2026-08-30 — because
                // whether this deployment finds a sender familiar
                // may not be grounds for hiding their mail — thirty
                // conversations went on being hidden by a rule that
                // no longer holds anything, and nothing in the
                // system could have noticed.
                //
                // Safe to do automatically because **no hold is a
                // person's judgement**. Two places set it: this
                // sweep, and `ingest.rs`, which acts on the verdict
                // the receive path stored. Both are the rules
                // speaking, so the rules may take it back.
                //
                // The version of this comment that shipped on
                // 2026-08-31 said the sweep was the only one. It was
                // written from a grep that missed `ingest.rs`, and it
                // is the reason held mail kept arriving unread: that
                // path holds and — until the same day — did not mark
                // read. A release path resting on "there is only one
                // writer" has to name them.
                let held_now = state
                    .mailbox
                    .get_thread_for_user(user, &tid)
                    .ok()
                    .flatten()
                    .is_some_and(|t| t.quarantined);
                if disposition(false, held_now) != Disposition::Release {
                    continue;
                }
                releasable += 1;
                if release_samples.len() < 25 {
                    release_samples.push(serde_json::json!({
                        "user": user,
                        "thread": tid,
                        "from": from,
                        "still_scored": findings.score(),
                    }));
                }
                if q.dry_run {
                    continue;
                }
                match state.mailbox.set_quarantined(user, &tid, false) {
                    Ok(_) => released += 1,
                    Err(e) => tracing::warn!(err = %e, tid, "release failed"),
                }
                continue;
            }
            found += 1;
            for r in findings.rules() {
                *by_reason.entry(r.to_string()).or_default() += 1;
            }
            if samples.len() < 25 {
                samples.push(serde_json::json!({
                    "user": user,
                    "thread": tid,
                    "from": from,
                    "reasons": findings.rules(),
                    "score": findings.score(),
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
                    let verdict = rescan_verdict(&raw, &findings);
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
                    // **And mark it read.** A held conversation that
                    // is also unread is bold in the review list, adds
                    // to the badge, and rings the phone — which is
                    // the attempt to defraud getting the attention it
                    // was sent to get. Holding is meant to take the
                    // attention away, and leaving the unread flag on
                    // gives back most of what holding removed.
                    //
                    // Read is a claim about the reader, and this
                    // makes it on their behalf. That is the trade:
                    // the alternative is a review screen whose whole
                    // purpose is "you do not need to look at these
                    // now" wearing a count that says the opposite.
                    // Releasing does not undo it, and should not — by
                    // then somebody *has* looked.
                    // Through the same function the read verbs use,
                    // not `mark_seen` alone. `mark_seen` writes the
                    // axis column and a shared blob **no read path has
                    // consulted since stage 5 of the per-user message
                    // projection** — so the thread left every unread
                    // list while `unread_count`, which is what the
                    // review screen renders, stayed at one. Held and
                    // bold, which is the thing this exists to stop.
                    //
                    // Found by checking after the sweep rather than by
                    // any test: three held conversations came back
                    // unread, and the docstring on the function beside
                    // `mark_seen` had said why all along.
                    crate::routes::thread_actions::mark_thread_read_everywhere(&state, user, &tid);
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
        releasable,
        released,
        held_but_unreadable,
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
        // Held by a rule that no longer holds anything. Two numbers
        // rather than one: a dry run can only report `releasable`,
        // and a single figure of 0 would not say whether it found
        // none or was not allowed to act.
        "releasable": releasable,
        "released": released,
        // Hidden, and its file is gone: it cannot be re-judged, so
        // no rule change will ever let it out. Zero is the answer
        // that means what it says.
        "held_but_unreadable": held_but_unreadable,
        "release_samples": release_samples,
        "already_junk": already_junk,
        "no_file": no_file,
        "by_reason": by_reason,
        "samples": samples,
    }))
    .into_response()
}

/// The display name on every account row.
///
/// Read here rather than configured, because the store is the
/// authority on who has an account and a variable is a second copy
/// that can drift from it.
fn account_display_names(state: &Arc<FastcoreState>) -> Vec<String> {
    let Ok(addrs) = state.mailbox.list_account_addresses() else {
        return Vec::new();
    };
    addrs
        .iter()
        .filter_map(|a| state.mailbox.get_account_blob(a).ok().flatten())
        .filter_map(|blob| serde_json::from_str::<serde_json::Value>(&blob).ok())
        .filter_map(|v| {
            v.get("display_name")
                .and_then(|d| d.as_str())
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string)
        })
        .collect()
}

/// The fraud policy this process was configured with, and a warning
/// when half of it is missing.
///
/// A sweep with no org names cannot fire the impersonation rule, and
/// its `found` count comes back looking like an answer. Say so.
fn policy_from_env(state: &Arc<FastcoreState>) -> mailrs_fraud::Policy {
    let policy = mailrs_fraud::Policy {
        org_names: csv_env("MAILRS_ORG_NAMES"),
        our_domains: csv_env("MAILRS_LOCAL_DOMAINS"),
        allowed_domains: csv_env("MAILRS_ORG_NAME_ALLOWED_DOMAINS"),
        // **From the account rows, not from the environment.** A
        // deployment knows who holds an account on it, and asking an
        // operator to keep a second copy in a variable is asking for
        // the thing that already happened once: `MAILRS_ORG_NAMES`
        // was set on the receiver and not on this process, so half
        // the impersonation check was silently off for a day
        // (`rules/a-policy-the-process-cannot-read.md`).
        account_names: account_display_names(state),
    };
    if policy.account_names.is_empty() {
        tracing::warn!(
            "fraud rescan: no account display names — the check for somebody wearing one of \
             our own people's names cannot fire."
        );
    }
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

    /// A conversation the rules no longer hold, that is still
    /// hidden, is let out. Nothing else about the sweep can do this
    /// — every other path either holds or leaves alone.
    #[test]
    fn a_hold_a_rule_no_longer_supports_is_released() {
        assert_eq!(disposition(false, true), Disposition::Release);
    }

    /// And the three that must not move.
    #[test]
    fn nothing_else_is_released() {
        assert_eq!(disposition(true, true), Disposition::Act);
        assert_eq!(disposition(true, false), Disposition::Act);
        assert_eq!(
            disposition(false, false),
            Disposition::Leave,
            "mail that was never held is not touched by the release path"
        );
    }

    /// Just the `From:`, for a case that is only about the name.
    fn facts(from: &str) -> mailrs_fraud::Facts<'_> {
        mailrs_fraud::Facts {
            from,
            ..mailrs_fraud::Facts::default()
        }
    }

    fn findings() -> mailrs_fraud::Findings {
        let mut f = mailrs_fraud::Findings::new();
        f.push(mailrs_fraud::Finding::new(
            mailrs_fraud::RULE_CLAIMS_OUR_NAME,
            mailrs_fraud::Layer::Identity,
            mailrs_fraud::CLAIMS_OUR_NAME_SCORE,
            "display name claims this organisation",
        ));
        f.push(mailrs_fraud::Finding::new(
            mailrs_fraud::RULE_GENERATED_MAILER,
            mailrs_fraud::Layer::Provenance,
            mailrs_fraud::GENERATED_MAILER_SCORE,
            "X-Mailer is one no mail client writes",
        ));
        f
    }

    /// The receipt, read back. The campaign passed every check, so a
    /// re-scan that reported transport as failing — or as unchecked —
    /// would be telling a different story than the one the receiver
    /// recorded.
    #[test]
    fn a_rescan_reads_the_transport_result_the_receiver_wrote() {
        let v = rescan_verdict(HELD, &findings());
        let t = v.layers.iter().find(|l| l.name == "transport").unwrap();
        assert_eq!(t.outcome, mailrs_inbound::Outcome::Pass);
        assert!(t.detail.contains("SPF pass"), "detail: {}", t.detail);
        assert!(
            v.quarantined,
            "the campaign was not held (score {})",
            v.score
        );
    }

    /// And where there is no receipt it says so. A default that read
    /// as verified would put a tick beside a check that never ran.
    #[test]
    fn without_that_header_transport_is_not_a_pass() {
        let raw = b"From: x <a@b.invalid>\r\nMessage-ID: <m2@b.invalid>\r\n\r\nbody\r\n";
        let v = rescan_verdict(raw, &findings());
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
    ///
    /// Read through `mailrs_inbound`, which is now the only reader:
    /// this file had its own, and two readers of one header is how
    /// the folded case came to work on one path and not the other.
    #[test]
    fn a_folded_x_mailer_is_still_seen() {
        let raw = b"From: a <a@b.invalid>\r\nX-Mailer:\r\n 4.28.1.9\r\n\r\nbody\r\n";
        assert_eq!(
            mailrs_inbound::x_mailer_header(raw).as_deref(),
            Some("4.28.1.9")
        );
    }

    /// Joining stops at the next field, or every header would be one
    /// long string.
    #[test]
    fn joining_stops_at_the_next_header() {
        let raw = b"Subject: one\r\n two\r\nX-Mailer: 9.9.9.9\r\n\r\nbody\r\n";
        assert_eq!(header_value(raw, b"subject:").as_deref(), Some("one two"));
        assert_eq!(
            mailrs_inbound::x_mailer_header(raw).as_deref(),
            Some("9.9.9.9")
        );
    }

    /// A header quoted in the body is not a header.
    #[test]
    fn the_scan_stops_at_the_blank_line() {
        let raw = b"From: x <a@b.invalid>\r\n\r\nX-Mailer: 9.9.9.9\r\n";
        assert_eq!(mailrs_inbound::x_mailer_header(raw), None);
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
            account_names: Vec::new(),
            org_names: vec!["GOLIA K.K.".into()],
            our_domains: vec!["golia.jp".into()],
            allowed_domains: Vec::new(),
        };
        let empty = mailrs_fraud::Policy {
            account_names: Vec::new(),
            org_names: Vec::new(),
            our_domains: vec!["golia.jp".into()],
            allowed_domains: Vec::new(),
        };

        assert!(
            mailrs_fraud::scan(&facts(&decoded), &configured)
                .has(mailrs_fraud::RULE_CLAIMS_OUR_NAME),
            "the configured policy did not catch a message claiming to be us"
        );
        assert!(
            !mailrs_fraud::scan(&facts(&decoded), &empty).has(mailrs_fraud::RULE_CLAIMS_OUR_NAME),
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
