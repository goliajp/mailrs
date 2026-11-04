//! Ham for the v2 spam/ham corpus, picked from the mail readers kept.
//!
//! The ham the legacy corpus was trained on is mostly the conversations
//! readers took out of Junk, so ordinary bulk mail they never had to
//! rescue, in Japanese especially, is nearly absent from it, and the
//! decoded-text tokenizer learnt "Japanese, or a bulk mailer's headers,
//! means spam". This adds such mail, chosen so that nothing a reader ever
//! treated as junk gets in:
//!
//! - in Inbox, Notifications or Promotions; not held, not in the Trash
//! - every message from someone else arrived at least [`MIN_AGE_SECS`] ago,
//!   and passed DMARC by the `Authentication-Results` this deployment
//!   wrote at the time
//! - no fraud verdict on any of its messages
//! - its sender's registrable domain has at least [`MIN_DOMAIN_THREADS`]
//!   of the reader's conversations outside Junk, none in Junk or held, is
//!   not on the reader's blacklist, was never marked as junk by the reader
//!   (a `UserJunk` sender-domain observation, retracted or not), and has
//!   no conversation of the reader's in either spam corpus set
//! - the reader replied: a message of their own in the same conversation
//!   after one from the sender, or, without one there, the contact record
//!   says they have sent to that sender's address (it keeps a count, not
//!   when)
//!
//! Then at most `per_domain` conversations per sender domain, and `total`
//! overall, in the order of a hash of user and thread id, so a page taken
//! with `skip` is the same slice on every call.

use std::collections::{BTreeSet, HashMap, HashSet};

use mailrs_triage::spam::{Corpus, keys};

use super::hangup::Hangup;
use super::prelude::*;

/// A message newer than this is not taken: a reader may not have judged it.
pub(super) const MIN_AGE_SECS: i64 = 14 * 86_400;
/// A sender the reader has fewer conversations with is not taken.
pub(super) const MIN_DOMAIN_THREADS: usize = 3;
/// At most this many conversations from one sender domain, by default.
pub(super) const DEFAULT_PER_DOMAIN: usize = 20;
/// At most this many conversations overall, by default.
pub(super) const DEFAULT_TOTAL: usize = 1000;

/// Why a conversation was not taken, counted at the first rule it fails.
#[derive(Default, serde::Serialize)]
pub(super) struct Rejected {
    held_or_trashed: u64,
    recent: u64,
    own_only: u64,
    fraud_verdict: u64,
    domain_marked_junk: u64,
    domain_in_junk_or_held: u64,
    domain_too_few: u64,
    no_reply: u64,
    dmarc_not_pass: u64,
    no_file: u64,
    per_domain_cap: u64,
    total_cap: u64,
}

/// One conversation taken, and the files of the messages others sent.
pub(super) struct Pick {
    pub(super) thread_id: String,
    pub(super) files: Vec<std::path::PathBuf>,
}

/// The conversations taken, how many were looked at, and why the rest
/// were not.
pub(super) struct Roster {
    pub(super) picks: Vec<Pick>,
    pub(super) candidates: u64,
    pub(super) rejected: Rejected,
}

pub(super) struct Message {
    pub(super) from: String,
    pub(super) arrived: i64,
    pub(super) message_id: String,
    pub(super) blob_ref: String,
}

/// When `user` wrote in `tid`, each of their own messages.
fn own_dates(state: &FastcoreState, user: &str, tid: &str) -> Vec<i64> {
    let user = user.to_ascii_lowercase();
    state
        .mailbox
        .list_thread_messages(&user, tid)
        .unwrap_or_default()
        .iter()
        .filter_map(|blob| serde_json::from_slice::<serde_json::Value>(blob).ok())
        .filter(|v| bare_address(v.get("sender").and_then(|x| x.as_str()).unwrap_or("")) == user)
        .map(|v| v.get("internal_date").and_then(|x| x.as_i64()).unwrap_or(0))
        .collect()
}

/// The reader answered the sender of `msgs`: their own message in the
/// conversation after the first one from the sender, or a contact record
/// of having sent to that address.
fn replied(
    state: &FastcoreState,
    conn: &mut kevy_client::Connection,
    user: &str,
    tid: &str,
    msgs: &[Message],
) -> bool {
    let Some(first) = msgs.iter().map(|m| m.arrived).min() else {
        return false;
    };
    own_dates(state, user, tid).iter().any(|d| *d > first)
        || msgs.iter().any(|m| {
            mailrs_core_sidestate::families::contacts::has_sent_to_addr(conn, user, &m.from)
        })
}

