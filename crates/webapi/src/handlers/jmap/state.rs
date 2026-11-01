//! State strings and what changed between two of them.
//!
//! A state names a snapshot of the caller's mail: per message its uid, a
//! 32-bit hash of its keywords and mailboxes, and its thread's key; per
//! mailbox a hash of its name and counts. `/changes` compares the snapshot
//! the client's state names with the one now, so it does not depend on
//! every writer of a flag, a label or the Trash recording that it wrote.
//!
//! Snapshots live in the network kevy, 16 bytes a message (30,000
//! messages: 480 KB), the last [`KEEP`] per user and none older than
//! [`TTL`]. A state that is no longer kept answers `cannotCalculateChanges`
//! and the client syncs from scratch, which RFC 8620 §5.2 allows.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use mailrs_core_api::method::message_index::fnv64;
use mailrs_jmap::error::JmapMethodError;
use serde_json::Value;

use super::view::View;

/// Snapshots kept per user: the one number that bounds what JMAP keeps in
/// the network kevy, at most `KEEP` × 16 bytes × messages per account.
pub const KEEP: usize = 32;
/// How long a snapshot is kept at most.
pub const TTL: Duration = Duration::from_secs(30 * 24 * 3600);

const MAGIC: &[u8; 4] = b"JSN1";

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `(uid, version, thread key)`, by uid.
    pub emails: Vec<(u32, u32, u64)>,
    /// `(mailbox id, version)`, by id.
    pub mailboxes: Vec<(String, u32)>,
}

fn version(parts: &[&[u8]]) -> u32 {
    let mut buf = Vec::new();
    for p in parts {
        buf.extend_from_slice(&(p.len() as u32).to_le_bytes());
        buf.extend_from_slice(p);
    }
    fnv64(&buf) as u32
}

impl Snapshot {
    /// The snapshot of `view`; `mailboxes` is every mailbox object.
    pub fn of(view: &View, mailboxes: &[(String, Value)]) -> Self {
        let emails = view
            .index
            .messages
            .iter()
            .map(|m| {
                let mut places: Vec<String> = view.places(m).iter().map(|p| p.id()).collect();
                places.sort();
                let places = places.join(",");
                let v = version(&[&m.flags.to_le_bytes(), places.as_bytes()]);
                (m.uid, v, view.thread_key(m))
            })
            .collect();
        let mut mailboxes: Vec<(String, u32)> = mailboxes
            .iter()
            .map(|(id, o)| (id.clone(), version(&[o.to_string().as_bytes()])))
            .collect();
        mailboxes.sort();
        Self { emails, mailboxes }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(12 + self.emails.len() * 16 + self.mailboxes.len() * 24);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&(self.emails.len() as u32).to_le_bytes());
        for (uid, v, t) in &self.emails {
            b.extend_from_slice(&uid.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&t.to_le_bytes());
        }
        b.extend_from_slice(&(self.mailboxes.len() as u32).to_le_bytes());
        for (id, v) in &self.mailboxes {
            b.push(id.len().min(255) as u8);
            b.extend_from_slice(&id.as_bytes()[..id.len().min(255)]);
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }

