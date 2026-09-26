//! Re-evaluate historical mail with the same Lua rules as the receiver.
//!
//! Hot reload changes future scans; existing verdicts change only when
//! this bounded sweep is explicitly applied.
//!
//! # Dry by default
//!
//! The first run reports and changes nothing. That is not politeness —
//! it is the only way to see what a new signal would have done to a
//! real mailbox before it does it. Pass `dry_run=false` to move
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
use config::{our_authserv_id, policy_from_env};
use reading::*;

/// Threads between pauses.
const PAUSE_EVERY: u64 = 25;

pub(crate) use decision::*;

mod config;
mod decision;
mod holding;

pub(crate) fn snapshot(state: &FastcoreState) -> std::io::Result<Vec<(String, String)>> {
    let mut targets = Vec::new();
    for user in state.mailbox.list_account_addresses()? {
        for tid in state.mailbox.all_thread_ids_for_user(&user)? {
            targets.push((user.clone(), tid));
        }
    }
    targets.sort();
    Ok(targets)
}

/// `POST /v1/admin/maintenance:fraud-rescan?dry_run=false`
pub(crate) async fn fraud_rescan_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RescanQuery>,
) -> axum::response::Response {
    rescan(state, q, None, None).await
}

/// The background worker supplies an immutable batch and pins its rule version.
pub(crate) async fn rescan(
    state: Arc<FastcoreState>,
    q: RescanQuery,
    targets: Option<Vec<(String, String)>>,
    expected_version: Option<&str>,
) -> axum::response::Response {
    let policy = policy_from_env(&state);
    // This deployment's own `authserv-id`, for telling its stamp from
    // a forwarder's — `None` when the process was not told, which the
    // reader logs.
    let our_stamp = our_authserv_id();
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
    let targets = match targets.map(Ok).unwrap_or_else(|| snapshot(&state)) {
        Ok(targets) => targets,
        Err(e) => {
            tracing::error!(err = %e, "fraud snapshot failed");
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
    // Not held, but scored into Junk — kept apart from `found`, which
    // has always meant "hold-grade", so an old reading still compares.
    let mut scored_found = 0u64;
    let mut scored_moved = 0u64;
    let mut scored_already_junk = 0u64;
    let mut scored_by_reason: HashMap<String, u64> = HashMap::new();
    let mut scored_samples: Vec<serde_json::Value> = Vec::new();

    for (user, tid) in &targets {
        seen += 1;
        if seen <= q.skip {
            continue;
        }
        if walked >= q.limit {
            stopped_early = true;
            break;
        }
        walked += 1;
        if walked.is_multiple_of(PAUSE_EVERY) {
            tokio::time::sleep(std::time::Duration::from_millis(q.pause_ms)).await;
        }

        let Some((message_id, raw)) = newest_raw(&state, user, tid) else {
            no_file += 1;
            // A conversation with no file cannot be re-judged, so
            // it can never be released either — it stays hidden
            // whatever the rules say afterwards. Counted apart
            // from `no_file`, which otherwise folds that together
            // with the ordinary case of a thread that was not
            // held in the first place.
            if state
                .mailbox
                .get_thread_for_user(user, tid)
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
        let auth = mailrs_inbound::identity::auth_results_tokens(&raw);
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
            // How this message was submitted, read back off the
            // message itself: the receiver stamps
            // `Authentication-Results:` on the inbound path only, and
            // that path runs only for sessions that did not
            // authenticate.  Without `MAILRS_HOSTNAME` this process
            // cannot tell its own stamp from a forwarder's, so it says
            // "not known" and the rules that need it decline — see the
            // warning in `policy_from_env`'s neighbour below.
            unauthenticated: our_stamp.as_deref().is_some_and(|host| {
                mailrs_inbound::identity::auth_results_authserv(&raw).as_deref() == Some(host)
            }),
            // Where it came from and what alignment said, both read
            // back off what the receiver wrote down: the connection is
            // gone and the DNS answers of the day are not worth
            // re-asking.  A message with no `Received:` line to read
            // is treated as arriving from outside, which is the
            // conservative direction for the only rule that asks.
            peer_is_private: mailrs_inbound::identity::first_received_peer_ip(&raw)
                .is_some_and(mailrs_inbound::identity::address_is_private),
            spf: &auth.0,
            dkim: &auth.1,
            dmarc: &auth.2,
        };
        let scan = match mailrs_fraud_lua::scan(&facts, &policy) {
            Ok(scan) => scan,
            Err(error) => {
                tracing::error!(%error, %user, %tid, "fraud rescan evaluation failed; leaving thread unchanged");
                verdict_failed += 1;
                continue;
            }
        };
        if expected_version.is_some_and(|version| version != scan.version) {
            verdict_failed += 1;
            continue;
        }
        let findings = scan.findings;
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
                .get_thread_for_user(user, tid)
                .ok()
                .flatten()
                .is_some_and(|t| t.quarantined);
            match disposition(false, junks_on_its_own(&findings), held_now) {
                Disposition::Release => {}
                Disposition::Junk => {
                    scored_found += 1;
                    for r in findings.rules() {
                        *scored_by_reason.entry(r.to_string()).or_default() += 1;
                    }
                    if scored_samples.len() < 25 {
                        scored_samples.push(serde_json::json!({
                            "user": user,
                            "thread": tid,
                            "from": from,
                            "subject": subject,
                            "reasons": findings.rules(),
                            "score": findings.score(),
                        }));
                    }
                    if !q.dry_run {
                        match move_to_junk(&state, user, tid) {
                            Some(true) => scored_already_junk += 1,
                            Some(false) => scored_moved += 1,
                            None => {}
                        }
                    }
                    continue;
                }
                Disposition::Act | Disposition::Leave => continue,
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
            match state.mailbox.set_quarantined(user, tid, false) {
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
            Action::Junk => match move_to_junk(&state, user, tid) {
                Some(true) => already_junk += 1,
                Some(false) => moved += 1,
                None => {}
            },
            Action::Hold => {
                let mut verdict = rescan_verdict(&raw, &findings);
                verdict.rules_version = scan.version.clone();
                match holding::apply(&state, user, tid, &message_id, &verdict) {
                    Ok(changed) => held += u64::from(changed),
                    Err(error) => {
                        verdict_failed += 1;
                        tracing::error!(%error, %user, %tid, "fraud hold incomplete");
                    }
                }
            }
            Action::Delete => match state.mailbox.delete_thread(user, tid) {
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
        "scored_found": scored_found,
        "scored_moved_to_junk": scored_moved,
        "scored_already_junk": scored_already_junk,
        "scored_by_reason": scored_by_reason,
        "scored_samples": scored_samples,
    }))
    .into_response()
}

/// Move a conversation to Junk. `Some(true)` when it was already
/// there, `None` when the write failed (logged).
///
/// **Reads the bucket first.** `set_junk` answers "did the row exist",
/// not "did anything change" — so counting its `true` as a move made
/// `already_junk` a number that could not come out other than zero,
/// and a second run reported moving fifty threads that were already in
/// Junk.
fn move_to_junk(state: &FastcoreState, user: &str, tid: &str) -> Option<bool> {
    let was_junk = state
        .mailbox
        .get_thread_for_user(user, tid)
        .ok()
        .flatten()
        .is_some_and(|r| {
            // `bucket_of`, not a literal: the category a Junk row
            // carries is `spam`, and the first version of this
            // compared against "junk" and was therefore never true.
            mailrs_mailbox_kevy::keys::bucket_of(&r.category)
                == mailrs_mailbox_kevy::keys::Bucket::Junk
        });
    match state.mailbox.set_junk(user, tid, true) {
        Ok(_) => Some(was_junk),
        Err(e) => {
            tracing::warn!(err = %e, %user, %tid, "fraud rescan: set_junk failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {

    /// Everything the sweep reads off a stored message, in one place,
    /// so a test can hand it the bytes production stored.
    fn facts_as_the_sweep_reads_them(raw: &[u8], our_stamp: &str) -> (String, bool, bool, String) {
        let stamp = mailrs_inbound::identity::auth_results_authserv(raw);
        let (_, _, dmarc) = mailrs_inbound::identity::auth_results_tokens(raw);
        (
            mailrs_inbound::from_header(raw),
            stamp.as_deref() == Some(our_stamp),
            mailrs_inbound::identity::first_received_peer_ip(raw)
                .is_some_and(mailrs_inbound::identity::address_is_private),
            dmarc,
        )
    }

    fn holds_as_the_sweep_would(raw: &[u8]) -> bool {
        let (from, unauthenticated, peer_is_private, dmarc) =
            facts_as_the_sweep_reads_them(raw, "mail.golia.ai");
        let facts = mailrs_fraud::Facts {
            from: &from,
            subject: &mailrs_inbound::subject_header(raw),
            unauthenticated,
            peer_is_private,
            dmarc: &dmarc,
            ..mailrs_fraud::Facts::default()
        };
        let policy = mailrs_fraud::Policy {
            our_domains: vec!["golia.jp".into(), "golia.ai".into()],
            ..mailrs_fraud::Policy::default()
        };
        let mut rules = mailrs_fraud_lua::Rules::compile(mailrs_fraud_lua::DEFAULT_SOURCE)
            .expect("shipped bundle compiles");
        let findings = rules.classify(&facts, &policy).findings;
        assert!(
            findings.has(mailrs_fraud::RULE_CLAIMS_OUR_DOMAIN) == findings.hold_worthy()
                || !findings.has(mailrs_fraud::RULE_CLAIMS_OUR_DOMAIN),
            "{findings:?}"
        );
        findings.has(mailrs_fraud::RULE_CLAIMS_OUR_DOMAIN)
    }

    /// The message this rule was written for, as production stored it
    /// (`lihao/cur/1789680401.…`, 2026-09-18 06:26 JST).  It went to
    /// Junk on `dmarc=fail` and nothing held it, so it never reached
    /// the review screen.
    ///
    /// Our own receiver stamped it — so it arrived as a stranger — from
    /// a public address, and DMARC failed.  The `spf=pass; dkim=pass`
    /// belong to `pvyo.cn`, the sender's own domain, aligned with
    /// nothing.
    #[test]
    fn the_reported_bec_message_is_held() {
        let raw: &[u8] = b"Authentication-Results: mail.golia.ai;\r\n\
\tspf=pass;\r\n\tdkim=pass;\r\n\tarc=none;\r\n\
\tdmarc=fail reason=\"policy=quarantine\"\r\n\
Received: from mail.golia.ai (162.4.137.23:39822)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
Sender: <mliwzodler@pvyo.cn>\r\n\
From: =?utf-8?B?6b2L6JekIOecnw==?= <aiyhccspbu@golia.jp>\r\n\
To: \"finance@golia.jp\" <finance@golia.jp>\r\n\
Subject: =?utf-8?B?44Ku44Oq44Ki5qCq5byP5Lya56S+IOalreWLmeWkieabtA==?=\r\n\r\nbody\r\n";

        let (_, unauth, private, dmarc) = facts_as_the_sweep_reads_them(raw, "mail.golia.ai");
        assert!(unauth, "our own stamp is on it");
        assert!(!private, "162.4.137.23 is not one of ours");
        assert_eq!(dmarc, "fail");
        assert!(holds_as_the_sweep_would(raw));
    }

    /// And this deployment's own control plane, as production stored it
    /// — `devops@golia.jp`, submitted to the MX from the container
    /// bridge without SMTP AUTH and unsigned, so authentication reads
    /// exactly like the forgery above.
    ///
    /// The first version of the rule held 62 of these.  The user
    /// noticed within the hour: "这不是 review 应该有的啊，这是我们自己
    /// 系统发的".
    #[test]
    fn our_own_control_plane_mail_is_not_held() {
        let raw: &[u8] = b"Authentication-Results: mail.golia.ai;\r\n\
\tspf=softfail;\r\n\tdkim=none;\r\n\tarc=none;\r\n\
\tdmarc=fail reason=\"policy=quarantine\"\r\n\
Received: from mail.golia.ai (172.18.0.1:58646)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
From: devops@golia.jp\r\n\
To: lihao@golia.jp\r\n\
Subject: [devops] expo SDK 58 - latest 57.0.23 -> 57.0.24\r\n\r\nbody\r\n";

        let (_, unauth, private, dmarc) = facts_as_the_sweep_reads_them(raw, "mail.golia.ai");
        assert!(unauth, "it did not authenticate either");
        assert!(private, "172.18.0.1 is this host");
        assert_eq!(dmarc, "fail", "and nothing signed it");
        assert!(
            !holds_as_the_sweep_would(raw),
            "our own control plane was held"
        );
    }

    /// The third shape production has: a public sender at our domain
    /// that DMARC verified.  Aligned means it is us.
    #[test]
    fn a_verified_public_sender_at_our_domain_is_not_held() {
        let raw: &[u8] = b"Authentication-Results: mail.golia.ai;\r\n\
\tspf=pass;\r\n\tdkim=none;\r\n\tdmarc=pass\r\n\
Received: from mail.golia.ai (115.124.28.66:22574)\r\n\
\tby mail.golia.ai with ESMTP\r\n\
From: noreply@golia.jp\r\n\
To: lihao@golia.jp\r\n\
Subject: notice\r\n\r\nbody\r\n";
        assert!(!holds_as_the_sweep_would(raw));
    }
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
        assert_eq!(disposition(false, false, true), Disposition::Release);
        assert_eq!(disposition(false, true, true), Disposition::Release);
    }

    /// And the three that must not move.
    #[test]
    fn nothing_else_is_released() {
        assert_eq!(disposition(true, false, true), Disposition::Act);
        assert_eq!(disposition(true, false, false), Disposition::Act);
        assert_eq!(disposition(true, true, false), Disposition::Act);
        assert_eq!(
            disposition(false, false, false),
            Disposition::Leave,
            "mail that was never held is not touched by the release path"
        );
    }

    /// A score that reaches the Junk threshold on its own moves a
    /// conversation nothing holds — and only that: below the
    /// threshold it is left, and a hold still wins.
    #[test]
    fn a_score_alone_at_the_threshold_is_junked_and_nothing_less() {
        assert_eq!(disposition(false, true, false), Disposition::Junk);
        assert_eq!(disposition(false, false, false), Disposition::Leave);
        assert_eq!(disposition(true, true, false), Disposition::Act);
    }

    /// The reported `Hao` message scores into Junk by itself, and a
    /// message that trips nothing does not. Read through the same
    /// function the sweep calls, so the threshold is the one it uses.
    #[test]
    fn the_cut_from_the_address_campaign_reaches_junk_in_the_sweep() {
        let policy = mailrs_fraud::Policy {
            our_domains: vec!["golia.jp".into()],
            ..mailrs_fraud::Policy::default()
        };
        let mut rules = mailrs_fraud_lua::Rules::compile(mailrs_fraud_lua::DEFAULT_SOURCE)
            .expect("shipped bundle compiles");
        let mut judge = |subject: &str| {
            let facts = mailrs_fraud::Facts {
                from: "Erica Flores <ericaf_flores@caredealspark.com>",
                subject,
                to_display: "lihao@golia.jp",
                ..mailrs_fraud::Facts::default()
            };
            let f = rules.classify(&facts, &policy).findings;
            (mailrs_inbound::holds(&f), junks_on_its_own(&f))
        };
        assert_eq!(judge("Hao"), (false, true));
        assert_eq!(judge("Quarterly contracts"), (false, false));
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