/// The messages of `tid` others sent to `user`.
pub(super) fn inbound(state: &FastcoreState, user: &str, tid: &str) -> Vec<Message> {
    let user = user.to_ascii_lowercase();
    let mut out = Vec::new();
    for blob in state
        .mailbox
        .list_thread_messages(&user, tid)
        .unwrap_or_default()
    {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&blob) else {
            continue;
        };
        let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let from = bare_address(&field("sender"));
        if from.is_empty() || from == user {
            continue;
        }
        out.push(Message {
            from,
            arrived: v.get("internal_date").and_then(|x| x.as_i64()).unwrap_or(0),
            message_id: field("message_id"),
            blob_ref: field("blob_ref"),
        });
    }
    out
}

/// `Name <a@b>` or `a@b`, lowercased.
fn bare_address(sender: &str) -> String {
    let s = match (sender.rfind('<'), sender.rfind('>')) {
        (Some(a), Some(b)) if a < b => &sender[a + 1..b],
        _ => sender,
    };
    s.trim().to_ascii_lowercase()
}

pub(super) fn registrable_of(address: &str) -> String {
    let host = address.rsplit_once('@').map_or(address, |(_, d)| d);
    mailrs_fraud::brand::registrable(host)
}

/// The sender domain of a conversation: that of its first message from
/// someone else.
fn domain_of(msgs: &[Message]) -> Option<String> {
    msgs.first().map(|m| registrable_of(&m.from))
}

/// The DMARC result in the `Authentication-Results` header `stamp` wrote.
fn our_dmarc(file: &std::path::Path, stamp: &str) -> Option<String> {
    use std::io::Read;
    let mut head = vec![0u8; 64 * 1024];
    let n = std::fs::File::open(file).ok()?.read(&mut head).ok()?;
    head.truncate(n);
    let text = String::from_utf8_lossy(&head);
    let mut value: Option<String> = None;
    let mut ours = false;
    for line in text.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(v) = value.as_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if ours {
            break;
        }
        value = None;
        if let Some(rest) = line
            .to_ascii_lowercase()
            .strip_prefix("authentication-results:")
        {
            let v = line[line.len() - rest.len()..].trim().to_string();
            ours = v
                .to_ascii_lowercase()
                .starts_with(&stamp.to_ascii_lowercase());
            if ours {
                value = Some(v);
            }
        }
    }
    let value = value.filter(|_| ours)?;
    mailrs_inbound::auth_header::parse_auth_results(&value)
        .into_iter()
        .find(|r| r.method == "dmarc")
        .map(|r| r.result)
}

/// The sender domains `user` treated as junk: their own junk marks, their
/// blacklist, and their conversations in either spam corpus set.
fn junk_domains(
    state: &FastcoreState,
    conn: &mut kevy_client::Connection,
    user: &str,
) -> std::io::Result<HashSet<String>> {
    use mailrs_reputation::{Kind, Source};
    let mut out = HashSet::new();
    for o in mailrs_core_sidestate::families::screening::observations(conn)? {
        if o.source == Source::UserJunk
            && o.kind == Kind::SenderDomain
            && o.actor
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(user))
        {
            out.insert(mailrs_fraud::brand::registrable(
                &o.value.to_ascii_lowercase(),
            ));
        }
    }
    let black = mailrs_core_sidestate::families::sender_lists::blacklist_key(user);
    for m in conn.smembers(black.as_bytes())? {
        out.insert(registrable_of(
            &String::from_utf8_lossy(&m).trim().to_ascii_lowercase(),
        ));
    }
    for corpus in Corpus::ALL {
        for tid in conn.smembers(keys(corpus).corpus_spam)? {
            let tid = String::from_utf8_lossy(&tid);
            if let Some(d) = domain_of(&inbound(state, user, &tid)) {
                out.insert(d);
            }
        }
    }
    Ok(out)
}

