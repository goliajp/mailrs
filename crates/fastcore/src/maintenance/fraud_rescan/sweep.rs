//! The sweep itself: walk the batch, judge each thread, answer the tally.

use super::*;

/// One walk: what it judges with, and what it has counted so far.
struct Sweep<'a> {
    state: &'a Arc<FastcoreState>,
    q: &'a RescanQuery,
    policy: &'a mailrs_fraud::Policy,
    our_stamp: Option<&'a str>,
    expected_version: Option<&'a str>,
    hist: Option<kevy_client::Connection>,
    t: Tally,
}

/// The counts and samples the answer reports.
#[derive(Default)]
struct Tally {
    found: u64,
    already_junk: u64,
    moved: u64,
    deleted: u64,
    held: u64,
    verdict_failed: u64,
    // Held by a rule that no longer holds. Counted separately from
    // `released` so a dry run says how many *would* be let go — a
    // single number could not be told apart from "none were".
    held_but_unreadable: u64,
    releasable: u64,
    released: u64,
    release_samples: Vec<serde_json::Value>,
    no_file: u64,
    // Which check fired, because "12 found" does not say whether the
    // one that needs an allow-list entry is among them.
    by_reason: HashMap<String, u64>,
    samples: Vec<serde_json::Value>,
    // Not held, but scored into Junk — kept apart from `found`, which
    // has always meant "hold-grade", so an old reading still compares.
    scored_found: u64,
    scored_moved: u64,
    scored_already_junk: u64,
    scored_whitelisted: u64,
    scored_by_reason: HashMap<String, u64>,
    scored_samples: Vec<serde_json::Value>,
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
    let hist = crate::live_sync::network_kevy_url()
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
    let mut sweep = Sweep {
        state: &state,
        q: &q,
        policy: &policy,
        our_stamp: our_stamp.as_deref(),
        expected_version,
        hist,
        t: Tally::default(),
    };

    let mut seen = 0u64;
    let mut walked = 0u64;
    let mut stopped_early = false;
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
        sweep.thread(user, tid);
    }

    sweep.t.into_response(&q, walked, stopped_early)
}

