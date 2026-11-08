//! Move authenticated newsletters the score put in Junk to the bucket
//! triage gives them. One-off: the score that put them there no longer
//! junks such mail (`mailrs_inbound::decision::counted`).
//!
//! A conversation moves when its newest message is inside the window and
//! every message of it is authenticated list mail (`verified` sender,
//! DMARC pass, `List-Unsubscribe` or `List-Id`) with no stored verdict, or
//! a verdict of the score alone in which the classifier decided
//! ([`judge::judge`]). Never one the fraud rules hold, and never one the
//! reader filed as Junk (a live `UserJunk` observation of theirs, or the
//! training marker their filing left). The bucket is the reader's sender
//! rule if they have one, else the triage classifier's when it says
//! notification or promotion, else the list-header label the triage
//! backfill uses. Nothing is trained: the move is ours, not the reader's.
//!
//! Dry by default. Paged over each run's roster of every account's Junk
//! conversations, sorted; a page that moves conversations takes them out
//! of the roster, so its `next_skip` counts only the ones left. Each move
//! is audited. A second run finds nothing to move.

mod judge;

use std::collections::BTreeMap;

use mailrs_mailbox_kevy::keys::Bucket;

use super::hangup::{Hangup, run_blocking};
use super::prelude::*;
use judge::{Seen, Stay};

const DAY: i64 = 86_400;

#[derive(serde::Deserialize)]
pub(crate) struct CorrectQuery {
    /// Report without moving anything. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    skip: usize,
    /// Only conversations whose newest message is this recent.
    #[serde(default = "default_days")]
    days: i64,
}

fn yes() -> bool {
    true
}
fn default_limit() -> usize {
    500
}
fn default_days() -> i64 {
    90
}

