//! Retrain the v2 spam/ham corpus, the one the decoded-text tokenizer
//! reads and the receiver scores against.
//!
//! **What may be learnt as spam.** Only a judgement that is not the
//! server's own: a conversation a reader filed as Junk (a live `UserJunk`
//! observation of theirs, or the 90-day training marker their filing
//! left), or one the fraud rules hold and nobody has released. The legacy
//! corpus also learnt every conversation its score had put in Junk, so it
//! learnt that newsletters are spam and went on junking them; such a
//! conversation is left out here (`spam_dropped`), and the report counts
//! how many of those were authenticated list mail.
//!
//! The roster is the legacy corpus's sets (`bayes:corpus:spam`,
//! `bayes:corpus:ham`), each account's held conversations and the ones
//! readers filed as Junk. A thread id names no account, so its owners come
//! from the per-user message rows (the declared `tid` index), and its raw
//! mail from the first owner whose maildir still has the files.
//!
//! A page is a slice of a roster sorted by key, so pages taken with `skip`
//! cover the same threads a single run would, and training is reversible per
//! thread, so a page run twice counts once. `reset=true` on the first page
//! empties v2 before training, for a corpus that learnt from the old roster.
//! Dry by default.

use std::collections::{BTreeMap, BTreeSet};

use mailrs_triage::spam::Corpus;

use super::hangup::{Hangup, run_blocking};
use super::prelude::*;
use crate::bayes_train::fetch_thread_raw;

#[derive(serde::Deserialize)]
pub(crate) struct RebuildQuery {
    /// Report without writing. **Default true.**
    #[serde(default = "yes")]
    dry_run: bool,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    skip: usize,
    /// Empty v2 first. Only on the first page (`skip=0`) of a run that
    /// writes.
    #[serde(default)]
    reset: bool,
}

fn yes() -> bool {
    true
}
fn default_limit() -> usize {
    500
}

/// `POST /v1/admin/maintenance:bayes-rebuild-v2?dry_run=false&skip=0&limit=500&reset=true`
pub(crate) async fn bayes_rebuild_v2_route(
    State(state): State<Arc<FastcoreState>>,
    Query(q): Query<RebuildQuery>,
) -> axum::response::Response {
    let Some(mut conn) = state.net_conn() else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let work = run_blocking(move |h| rebuild_spam(&state, &mut conn, &q, h));
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

/// Which way one thread is learnt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Spam,
    Ham,
    /// In the legacy spam set on the server's word alone: not learnt.
    Dropped,
    /// In both legacy sets, and nothing says which way it went last.
    Unknown,
}

/// What is known about one thread.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Evidence {
    pub in_spam_set: bool,
    pub in_ham_set: bool,
    /// The legacy marker: the direction a reader's filing last trained.
    pub marker: Option<bool>,
    /// An owner filed it as Junk and has not taken it out.
    pub reader_junked: bool,
    /// An owner's copy is held by the fraud rules.
    pub held: bool,
}