    pub fn decode(b: &[u8]) -> Option<Self> {
        let mut at = 0usize;
        let mut take = |n: usize| -> Option<&[u8]> {
            let s = b.get(at..at + n)?;
            at += n;
            Some(s)
        };
        if take(4)? != MAGIC {
            return None;
        }
        let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let mut emails = Vec::with_capacity(n.min(1 << 22));
        for _ in 0..n {
            let uid = u32::from_le_bytes(take(4)?.try_into().ok()?);
            let v = u32::from_le_bytes(take(4)?.try_into().ok()?);
            let t = u64::from_le_bytes(take(8)?.try_into().ok()?);
            emails.push((uid, v, t));
        }
        let n = u32::from_le_bytes(take(4)?.try_into().ok()?) as usize;
        let mut mailboxes = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            let len = take(1)?[0] as usize;
            let id = String::from_utf8(take(len)?.to_vec()).ok()?;
            let v = u32::from_le_bytes(take(4)?.try_into().ok()?);
            mailboxes.push((id, v));
        }
        Some(Self { emails, mailboxes })
    }

    /// The state string naming this snapshot.
    pub fn state(&self) -> String {
        format!("{:016x}", fnv64(&self.encode()))
    }

    /// Each thread with a hash of the messages in it.
    fn threads(&self) -> BTreeMap<u64, u32> {
        let mut members: BTreeMap<u64, Vec<u8>> = BTreeMap::new();
        for (uid, _, t) in &self.emails {
            members
                .entry(*t)
                .or_default()
                .extend_from_slice(&uid.to_le_bytes());
        }
        members
            .into_iter()
            .map(|(t, uids)| (t, version(&[&uids])))
            .collect()
    }
}

/// What changed for one type of object.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Diff<K> {
    pub created: Vec<K>,
    pub updated: Vec<K>,
    pub destroyed: Vec<K>,
}

impl<K> Diff<K> {
    pub fn len(&self) -> usize {
        self.created.len() + self.updated.len() + self.destroyed.len()
    }
}

fn diff<K: Ord + Clone>(old: &BTreeMap<K, u32>, new: &BTreeMap<K, u32>) -> Diff<K> {
    let mut d = Diff {
        created: Vec::new(),
        updated: Vec::new(),
        destroyed: Vec::new(),
    };
    for (k, v) in new {
        match old.get(k) {
            None => d.created.push(k.clone()),
            Some(o) if o != v => d.updated.push(k.clone()),
            Some(_) => {}
        }
    }
    d.destroyed = old
        .keys()
        .filter(|k| !new.contains_key(k))
        .cloned()
        .collect();
    d
}

pub fn emails(old: &Snapshot, new: &Snapshot) -> Diff<u32> {
    let map = |s: &Snapshot| s.emails.iter().map(|(u, v, _)| (*u, *v)).collect();
    diff(&map(old), &map(new))
}

pub fn threads(old: &Snapshot, new: &Snapshot) -> Diff<u64> {
    diff(&old.threads(), &new.threads())
}

pub fn mailboxes(old: &Snapshot, new: &Snapshot) -> Diff<String> {
    let map = |s: &Snapshot| s.mailboxes.iter().cloned().collect();
    diff(&map(old), &map(new))
}

/// Uids whose mailboxes or keywords changed, and those created: what a
/// query's results may have gained or lost.
pub fn touched(d: &Diff<u32>) -> BTreeSet<u32> {
    d.created
        .iter()
        .chain(&d.updated)
        .chain(&d.destroyed)
        .copied()
        .collect()
}

fn snapshot_key(user: &str, state: &str) -> String {
    format!("mailrs:jmap:snapshot:{user}:{state}")
}

/// The states kept for a user, newest first, newline separated.
fn order_key(user: &str) -> String {
    format!("mailrs:jmap:snapshots:{user}")
}

/// Keep `snap` under its state unless it already is, and let the oldest
/// beyond [`KEEP`] go.
pub fn remember(user: &str, state: &str, snap: &Snapshot) -> Result<(), JmapMethodError> {
    let (user, state, bytes) = (user.to_string(), state.to_string(), snap.encode());
    crate::handlers::kevy_util::with_kevy(move |c| {
        let order_key = order_key(&user);
        let held = c.get(order_key.as_bytes())?.unwrap_or_default();
        let mut states: Vec<String> = String::from_utf8_lossy(&held)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        if states.first() == Some(&state) {
            return Ok(());
        }
        states.retain(|s| *s != state);
        states.insert(0, state.clone());
        let gone: Vec<String> = states.split_off(states.len().min(KEEP));
        c.set_with_ttl(snapshot_key(&user, &state).as_bytes(), &bytes, TTL)?;
        c.set_with_ttl(order_key.as_bytes(), states.join("\n").as_bytes(), TTL)?;
        if !gone.is_empty() {
            let keys: Vec<String> = gone.iter().map(|s| snapshot_key(&user, s)).collect();
            let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_bytes()).collect();
            c.del(&refs)?;
        }
        Ok(())
    })
    .map_err(|_| super::ctx::fail("snapshot store unavailable"))
}