/// `POST /v1/admin/maintenance:junk-bulk-correct?dry_run=false&skip=0&limit=500&days=90`
pub(crate) async fn junk_bulk_correct_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<CorrectQuery>,
) -> axum::response::Response {
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let now = chrono::Utc::now().timestamp();
    match run_blocking(move |h| correct(&state, &mut conn, &q, now, h)).await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "junk-bulk-correct: reading the roster failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "junk-bulk-correct: walk panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[derive(Default, serde::Serialize)]
struct PerAccount {
    walked: u64,
    moved: u64,
    stayed: BTreeMap<&'static str, u64>,
}

/// One conversation that moved, or on a dry run would.
#[derive(serde::Serialize)]
struct Moved {
    user: String,
    thread_id: String,
    sender_domain: String,
    subject: String,
    bucket: &'static str,
    /// A message of it had a stored verdict (the score's, classifier
    /// decisive); otherwise none had one.
    verdict: bool,
}

fn correct(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    q: &CorrectQuery,
    now: i64,
    hangup: &Hangup,
) -> std::io::Result<serde_json::Value> {
    let all = mailrs_core_sidestate::families::screening::observations(conn)?;
    let mut roster: Vec<(String, String)> = Vec::new();
    for user in state.mailbox.list_account_addresses()? {
        for tid in state.mailbox.list_thread_ids_by_bucket_via_table(
            &user,
            "junk",
            mailrs_mailbox_kevy::ArchiveScope::All,
            usize::MAX,
        )? {
            roster.push((user.clone(), tid));
        }
    }
    roster.sort();

    let mut accounts: BTreeMap<String, PerAccount> = BTreeMap::new();
    let mut moved: Vec<Moved> = Vec::new();
    let mut walked = 0usize;
    let mut junked: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    for (user, tid) in roster.iter().skip(q.skip).take(q.limit) {
        if hangup.hung_up() {
            break;
        }
        walked += 1;
        let acct = accounts.entry(user.clone()).or_default();
        acct.walked += 1;
        let reader = junked.entry(user.clone()).or_insert_with(|| {
            mailrs_core_sidestate::families::junk_corrections::user_junked_threads(&all, user)
        });
        let marker =
            mailrs_triage::spam::trained_as_spam(conn, mailrs_triage::spam::Corpus::Legacy, tid);
        let found = match reader.contains(tid) || marker == Some(true) {
            true => Err(Stay::ReaderJunked),
            false => examine(state, conn, user, tid, now - q.days * DAY),
        };
        let m = match found {
            Ok(m) => m,
            Err(stay) => {
                *acct.stayed.entry(stay.as_str()).or_default() += 1;
                continue;
            }
        };
        if !q.dry_run {
            let target = match m.bucket {
                "notification" => Bucket::Notifications,
                "promotion" => Bucket::Promotions,
                _ => Bucket::Inbox,
            };
            if !state.mailbox.set_bucket(user, tid, target)? {
                continue;
            }
            mailrs_core_sidestate::families::audit::record_by_core(
                state.as_ref(),
                "maintenance",
                "junk.bulk_correct",
                tid,
                &format!("user={user} to={} reason=authenticated-list-mail", m.bucket),
                now,
            );
        }
        acct.moved += 1;
        moved.push(m);
    }
    let moved_here = match q.dry_run {
        true => 0,
        false => moved.len(),
    };
    let next_skip = q.skip + walked - moved_here;
    let mut domains: BTreeMap<&str, u64> = BTreeMap::new();
    for m in &moved {
        *domains.entry(m.sender_domain.as_str()).or_default() += 1;
    }
    let mut top: Vec<(&str, u64)> = domains.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    top.truncate(30);
    tracing::info!(
        walked,
        moved = moved.len(),
        dry_run = q.dry_run,
        "junk-bulk-correct"
    );
    Ok(serde_json::json!({
        "dry_run": q.dry_run,
        // the input: zero here is no Junk at all, not finished work
        "roster": roster.len(),
        "skip": q.skip,
        "walked": walked,
        "next_skip": next_skip,
        "done": next_skip >= roster.len() - moved_here,
        // moved, or on a dry run would be
        "moved": moved.len(),
        "moved_with_verdict": moved.iter().filter(|m| m.verdict).count(),
        "accounts": accounts,
        "top_sender_domains": top,
        "conversations": moved,
    }))
}

/// Whether `tid` may move for `user`, and where to.
fn examine(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    user: &str,
    tid: &str,
    since: i64,
) -> Result<Moved, Stay> {
    let row = state
        .mailbox
        .get_thread_for_user(user, tid)
        .ok()
        .flatten()
        .ok_or(Stay::NoFile)?;
    if row.quarantined {
        return Err(Stay::Held);
    }
    if row.latest_date < since {
        return Err(Stay::OutOfWindow);
    }
    let mids = state
        .mailbox
        .user_thread_message_ids(user, tid)
        .unwrap_or_default();
    let mut first: Option<Vec<u8>> = None;
    let mut had_verdict = false;
    for mid in &mids {
        let wire = state
            .mailbox
            .user_message_view(user, mid)
            .ok()
            .flatten()
            .and_then(|b| {
                serde_json::from_slice::<mailrs_core_api::method::message::MessageWire>(&b).ok()
            })
            .ok_or(Stay::NoFile)?;
        let raw =
            crate::maildir_scan::read_maildir_file_in(&state.maildir_root, user, &wire.blob_ref)
                .ok_or(Stay::NoFile)?;
        // stored under the id the arrival carried
        let verdict = state.mailbox.fraud_verdict(&wire.message_id).ok().flatten();
        had_verdict |= verdict.is_some();
        judge::judge(&Seen {
            sender_trust: &wire.sender_trust,
            raw: &raw,
            verdict: verdict.as_deref(),
        })?;
        first.get_or_insert(raw);
    }
    let raw = first.ok_or(Stay::NoFile)?;
    let from = mailrs_inbound::identity::from_address(&raw);
    let bucket = bucket_for(state, conn, user, &from, &raw).ok_or(Stay::NoBucket)?;
    Ok(Moved {
        user: user.to_string(),
        thread_id: tid.to_string(),
        sender_domain: from
            .rsplit_once('@')
            .map(|(_, d)| d.to_ascii_lowercase())
            .unwrap_or_default(),
        subject: row.subject,
        bucket,
        verdict: had_verdict,
    })
}

/// The bucket triage gives this mail today: the reader's sender rule, else
/// the classifier when it says notification or promotion, else the label
/// its list headers give.
fn bucket_for(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    user: &str,
    from: &str,
    raw: &[u8],
) -> Option<&'static str> {
    if let Some(rule) =
        mailrs_intake::arrival::sender_rule_category(Some(conn), user, from, "INBOX")
    {
        return Some(rule);
    }
    match crate::bayes_train::classify_triage(state, raw) {
        Some(c @ ("notification" | "promotion")) => Some(c),
        _ => match crate::bayes_train::heuristic_triage_label(raw) {
            l @ ("notification" | "promotion") => Some(l),
            _ => None,
        },
    }
}