pub(super) fn hash_of(user: &str, tid: &str) -> u64 {
    user.bytes()
        .chain([0u8])
        .chain(tid.bytes())
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

struct Candidate {
    hash: u64,
    domain: String,
    pick: Pick,
}

/// Every account's conversations, through the rules above, then the caps.
pub(super) fn roster(
    state: &Arc<FastcoreState>,
    conn: &mut kevy_client::Connection,
    per_domain: usize,
    total: usize,
    now: i64,
    hangup: &Hangup,
) -> std::io::Result<Roster> {
    let mut rejected = Rejected::default();
    let mut candidates = 0u64;
    let mut kept: Vec<Candidate> = Vec::new();
    let Some(stamp) = state.authserv_id.clone() else {
        return Err(std::io::Error::other(
            "MAILRS_HOSTNAME is unset: whose Authentication-Results to read is unknown",
        ));
    };
    for user in state.mailbox.list_account_addresses()? {
        if hangup.hung_up() {
            break;
        }
        let bad = junk_domains(state, conn, &user)?;
        let list = |bucket: &str| {
            state.mailbox.list_thread_ids_by_bucket_unsent_via_table(
                &user,
                bucket,
                mailrs_mailbox_kevy::ArchiveScope::All,
                usize::MAX,
            )
        };
        let mut kept_out: BTreeSet<String> = BTreeSet::new();
        for b in ["inbox", "notifications", "promotions"] {
            kept_out.extend(list(b)?);
        }
        // a domain with a conversation in Junk or held is out
        let mut tainted: HashSet<String> = HashSet::new();
        for tid in list("junk")? {
            if let Some(d) = domain_of(&inbound(state, &user, &tid)) {
                tainted.insert(d);
            }
        }
        let mut threads: Vec<(String, Vec<Message>, Option<String>)> = Vec::new();
        let mut per_dom: HashMap<String, usize> = HashMap::new();
        for tid in &kept_out {
            let msgs = inbound(state, &user, tid);
            let dom = domain_of(&msgs);
            let row = state.mailbox.get_thread_for_user(&user, tid)?;
            let held = row.as_ref().is_some_and(|r| r.quarantined);
            if let Some(d) = &dom {
                *per_dom.entry(d.clone()).or_default() += 1;
                if held {
                    tainted.insert(d.clone());
                }
            }
            if row
                .as_ref()
                .is_some_and(|r| r.quarantined || r.trashed_at > 0)
            {
                candidates += 1;
                rejected.held_or_trashed += 1;
                continue;
            }
            threads.push((tid.clone(), msgs, dom));
        }
        let mut files: Option<HashMap<String, std::path::PathBuf>> = None;
        for (tid, msgs, dom) in threads {
            candidates += 1;
            let Some(dom) = dom else {
                rejected.own_only += 1;
                continue;
            };
            if msgs.iter().any(|m| m.arrived > now - MIN_AGE_SECS) {
                rejected.recent += 1;
                continue;
            }
            let fraud = msgs.iter().any(|m| {
                !m.message_id.is_empty()
                    && state
                        .mailbox
                        .fraud_verdict(&m.message_id)
                        .ok()
                        .flatten()
                        .is_some()
            });
            if fraud {
                rejected.fraud_verdict += 1;
                continue;
            }
            if bad.contains(&dom) || msgs.iter().any(|m| bad.contains(&registrable_of(&m.from))) {
                rejected.domain_marked_junk += 1;
                continue;
            }
            if tainted.contains(&dom) {
                rejected.domain_in_junk_or_held += 1;
                continue;
            }
            if per_dom.get(&dom).copied().unwrap_or(0) < MIN_DOMAIN_THREADS {
                rejected.domain_too_few += 1;
                continue;
            }
            if !replied(state, conn, &user, &tid, &msgs) {
                rejected.no_reply += 1;
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
            if !paths
                .iter()
                .all(|p| our_dmarc(p, &stamp).as_deref() == Some("pass"))
            {
                rejected.dmarc_not_pass += 1;
                continue;
            }
            kept.push(Candidate {
                hash: hash_of(&user, &tid),
                domain: dom,
                pick: Pick {
                    thread_id: tid,
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
        if *n >= per_domain {
            rejected.per_domain_cap += 1;
            continue;
        }
        if picks.len() >= total {
            rejected.total_cap += 1;
            continue;
        }
        *n += 1;
        picks.push(c.pick);
    }
    Ok(Roster {
        picks,
        candidates,
        rejected,
    })
}

/// Train the page `skip..skip + limit` of the roster into the v2 corpus as
/// ham, or on a dry run only count it.
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
    let roster = roster(state, conn, caps.0, caps.1, now, hangup)?;
    let mut walked = 0usize;
    let mut unreadable = 0u64;
    for pick in roster.picks.iter().skip(skip).take(limit) {
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
            mailrs_triage::spam::train_in(conn, Corpus::V2, &pick.thread_id, &raws, false);
        }
    }
    let next_skip = skip + walked;
    tracing::info!(
        walked,
        kept = roster.picks.len(),
        candidates = roster.candidates,
        dry_run,
        "bayes-rebuild-v2 curated ham"
    );
    Ok(serde_json::json!({
        "source": "curated",
        "dry_run": dry_run,
        "per_domain_cap": caps.0,
        "total_cap": caps.1,
        "candidates": roster.candidates,
        "kept": roster.picks.len(),
        "rejected": roster.rejected,
        "skip": skip,
        "walked": walked,
        "next_skip": next_skip,
        "done": next_skip >= roster.picks.len(),
        // a file listed a moment ago and gone now
        "unreadable": unreadable,
    }))
}