/// The snapshot a state names, if it is still kept.
pub fn recall(user: &str, state: &str) -> Result<Option<Snapshot>, JmapMethodError> {
    let key = snapshot_key(user, state);
    let bytes = crate::handlers::kevy_util::with_kevy(move |c| Ok(c.get(key.as_bytes())?))
        .map_err(|_| super::ctx::fail("snapshot store unavailable"))?;
    Ok(bytes.and_then(|b| Snapshot::decode(&b)))
}

/// The answer when the client's state is not kept any more.
pub fn cannot_calculate() -> JmapMethodError {
    JmapMethodError::Other {
        kind: "cannotCalculateChanges".into(),
        description: "that state is no longer kept; sync again".into(),
    }
}

/// The state a request reports: its snapshot, kept for later `/changes`.
pub struct Now {
    pub snapshot: Snapshot,
    pub state: String,
}

/// Snapshot `view` and keep it. In-process memory skips the store when
/// the state is the one last kept for this user.
pub fn now(user: &str, view: &View, mailboxes: &[(String, Value)]) -> Result<Now, JmapMethodError> {
    use std::sync::{Mutex, OnceLock};
    static LAST: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let snapshot = Snapshot::of(view, mailboxes);
    let state = snapshot.state();
    let last = LAST.get_or_init(|| Mutex::new(HashMap::new()));
    let known = last
        .lock()
        .map_err(|_| super::ctx::fail("state cache poisoned"))?
        .get(user)
        .is_some_and(|s| *s == state);
    if !known {
        remember(user, &state, &snapshot)?;
        last.lock()
            .map_err(|_| super::ctx::fail("state cache poisoned"))?
            .insert(user.to_string(), state.clone());
    }
    Ok(Now { snapshot, state })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(emails: &[(u32, u32, u64)], mailboxes: &[(&str, u32)]) -> Snapshot {
        Snapshot {
            emails: emails.to_vec(),
            mailboxes: mailboxes.iter().map(|(i, v)| (i.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn a_snapshot_round_trips_and_is_sixteen_bytes_a_message() {
        let s = snap(&[(1, 9, 77), (2, 8, 77)], &[("inbox", 3)]);
        assert_eq!(Snapshot::decode(&s.encode()), Some(s.clone()));
        let bigger = snap(&[(1, 9, 77), (2, 8, 77), (3, 1, 5)], &[("inbox", 3)]);
        assert_eq!(bigger.encode().len() - s.encode().len(), 16);
        assert_eq!(Snapshot::decode(b"JSN1\x01"), None);
        assert_ne!(s.state(), bigger.state());
    }

    #[test]
    fn changes_are_the_difference_of_two_snapshots() {
        let old = snap(
            &[(1, 9, 77), (2, 8, 77), (3, 1, 5)],
            &[("inbox", 3), ("L1", 1)],
        );
        let new = snap(
            &[(1, 9, 77), (2, 4, 77), (4, 1, 6)],
            &[("inbox", 4), ("L2", 1)],
        );
        let e = emails(&old, &new);
        assert_eq!(
            e,
            Diff {
                created: vec![4],
                updated: vec![2],
                destroyed: vec![3]
            }
        );
        let t = threads(&old, &new);
        assert_eq!(
            t,
            Diff {
                created: vec![6],
                updated: vec![],
                destroyed: vec![5]
            }
        );
        let m = mailboxes(&old, &new);
        assert_eq!(m.created, ["L2"]);
        assert_eq!(m.updated, ["inbox"]);
        assert_eq!(m.destroyed, ["L1"]);
        assert_eq!(emails(&new, &new).len(), 0);
        assert_eq!(touched(&e).into_iter().collect::<Vec<_>>(), [2, 3, 4]);
    }

    /// A RESP command to the kevy-server at `url`, its reply as text.
    fn resp(url: &str, args: &[&str]) -> String {
        use std::io::{Read, Write};
        let addr = url.trim_start_matches("kevy://");
        let mut s = std::net::TcpStream::connect(addr).expect("connect");
        s.set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("timeout");
        let mut cmd = format!("*{}\r\n", args.len());
        for a in args {
            cmd.push_str(&format!("${}\r\n{a}\r\n", a.len()));
        }
        s.write_all(cmd.as_bytes()).expect("write");
        let mut buf = vec![0u8; 64 * 1024];
        let n = s.read(&mut buf).unwrap_or(0);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }

    /// kevy-server's resident size, from the host's view of its process.
    fn server_rss_kib(container: &str) -> u64 {
        let out = std::process::Command::new("docker")
            .args(["inspect", "-f", "{{.State.Pid}}", container])
            .output()
            .expect("docker inspect");
        let pid = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("server status");
        status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse().ok())
            .expect("VmRSS")
    }

    /// What snapshots of a 30,000-message mailbox cost in a real
    /// kevy-server: one, then [`KEEP`] of them. A measurement, run by hand
    /// on a Linux host with docker:
    /// `cargo test -p mailrs-webapi --lib snapshots_in_kevy -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "a measurement, run by hand"]
    async fn snapshots_in_kevy() {
        const MESSAGES: u32 = 30_000;
        let (container, url) = mailrs_test_docker::kevy::kevy_server().await;
        // SAFETY: run alone, before anything opens the shared pool
        unsafe { std::env::set_var("MAILRS_KEVY_URL", &url) };
        let id = container.id().to_string();
        let mailboxes: Vec<(&str, u32)> = ["archive", "drafts", "inbox", "junk", "sent", "trash"]
            .iter()
            .map(|m| (*m, 7))
            .collect();
        let snapshot = move |salt: u32| {
            let emails: Vec<(u32, u32, u64)> = (1..=MESSAGES)
                .map(|uid| {
                    let v = fnv64(&(uid ^ salt).to_le_bytes()) as u32;
                    (uid, v, fnv64(&(uid % 20_000).to_le_bytes()))
                })
                .collect();
            snap(&emails, &mailboxes)
        };
        let one = snapshot(0);
        let encoded = one.encode().len();
        let url_c = url.clone();
        let (rss0, used) = tokio::task::spawn_blocking(move || {
            let rss0 = server_rss_kib(&id);
            remember("measure@x.test", &one.state(), &one).expect("kept");
            let key = snapshot_key("measure@x.test", &one.state());
            let used = resp(&url_c, &["MEMORY", "USAGE", &key]);
            (rss0, used)
        })
        .await
        .expect("join");
        let rss1 = server_rss_kib(container.id());
        println!(
            "one snapshot: {encoded} bytes encoded; MEMORY USAGE -> {used:?}; server RSS {rss0} -> {rss1} KiB"
        );
        let user = "measure@x.test".to_string();
        tokio::task::spawn_blocking(move || {
            for salt in 1..=KEEP as u32 {
                let s = snapshot(salt);
                remember(&user, &s.state(), &s).expect("kept");
            }
        })
        .await
        .expect("join");
        let rss2 = server_rss_kib(container.id());
        let info = resp(&url, &["INFO", "memory"]);
        println!(
            "{KEEP} kept: server RSS {rss1} -> {rss2} KiB (+{} KiB); INFO memory -> {info:?}",
            rss2.saturating_sub(rss1)
        );
    }
}
