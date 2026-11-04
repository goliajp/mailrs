//! Spam for the v2 spam/ham corpus, from what readers filed as junk.
//!
//! The per-conversation fact is the `UserJunk` observation a reader's
//! "move to Junk" leaves (`moved conversation <tid> to Junk`, one per
//! sender domain and link host); taking it back out retracts those marks.
//! A conversation is taken when:
//!
//! - the reader has a mark on it and none of their marks on it was ever
//!   retracted, and it is in their Junk now
//! - it has a message from someone else, and every such message arrived at
//!   least [`MIN_AGE_SECS`](super::curated_ham::MIN_AGE_SECS) ago
//! - it is not already in a spam corpus set (those are learnt by the
//!   legacy-set rebuild)
//!
//! then at most `per_domain` per sender domain and `total` overall, in the
//! order of a hash of user and thread id.

use std::collections::{BTreeSet, HashMap, HashSet};

use mailrs_reputation::{Source, Stance};
use mailrs_triage::spam::{Corpus, keys};

use super::curated_ham::{MIN_AGE_SECS, Pick, hash_of, inbound, registrable_of};
use super::hangup::Hangup;
use super::prelude::*;

/// Why a marked conversation was not taken, at the first rule it fails.
#[derive(Default, serde::Serialize)]
pub(super) struct Rejected {
    own_only: u64,
    released: u64,
    not_in_junk_now: u64,
    recent: u64,
    already_in_corpus: u64,
    no_file: u64,
    per_domain_cap: u64,
    total_cap: u64,
}

/// The conversation a `UserJunk` reason names.
fn marked_thread(reason: &str) -> Option<&str> {
    reason
        .strip_prefix("moved conversation ")?
        .strip_suffix(" to Junk")
}

/// Each reader's marked conversations, and those with a mark retracted.
type Marks = HashMap<String, (BTreeSet<String>, HashSet<String>)>;

fn marks(conn: &mut kevy_client::Connection) -> std::io::Result<Marks> {
    let all = mailrs_core_sidestate::families::screening::observations(conn)?;
    let retracted: HashSet<&str> = all
        .iter()
        .filter(|o| o.stance == Stance::Retract)
        .filter_map(|o| o.retracts.as_deref())
        .collect();
    let mut out: Marks = HashMap::new();
    for o in all
        .iter()
        .filter(|o| o.source == Source::UserJunk && o.stance == Stance::Deny)
    {
        let (Some(user), Some(tid)) = (o.actor.as_deref(), marked_thread(&o.reason)) else {
            continue;
        };
        let e = out.entry(user.to_ascii_lowercase()).or_default();
        e.0.insert(tid.to_string());
        if retracted.contains(o.id.as_str()) {
            e.1.insert(tid.to_string());
        }
    }
    Ok(out)
}

struct Candidate {
    hash: u64,
    domain: String,
    pick: Pick,
}

/// The conversations taken, how many were looked at, and why the rest
/// were not.
fn roster(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    caps: (usize, usize),
    now: i64,
    hangup: &Hangup,
) -> std::io::Result<(Vec<Pick>, u64, Rejected)> {
    let mut rejected = Rejected::default();
    let mut candidates = 0u64;
    let mut kept: Vec<Candidate> = Vec::new();
    let mut in_corpus: HashSet<String> = HashSet::new();
    for corpus in Corpus::ALL {
        for t in conn.smembers(keys(corpus).corpus_spam)? {
            in_corpus.insert(String::from_utf8_lossy(&t).into_owned());
        }
    }
    let marks = marks(conn)?;
    for user in state.mailbox.list_account_addresses()? {
        if hangup.hung_up() {
            break;
        }
        let Some((marked, released)) = marks.get(&user.to_ascii_lowercase()) else {
            continue;
        };
        let junk: HashSet<String> = state
            .mailbox
            .list_thread_ids_by_bucket_unsent_via_table(
                &user,
                "junk",
                mailrs_mailbox_kevy::ArchiveScope::All,
                usize::MAX,
            )?
            .into_iter()
            .collect();
        let mut files: Option<HashMap<String, std::path::PathBuf>> = None;
        for tid in marked {
            if state.mailbox.get_thread_for_user(&user, tid)?.is_none() {
                continue;
            }
            candidates += 1;
            let msgs = inbound(state, &user, tid);
            let Some(first) = msgs.first() else {
                rejected.own_only += 1;
                continue;
            };
            if released.contains(tid) {
                rejected.released += 1;
                continue;
            }
            if !junk.contains(tid) {
                rejected.not_in_junk_now += 1;
                continue;
            }
            if msgs.iter().any(|m| m.arrived > now - MIN_AGE_SECS) {
                rejected.recent += 1;
                continue;
            }
            if in_corpus.contains(tid) {
                rejected.already_in_corpus += 1;
                continue;
            }
            let on_disk =
                files.get_or_insert_with(|| crate::maildir_scan::paths_on_disk(state, &user));
            let paths: Option<Vec<std::path::PathBuf>> = msgs
                .iter()
                .map(|m| {
                    on_disk
                        .get(crate::maildir_scan::base_id(&m.blob_ref))
                        .cloned()
                })
                .collect();
            let Some(paths) = paths else {
                rejected.no_file += 1;
                continue;
            };
            kept.push(Candidate {
                hash: hash_of(&user, tid),
                domain: registrable_of(&first.from),
                pick: Pick {
                    thread_id: tid.clone(),
                    files: paths,
                },
            });
        }
    }
    kept.sort_by_key(|c| c.hash);
    let mut taken: HashMap<String, usize> = HashMap::new();
    let mut picks = Vec::new();
    for c in kept {
        let n = taken.entry(c.domain).or_default();
        if *n >= caps.0 {
            rejected.per_domain_cap += 1;
            continue;
        }
        if picks.len() >= caps.1 {
            rejected.total_cap += 1;
            continue;
        }
        *n += 1;
        picks.push(c.pick);
    }
    Ok((picks, candidates, rejected))
}

/// Train the page `skip..skip + limit` of the roster into the v2 corpus as
/// spam, or on a dry run only count it.
pub(super) fn rebuild(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    page: (usize, usize),
    caps: (usize, usize),
    dry_run: bool,
    hangup: &Hangup,
) -> std::io::Result<serde_json::Value> {
    let (skip, limit) = page;
    let now = chrono::Utc::now().timestamp();
    let (picks, candidates, rejected) = roster(state, conn, caps, now, hangup)?;
    let mut walked = 0usize;
    let mut unreadable = 0u64;
    for pick in picks.iter().skip(skip).take(limit) {
        if hangup.hung_up() {
            break;
        }
        walked += 1;
        let raws: Vec<Vec<u8>> = pick
            .files
            .iter()
            .filter_map(|p| std::fs::read(p).ok())
            .collect();
        if raws.len() != pick.files.len() {
            unreadable += 1;
            continue;
        }
        if !dry_run {
            mailrs_triage::spam::train_in(conn, Corpus::V2, &pick.thread_id, &raws, true);
        }
    }
    let next_skip = skip + walked;
    tracing::info!(
        walked,
        kept = picks.len(),
        candidates,
        dry_run,
        "bayes-rebuild-v2 curated spam"
    );
    Ok(serde_json::json!({
        "source": "curated_spam",
        "dry_run": dry_run,
        "per_domain_cap": caps.0,
        "total_cap": caps.1,
        "candidates": candidates,
        "kept": picks.len(),
        "rejected": rejected,
        "skip": skip,
        "walked": walked,
        "next_skip": next_skip,
        "done": next_skip >= picks.len(),
        "unreadable": unreadable,
    }))
}
