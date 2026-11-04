//! Retrain the v2 spam/ham corpus, the one the decoded-text tokenizer
//! reads.
//!
//! The thread ids the raw-byte corpus was trained on are still in its sets
//! (`bayes:corpus:spam`, `bayes:corpus:ham`). A thread id names no account,
//! so its owners come from the per-user message rows (the declared `tid`
//! index), and its raw mail from the first owner whose maildir still has
//! the files.
//!
//! With `source=curated` it adds ham instead: conversations readers kept
//! out of Junk, chosen by the rules in [`super::curated_ham`].
//!
//! A page is a slice of a roster sorted by key, so pages taken with `skip`
//! cover the same threads a single run would, and training is reversible per
//! thread, so a page run twice counts once. Dry by default.

use std::collections::BTreeSet;

use mailrs_triage::spam::Corpus;

use super::hangup::{Hangup, run_blocking};
use super::prelude::*;
use crate::bayes_train::fetch_thread_raw;

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Source {
    /// The thread ids in the legacy corpus sets, as they were labelled.
    #[default]
    Legacy,
    /// Ham picked from what readers kept, see `curated_ham`.
    Curated,
}

#[derive(serde::Deserialize)]
pub(crate) struct RebuildQuery {
    #[serde(default)]
    source: Source,
    /// Curated ham: at most this many conversations per sender domain.
    #[serde(default = "default_per_domain")]
    per_domain_cap: usize,
    /// Curated ham: at most this many conversations overall.
    #[serde(default = "default_total")]
    total_cap: usize,
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
fn default_per_domain() -> usize {
    super::curated_ham::DEFAULT_PER_DOMAIN
}
fn default_total() -> usize {
    super::curated_ham::DEFAULT_TOTAL
}

/// `POST /v1/admin/maintenance:bayes-rebuild-v2?dry_run=false&skip=0&limit=500`
/// (`&source=curated&per_domain_cap=20&total_cap=1000` for curated ham)
pub(crate) async fn bayes_rebuild_v2_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RebuildQuery>,
) -> axum::response::Response {
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let work = run_blocking(move |h| match q.source {
        Source::Legacy => rebuild_spam(&state, &mut conn, &q, h),
        Source::Curated => super::curated_ham::rebuild(
            &state,
            &mut conn,
            (q.skip, q.limit),
            (q.per_domain_cap, q.total_cap),
            q.dry_run,
            h,
        ),
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
    let legacy = mailrs_triage::spam::keys(Corpus::Legacy);
    let spam = members(conn, legacy.corpus_spam)?;
    let ham = members(conn, legacy.corpus_ham)?;
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
                match mailrs_triage::spam::trained_as_spam(conn, Corpus::Legacy, tid) {
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
            mailrs_triage::spam::train_in(conn, Corpus::V2, tid, &raws, is_spam);
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