/// The reader's latest word wins, then the reader's Junk and the fraud
/// hold; a thread only the server called spam is dropped.
pub(crate) fn direction(e: Evidence) -> Direction {
    match e.marker {
        Some(false) => return Direction::Ham,
        Some(true) => return Direction::Spam,
        None => {}
    }
    if e.reader_junked || e.held {
        return Direction::Spam;
    }
    match (e.in_spam_set, e.in_ham_set) {
        (true, true) => Direction::Unknown,
        (true, false) => Direction::Dropped,
        _ => Direction::Ham,
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

/// "Every row" for the held list: far above any account's count, and short
/// of the `offset + limit` sums the list takes, which `usize::MAX` would
/// overflow.
const EVERY: usize = 1 << 30;

/// Each account's held conversations, and the ones it filed as Junk.
struct Judged {
    held: BTreeMap<String, BTreeSet<String>>,
    junked: BTreeMap<String, BTreeSet<String>>,
}

impl Judged {
    fn read(state: &FastcoreState, conn: &mut kevy_client::Connection) -> std::io::Result<Self> {
        use mailrs_mailbox_kevy::{ListThreadsFilter, QuarantineScope};
        let all = mailrs_core_sidestate::families::screening::observations(conn)?;
        let mut out = Judged {
            held: BTreeMap::new(),
            junked: BTreeMap::new(),
        };
        for user in state.mailbox.list_account_addresses()? {
            let filter = ListThreadsFilter {
                quarantine: QuarantineScope::Only,
                ..Default::default()
            };
            let (rows, _) = state
                .mailbox
                .list_threads_by_activity(&user, &filter, 0, EVERY)?;
            out.held.insert(
                user.clone(),
                rows.into_iter().map(|r| r.thread_id).collect(),
            );
            let junked =
                mailrs_core_sidestate::families::junk_corrections::user_junked_threads(&all, &user);
            out.junked.insert(user, junked);
        }
        Ok(out)
    }

    fn threads(&self) -> impl Iterator<Item = &String> {
        self.held.values().chain(self.junked.values()).flatten()
    }

    fn has(map: &BTreeMap<String, BTreeSet<String>>, owners: &[String], tid: &str) -> bool {
        owners
            .iter()
            .any(|u| map.get(u).is_some_and(|s| s.contains(tid)))
    }
}

/// The raw mail is authenticated list mail, as the receiver judged it.
fn authenticated_list_mail(raw: &[u8]) -> bool {
    mailrs_inbound::list_mail::has_list_headers(raw)
        && mailrs_inbound::identity::auth_results_tokens(raw).2 == "pass"
}

#[derive(Default)]
struct Tally {
    walked: usize,
    spam: u64,
    ham: u64,
    spam_from_reader: u64,
    spam_from_hold: u64,
    dropped: u64,
    dropped_list_mail: u64,
    /// The dropped threads by what their first message shows: list headers
    /// or not, and the DMARC result the receiver wrote.
    dropped_by: BTreeMap<String, u64>,
    in_both: u64,
    in_both_unknown: u64,
    no_owner: u64,
    no_raw: u64,
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
    let judged = Judged::read(state, conn)?;
    let roster: BTreeSet<&String> = spam
        .iter()
        .chain(ham.iter())
        .chain(judged.threads())
        .collect();

    let cleared = match (q.reset, q.dry_run, q.skip) {
        (true, false, 0) => Some(mailrs_triage::spam::clear(conn, Corpus::V2)?),
        _ => None,
    };

    let mut t = Tally::default();
    for tid in roster.iter().skip(q.skip).take(q.limit) {
        if hangup.hung_up() {
            break;
        }
        t.walked += 1;
        let owners = owners_of(state, tid)?;
        if owners.is_empty() {
            t.no_owner += 1;
            continue;
        }
        let e = Evidence {
            in_spam_set: spam.contains(*tid),
            in_ham_set: ham.contains(*tid),
            marker: mailrs_triage::spam::trained_as_spam(conn, Corpus::Legacy, tid),
            reader_junked: Judged::has(&judged.junked, &owners, tid),
            held: Judged::has(&judged.held, &owners, tid),
        };
        if e.in_spam_set && e.in_ham_set {
            t.in_both += 1;
        }
        let way = direction(e);
        if way == Direction::Unknown {
            t.in_both_unknown += 1;
            continue;
        }
        let Some(raws) = owners
            .iter()
            .map(|user| fetch_thread_raw(state, user, tid))
            .find(|raws| !raws.is_empty())
        else {
            t.no_raw += 1;
            continue;
        };
        let is_spam = match way {
            Direction::Dropped => {
                t.dropped += 1;
                if authenticated_list_mail(&raws[0]) {
                    t.dropped_list_mail += 1;
                }
                let list = mailrs_inbound::list_mail::has_list_headers(&raws[0]);
                let dmarc = mailrs_inbound::identity::auth_results_tokens(&raws[0]).2;
                let dmarc = match dmarc.as_str() {
                    "" => "-",
                    d => d,
                };
                let key = format!("list={list} dmarc={dmarc}");
                *t.dropped_by.entry(key).or_default() += 1;
                continue;
            }
            Direction::Spam => {
                t.spam += 1;
                match e.reader_junked || e.marker == Some(true) {
                    true => t.spam_from_reader += 1,
                    false => t.spam_from_hold += 1,
                }
                true
            }
            _ => {
                t.ham += 1;
                false
            }
        };
        if !q.dry_run {
            mailrs_triage::spam::train_in(conn, Corpus::V2, tid, &raws, is_spam);
        }
    }
    let next_skip = q.skip + t.walked;
    tracing::info!(
        walked = t.walked,
        spam = t.spam,
        ham = t.ham,
        dropped = t.dropped,
        no_owner = t.no_owner,
        no_raw = t.no_raw,
        in_both_unknown = t.in_both_unknown,
        dry_run = q.dry_run,
        "bayes-rebuild-v2 spam"
    );
    Ok(serde_json::json!({
        "corpus": "spam",
        "dry_run": q.dry_run,
        // the threads v2 held before it was emptied; null when it was not
        "reset_cleared": cleared,
        // the input: zero here is an empty legacy corpus, not finished work
        "legacy_spam_ids": spam.len(),
        "legacy_ham_ids": ham.len(),
        "held_ids": judged.held.values().map(BTreeSet::len).sum::<usize>(),
        "reader_junked_ids": judged.junked.values().map(BTreeSet::len).sum::<usize>(),
        "roster": roster.len(),
        "skip": q.skip,
        "walked": t.walked,
        "next_skip": next_skip,
        "done": next_skip >= roster.len(),
        // trained, or on a dry run what would be
        "spam": t.spam,
        "spam_from_reader": t.spam_from_reader,
        "spam_from_hold": t.spam_from_hold,
        "ham": t.ham,
        // in the legacy spam set on the server's score alone, not learnt
        "spam_dropped": t.dropped,
        "spam_dropped_authenticated_list_mail": t.dropped_list_mail,
        "spam_dropped_by": t.dropped_by,
        "in_both_sets": t.in_both,
        // could not be resolved to raw mail, or to a direction
        "unresolved": t.no_owner + t.no_raw + t.in_both_unknown,
        "no_owner": t.no_owner,
        "no_raw": t.no_raw,
        "in_both_unknown": t.in_both_unknown,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev() -> Evidence {
        Evidence::default()
    }

    #[test]
    fn the_servers_own_verdict_is_not_learnt() {
        let e = Evidence {
            in_spam_set: true,
            ..ev()
        };
        assert_eq!(direction(e), Direction::Dropped);
    }

    #[test]
    fn a_reader_or_the_hold_makes_it_spam() {
        for e in [
            Evidence {
                in_spam_set: true,
                reader_junked: true,
                ..ev()
            },
            Evidence { held: true, ..ev() },
            Evidence {
                in_spam_set: true,
                marker: Some(true),
                ..ev()
            },
        ] {
            assert_eq!(direction(e), Direction::Spam, "{e:?}");
        }
    }

    #[test]
    fn the_readers_latest_not_junk_wins() {
        let e = Evidence {
            in_spam_set: true,
            held: true,
            marker: Some(false),
            ..ev()
        };
        assert_eq!(direction(e), Direction::Ham);
    }

    #[test]
    fn ham_stays_ham_and_both_without_a_marker_is_unknown() {
        let ham = Evidence {
            in_ham_set: true,
            ..ev()
        };
        assert_eq!(direction(ham), Direction::Ham);
        let both = Evidence {
            in_spam_set: true,
            in_ham_set: true,
            ..ev()
        };
        assert_eq!(direction(both), Direction::Unknown);
    }
}