impl Sweep<'_> {
    fn is_held(&self, user: &str, tid: &str) -> bool {
        self.state
            .mailbox
            .get_thread_for_user(user, tid)
            .ok()
            .flatten()
            .is_some_and(|t| t.quarantined)
    }

    fn thread(&mut self, user: &str, tid: &str) {
        let Some((message_id, raw)) = newest_raw(self.state, user, tid) else {
            self.t.no_file += 1;
            // A conversation with no file cannot be re-judged, so
            // it can never be released either — it stays hidden
            // whatever the rules say afterwards. Counted apart
            // from `no_file`, which otherwise folds that together
            // with the ordinary case of a thread that was not
            // held in the first place.
            if self.is_held(user, tid) {
                self.t.held_but_unreadable += 1;
            }
            return;
        };
        // What extraction stored from this message's attachments.
        let attachment_texts = self
            .state
            .mailbox
            .attachment_texts(&message_id)
            .unwrap_or_default();
        let msg = facts::Reading::of(&raw, self.our_stamp, &attachment_texts);
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
        let seen = domain_seen(self.hist.as_mut(), &msg.from);
        // Read, not recorded. The sweep walks the same message
        // more than once over its life, and a set does not care
        // — but the live path is the one that owns the writing,
        // and a sweep that also wrote would make "how many
        // domains" depend on how often the sweep had run.
        let reply_rotation = reading::reply_rotation(&mut self.hist, &msg.host, &msg.reply_to);
        let facts = msg.facts(seen, reply_rotation);
        let scan = match mailrs_fraud_lua::scan(&facts, self.policy) {
            Ok(scan) => scan,
            Err(error) => {
                tracing::error!(%error, %user, %tid, "fraud rescan evaluation failed; leaving thread unchanged");
                self.t.verdict_failed += 1;
                return;
            }
        };
        if self
            .expected_version
            .is_some_and(|version| version != scan.version)
        {
            self.t.verdict_failed += 1;
            return;
        }
        let findings = scan.findings;
        // The one definition of "this is held", shared with the
        // verdict this sweep is about to store. `findings.any()`
        // here and a score threshold there is what left 43 held
        // conversations carrying a verdict that said otherwise.
        if !mailrs_inbound::holds(&findings) {
            self.not_held(user, tid, &raw, &msg, &findings);
            return;
        }
        self.t.found += 1;
        for r in findings.rules() {
            *self.t.by_reason.entry(r.to_string()).or_default() += 1;
        }
        if self.t.samples.len() < 25 {
            self.t.samples.push(serde_json::json!({
                "user": user,
                "thread": tid,
                "from": &msg.from,
                "reasons": findings.rules(),
                "score": findings.score(),
            }));
        }
        if self.q.dry_run {
            return;
        }
        self.act(user, tid, &message_id, &raw, &findings, &scan.version);
    }

    /// A thread the rules do not hold: release it, score it into Junk,
    /// or leave it.
    fn not_held(
        &mut self,
        user: &str,
        tid: &str,
        raw: &[u8],
        msg: &facts::Reading,
        findings: &mailrs_fraud::Findings,
    ) {
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
        let held_now = self.is_held(user, tid);
        match disposition(false, junks_on_its_own(findings), held_now) {
            Disposition::Release => {}
            Disposition::Junk => {
                self.score_into_junk(user, tid, raw, msg, findings);
                return;
            }
            Disposition::Act | Disposition::Leave => return,
        }
        self.t.releasable += 1;
        if self.t.release_samples.len() < 25 {
            self.t.release_samples.push(serde_json::json!({
                "user": user,
                "thread": tid,
                "from": &msg.from,
                "still_scored": findings.score(),
            }));
        }
        if self.q.dry_run {
            return;
        }
        match self.state.mailbox.set_quarantined(user, tid, false) {
            Ok(_) => self.t.released += 1,
            Err(e) => tracing::warn!(err = %e, tid, "release failed"),
        }
    }

    fn score_into_junk(
        &mut self,
        user: &str,
        tid: &str,
        raw: &[u8],
        msg: &facts::Reading,
        findings: &mailrs_fraud::Findings,
    ) {
        // the receive path lets a whitelisted sender past
        // every score, so the sweep must not score them back
        if sender_whitelisted(&mut self.hist, user, raw) {
            self.t.scored_whitelisted += 1;
            return;
        }
        self.t.scored_found += 1;
        for r in findings.rules() {
            *self.t.scored_by_reason.entry(r.to_string()).or_default() += 1;
        }
        if self.t.scored_samples.len() < 25 {
            self.t.scored_samples.push(serde_json::json!({
                "user": user,
                "thread": tid,
                "from": &msg.from,
                "subject": &msg.subject,
                "reasons": findings.rules(),
                "score": findings.score(),
            }));
        }
        if !self.q.dry_run {
            match move_to_junk(self.state, user, tid) {
                Some(true) => self.t.scored_already_junk += 1,
                Some(false) => self.t.scored_moved += 1,
                None => {}
            }
        }
    }

    /// Carry out the requested action on a held thread.
    fn act(
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
                match holding::apply(state, user, tid, message_id, &verdict) {
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
                        crate::routes::message_ops::unlink_maildir_file(
                            &state.maildir_root,
                            user,
                            b,
                        );
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

impl Tally {
    fn into_response(
        self,
        q: &RescanQuery,
        walked: u64,
        stopped_early: bool,
    ) -> axum::response::Response {
        tracing::info!(
            walked,
            found = self.found,
            moved = self.moved,
            deleted = self.deleted,
            held = self.held,
            verdict_failed = self.verdict_failed,
            releasable = self.releasable,
            released = self.released,
            held_but_unreadable = self.held_but_unreadable,
            already_junk = self.already_junk,
            no_file = self.no_file,
            dry_run = q.dry_run,
            "fraud-rescan complete"
        );
        Json(serde_json::json!({
            "done": !stopped_early,
            "next_skip": q.skip + walked,
            "dry_run": q.dry_run,
            "threads_walked": walked,
            "found": self.found,
            "moved_to_junk": self.moved,
            "deleted": self.deleted,
            "held": self.held,
            // Held but unable to explain itself. Reported rather than
            // folded into `held`, because a hold nobody can see the
            // reasons for is the one that turns into "my mail vanished".
            "verdict_failed": self.verdict_failed,
            // Held by a rule that no longer holds anything. Two numbers
            // rather than one: a dry run can only report `releasable`,
            // and a single figure of 0 would not say whether it found
            // none or was not allowed to act.
            "releasable": self.releasable,
            "released": self.released,
            // Hidden, and its file is gone: it cannot be re-judged, so
            // no rule change will ever let it out. Zero is the answer
            // that means what it says.
            "held_but_unreadable": self.held_but_unreadable,
            "release_samples": self.release_samples,
            "already_junk": self.already_junk,
            "no_file": self.no_file,
            "by_reason": self.by_reason,
            "samples": self.samples,
            "scored_found": self.scored_found,
            "scored_moved_to_junk": self.scored_moved,
            "scored_already_junk": self.scored_already_junk,
            "scored_but_whitelisted": self.scored_whitelisted,
            "scored_by_reason": self.scored_by_reason,
            "scored_samples": self.scored_samples,
        }))
        .into_response()
    }
}
