//! Retrain the v2 corpora, the ones the decoded-text tokenizer reads.
//!
//! Spam/ham: the thread ids the raw-byte corpus was trained on are still in
//! its sets (`bayes:corpus:spam`, `bayes:corpus:ham`). A thread id names no
//! account, so its owners come from the per-user message rows (the declared
//! `tid` index), and its raw mail from the first owner whose maildir still
//! has the files. Triage: each account's threads in Inbox, Notifications and
//! Promotions are learnt as the class of the bucket they sit in now, which is
//! the classification the readers have kept or corrected. Unlike
//! `backfill-triage` this files nothing.
//!
//! A page is a slice of a roster sorted by key, so pages taken with `skip`
//! cover the same threads a single run would, and training is reversible per
//! thread, so a page run twice counts once. Dry by default.

use std::collections::BTreeSet;

use mailrs_triage::spam::{LEGACY_CORPUS_HAM, LEGACY_CORPUS_SPAM};

use super::hangup::{Hangup, run_blocking};
use super::prelude::*;
use crate::bayes_train::fetch_thread_raw;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Corpus {
    Spam,
    Triage,
}

#[derive(serde::Deserialize)]
pub(crate) struct RebuildQuery {
    corpus: Corpus,
    /// Report without writing. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    skip: usize,
}

fn yes() -> bool {
    true
}
fn default_limit() -> usize {
    500
}

/// `POST /v1/admin/maintenance:bayes-rebuild-v2?corpus=spam|triage&dry_run=false&skip=0&limit=500`
pub(crate) async fn bayes_rebuild_v2_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RebuildQuery>,
) -> axum::response::Response {
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let work = run_blocking(move |h| match q.corpus {
        Corpus::Spam => rebuild_spam(&state, &mut conn, &q, h),
        Corpus::Triage => rebuild_triage(&state, &mut conn, &q, h),
    });
    match work.await {
        Ok(Ok(report)) => Json(report).into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "bayes-rebuild-v2: reading the roster failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "bayes-rebuild-v2: walk panicked");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn members(conn: &mut kevy_client::Connection, key: &[u8]) -> std::io::Result<BTreeSet<String>> {
    Ok(conn
        .smembers(key)?
        .into_iter()
        .filter_map(|m| String::from_utf8(m).ok())
        .collect())
}

/// Every account holding a message of `tid`, each once, in index order.
fn owners_of(state: &FastcoreState, tid: &str) -> std::io::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for (user, _) in state.mailbox.thread_message_owners(tid)? {
        if !out.contains(&user) {
            out.push(user);
        }
    }
    Ok(out)
}

fn rebuild_spam(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    q: &RebuildQuery,
    hangup: &Hangup,
) -> std::io::Result<serde_json::Value> {
    let spam = members(conn, LEGACY_CORPUS_SPAM)?;
    let ham = members(conn, LEGACY_CORPUS_HAM)?;
    let roster: BTreeSet<&String> = spam.iter().chain(ham.iter()).collect();

    let mut walked = 0usize;
    let (mut as_spam, mut as_ham) = (0u64, 0u64);
    let (mut in_both, mut in_both_unknown) = (0u64, 0u64);
    let (mut no_owner, mut no_raw) = (0u64, 0u64);
    for tid in roster.iter().skip(q.skip).take(q.limit) {
        if hangup.hung_up() {
            break;
        }
        walked += 1;
        let is_spam = match (spam.contains(*tid), ham.contains(*tid)) {
            (true, true) => {
                in_both += 1;
                // the old marker says which way it went last, while it lasts
                match mailrs_triage::spam::legacy_trained_as_spam(conn, tid) {
                    Some(s) => s,
                    None => {
                        in_both_unknown += 1;
                        continue;
                    }
                }
            }
            (s, _) => s,
        };
        let owners = owners_of(state, tid)?;
        if owners.is_empty() {
            no_owner += 1;
            continue;
        }
        let Some(raws) = owners
            .iter()
            .map(|user| fetch_thread_raw(state, user, tid))
            .find(|raws| !raws.is_empty())
        else {
            no_raw += 1;
            continue;
        };
        match is_spam {
            true => as_spam += 1,
            false => as_ham += 1,
        }
        if !q.dry_run {
            mailrs_triage::spam::train(conn, tid, &raws, is_spam);
        }
    }
    let next_skip = q.skip + walked;
    tracing::info!(
        walked,
        as_spam,
        as_ham,
        no_owner,
        no_raw,
        in_both_unknown,
        dry_run = q.dry_run,
        "bayes-rebuild-v2 spam"
    );
    Ok(serde_json::json!({
        "corpus": "spam",
        "dry_run": q.dry_run,
        // the input: zero here is an empty legacy corpus, not finished work
        "legacy_spam_ids": spam.len(),
        "legacy_ham_ids": ham.len(),
        "roster": roster.len(),
        "skip": q.skip,
        "walked": walked,
        "next_skip": next_skip,
        "done": next_skip >= roster.len(),
        // trained, or on a dry run what would be
        "spam": as_spam,
        "ham": as_ham,
        "in_both_sets": in_both,
        // could not be resolved to raw mail, or to a direction
        "unresolved": no_owner + no_raw + in_both_unknown,
        "no_owner": no_owner,
        "no_raw": no_raw,
        "in_both_unknown": in_both_unknown,
    }))
}

fn rebuild_triage(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    q: &RebuildQuery,
    hangup: &Hangup,
) -> std::io::Result<serde_json::Value> {
    let users = state.mailbox.list_account_addresses()?;
    let mut roster: Vec<(String, String, &'static str)> = Vec::new();
    for user in &users {
        for bucket in mailrs_mailbox::scope::NON_JUNK {
            let tids = state.mailbox.list_thread_ids_by_bucket_unsent_via_table(
                user,
                bucket.name(),
                mailrs_mailbox_kevy::ArchiveScope::All,
                usize::MAX,
            )?;
            roster.extend(
                tids.into_iter()
                    .map(|tid| (user.clone(), tid, bucket.category())),
            );
        }
    }
    roster.sort();

    let mut walked = 0usize;
    let mut no_raw = 0u64;
    let mut per_class = [0u64; mailrs_triage::CLASSES.len()];
    for (user, tid, class) in roster.iter().skip(q.skip).take(q.limit) {
        if hangup.hung_up() {
            break;
        }
        walked += 1;
        let raws = fetch_thread_raw(state, user, tid);
        if raws.is_empty() {
            no_raw += 1;
            continue;
        }
        if let Some(i) = mailrs_triage::CLASSES.iter().position(|c| c == class) {
            per_class[i] += 1;
        }
        if !q.dry_run {
            mailrs_triage::spam::train_class(conn, tid, &raws, class);
        }
    }
    let next_skip = q.skip + walked;
    tracing::info!(
        walked,
        no_raw,
        dry_run = q.dry_run,
        "bayes-rebuild-v2 triage"
    );
    let mut classes = serde_json::Map::new();
    for (class, n) in mailrs_triage::CLASSES.iter().zip(per_class) {
        classes.insert((*class).to_string(), n.into());
    }
    Ok(serde_json::json!({
        "corpus": "triage",
        "dry_run": q.dry_run,
        "accounts": users.len(),
        "roster": roster.len(),
        "skip": q.skip,
        "walked": walked,
        "next_skip": next_skip,
        "done": next_skip >= roster.len(),
        // trained per class, or on a dry run what would be
        "classes": classes,
        // a thread whose maildir files are gone
        "unresolved": no_raw,
    }))
}
