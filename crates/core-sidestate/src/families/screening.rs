//! The server's own observations behind the reputation and attachment
//! facts the screening rules judge, and the sets built from them.
//!
//! ```text
//! rep:obs            hash  id → mailrs_reputation::Observation (JSON)
//! rep:obs:seq        counter the ids are taken from
//! notify:reputation  channel: each new observation's id, for the re-judge
//! scan:hashobs       hash  id → mailrs_attachment_scan::HashObservation (JSON)
//! scan:hashobs:seq   counter
//! screening:rules    hash  report → RulesReport (JSON): the rule bundle the
//!                    receiver uses, overwritten each minute (a report)
//! ```
//!
//! Facts: added, never changed; a mistake is withdrawn by a `Retract`
//! observation. The sets are derived and live in each process's memory,
//! rebuilt when an observation is added and at least hourly, so the ages
//! they report stay current.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use mailrs_attachment_scan::{HashObservation, Inspector, Limits};
use mailrs_reputation::{NeverList, Observation, Set};

const OBS: &[u8] = b"rep:obs";
const OBS_SEQ: &[u8] = b"rep:obs:seq";
/// Where each new observation's id is published, so the mail it concerns
/// is judged again at once rather than at the next poll of the counter.
pub const REPUTATION_CHANNEL: &[u8] = b"notify:reputation";
const HASHES: &[u8] = b"scan:hashobs";
const HASHES_SEQ: &[u8] = b"scan:hashobs:seq";
const RULES: &[u8] = b"screening:rules";
/// Rebuilt at least this often, for the ages in the facts.
const REBUILD_SECS: i64 = 3600;

/// Record an observation; its id is assigned here. Returns the id.
///
/// # Errors
/// The store could not be written.
pub fn add_observation(
    conn: &mut kevy_client::Connection,
    mut o: Observation,
) -> std::io::Result<String> {
    o.id = format!("r{}", conn.incr(OBS_SEQ)?);
    let bytes = serde_json::to_vec(&o).map_err(std::io::Error::other)?;
    conn.hset(OBS, &[(o.id.as_bytes(), bytes.as_slice())])?;
    // the observation is stored; a lost announcement is caught by the
    // re-judge reading the counter
    if let Err(e) = conn.publish(REPUTATION_CHANNEL, o.id.as_bytes()) {
        tracing::warn!(error = %e, id = %o.id, "a new observation was not announced");
    }
    Ok(o.id)
}

/// The highest observation id handed out: `r<n>` for every `n` up to it.
///
/// # Errors
/// The store could not be read.
pub fn observation_seq(conn: &mut kevy_client::Connection) -> std::io::Result<u64> {
    Ok(conn
        .get(OBS_SEQ)?
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0))
}

/// One observation by its id.
///
/// # Errors
/// The store could not be read, or the stored row does not parse.
pub fn observation(
    conn: &mut kevy_client::Connection,
    id: &str,
) -> std::io::Result<Option<Observation>> {
    conn.hget(OBS, id.as_bytes())?
        .map(|v| serde_json::from_slice(&v).map_err(std::io::Error::other))
        .transpose()
}

/// Record a file-hash observation. Returns its id.
///
/// # Errors
/// The store could not be written.
pub fn add_hash_observation(
    conn: &mut kevy_client::Connection,
    mut o: HashObservation,
) -> std::io::Result<String> {
    o.id = format!("h{}", conn.incr(HASHES_SEQ)?);
    let bytes = serde_json::to_vec(&o).map_err(std::io::Error::other)?;
    conn.hset(HASHES, &[(o.id.as_bytes(), bytes.as_slice())])?;
    Ok(o.id)
}

fn all<T: serde::de::DeserializeOwned>(
    conn: &mut kevy_client::Connection,
    key: &[u8],
) -> std::io::Result<Vec<T>> {
    let flat = conn.hgetall(key)?;
    Ok(flat
        .chunks_exact(2)
        .filter_map(|pair| match serde_json::from_slice(&pair[1]) {
            Ok(o) => Some(o),
            Err(e) => {
                let id = String::from_utf8_lossy(&pair[0]);
                tracing::warn!(event = "screening_observation_unreadable", %id, error = %e, "an observation could not be read and is left out");
                None
            }
        })
        .collect())
}

fn junk_reason(thread_id: &str) -> String {
    format!("moved conversation {thread_id} to Junk")
}

/// The live `UserJunk` marks `user` made on `thread_id`: not retracted.
fn live_junk_marks<'o>(
    all: &'o [Observation],
    user: &str,
    thread_id: &str,
) -> Vec<&'o Observation> {
    let reason = junk_reason(thread_id);
    let retracted: std::collections::HashSet<&str> = all
        .iter()
        .filter(|o| o.stance == mailrs_reputation::Stance::Retract)
        .filter_map(|o| o.retracts.as_deref())
        .collect();
    all.iter()
        .filter(|o| {
            o.source == mailrs_reputation::Source::UserJunk
                && o.stance == mailrs_reputation::Stance::Deny
                && o.actor.as_deref() == Some(user)
                && o.reason == reason
                && !retracted.contains(o.id.as_str())
        })
        .collect()
}

/// `user` filed `thread_id` as Junk: one `UserJunk` mark per sender domain
/// and link host, unless that user already has a live mark on it from the
/// same conversation. Returns how many were added.
///
/// # Errors
/// The store could not be read or written.
pub fn record_user_junk(
    conn: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
    subjects: &mailrs_inbound::scan_input::JunkSubjects,
    now: i64,
) -> std::io::Result<usize> {
    use mailrs_reputation::{Kind, Source, Stance};
    let all = observations(conn)?;
    let have = live_junk_marks(&all, user, thread_id);
    let wanted = subjects
        .senders
        .iter()
        .map(|d| (Kind::SenderDomain, d))
        .chain(subjects.links.iter().map(|h| (Kind::LinkDomain, h)));
    let mut added = 0;
    for (kind, value) in wanted {
        if have.iter().any(|o| o.kind == kind && &o.value == value) {
            continue;
        }
        add_observation(
            conn,
            Observation {
                id: String::new(),
                at: now,
                source: Source::UserJunk,
                kind,
                value: value.clone(),
                stance: Stance::Deny,
                retracts: None,
                reason: junk_reason(thread_id),
                actor: Some(user.to_string()),
            },
        )?;
        added += 1;
    }
    Ok(added)
}

/// `user` took `thread_id` out of Junk: retract the marks that filing
/// made. Returns how many were retracted.
///
/// # Errors
/// The store could not be read or written.
pub fn withdraw_user_junk(
    conn: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
    now: i64,
) -> std::io::Result<usize> {
    use mailrs_reputation::{Source, Stance};
    let all = observations(conn)?;
    let marks: Vec<(String, mailrs_reputation::Kind, String)> =
        live_junk_marks(&all, user, thread_id)
            .into_iter()
            .map(|o| (o.id.clone(), o.kind, o.value.clone()))
            .collect();
    for (id, kind, value) in &marks {
        add_observation(
            conn,
            Observation {
                id: String::new(),
                at: now,
                source: Source::UserJunk,
                kind: *kind,
                value: value.clone(),
                stance: Stance::Retract,
                retracts: Some(id.clone()),
                reason: format!("took conversation {thread_id} out of Junk"),
                actor: Some(user.to_string()),
            },
        )?;
    }
    Ok(marks.len())
}

/// Record or withdraw `user`'s Junk verdict on `thread_id` as screening
/// observations; both cores call this where they train on the verdict. A
/// failure is logged, never returned: the filing has already happened.
pub fn learn_junk_verdict(
    conn: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
    raws: &[Vec<u8>],
    is_junk: bool,
    now: i64,
) {
    let result = match is_junk {
        true => {
            let subjects = mailrs_inbound::scan_input::junk_subjects(raws, user);
            record_user_junk(conn, user, thread_id, &subjects, now)
        }
        false => withdraw_user_junk(conn, user, thread_id, now),
    };
    if let Err(e) = result {
        tracing::warn!(event = "screening_failed", stage = "observe", error = %e, %user, %thread_id, "a Junk verdict was not recorded as an observation");
    }
}

/// What the judging process reports about its rules each minute.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RulesReport {
    /// `lua:<sha256>` of the bundle in use, with `;fallback-from=…` after
    /// a fallback.
    pub version: String,
    /// The bundle in use is one a replay report approved.
    pub approved: bool,
    /// Screening failures since the process started: a set not rebuilt,
    /// facts not extracted, rules not run.
    pub failures_total: u64,
    /// Unix seconds of the report.
    pub reported_at: i64,
}

/// Overwrite the judging process's report.
///
/// # Errors
/// The store could not be written.
pub fn record_rules_report(
    conn: &mut kevy_client::Connection,
    r: &RulesReport,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(r).map_err(std::io::Error::other)?;
    conn.hset(RULES, &[(b"report".as_slice(), bytes.as_slice())])?;
    Ok(())
}

/// The last report, if one was made.
///
/// # Errors
/// The store could not be read, or the report is not readable.
pub fn rules_report(conn: &mut kevy_client::Connection) -> std::io::Result<Option<RulesReport>> {
    let flat = conn.hgetall(RULES)?;
    flat.chunks_exact(2)
        .find(|p| p[0] == b"report")
        .map(|p| serde_json::from_slice(&p[1]).map_err(std::io::Error::other))
        .transpose()
}

/// Every observation, in no particular order.
///
/// # Errors
/// The store could not be read.
pub fn observations(conn: &mut kevy_client::Connection) -> std::io::Result<Vec<Observation>> {
    all(conn, OBS)
}

/// Every file-hash observation.
///
/// # Errors
/// The store could not be read.
pub fn hash_observations(
    conn: &mut kevy_client::Connection,
) -> std::io::Result<Vec<HashObservation>> {
    all(conn, HASHES)
}

fn version(conn: &mut kevy_client::Connection) -> std::io::Result<(i64, i64)> {
    let read = |c: &mut kevy_client::Connection, k: &[u8]| -> std::io::Result<i64> {
        Ok(c.get(k)?
            .and_then(|v| String::from_utf8(v).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    };
    Ok((read(conn, OBS_SEQ)?, read(conn, HASHES_SEQ)?))
}

/// The derived sets one process reads facts from, rebuilt when the
/// observations change.
pub struct Live {
    own_domains: Vec<String>,
    reputation: RwLock<Arc<Set>>,
    inspector: RwLock<Arc<Inspector>>,
    built: Mutex<Option<((i64, i64), i64)>>,
    failures: AtomicU64,
}

impl Live {
    /// Empty sets until the first `refresh`; `own_domains` never reach a
    /// threshold listing.
    pub fn new(own_domains: Vec<String>) -> Live {
        let never = NeverList::builtin(&own_domains);
        let reputation = Set::build(&[], &never, 0);
        let inspector = Inspector::build(&[], Limits::default(), 0);
        Live {
            own_domains,
            reputation: RwLock::new(Arc::new(reputation)),
            inspector: RwLock::new(Arc::new(inspector)),
            built: Mutex::new(None),
            failures: AtomicU64::new(0),
        }
    }

    /// Rebuild when an observation was added since the last build, or the
    /// last build is an hour old. Returns whether it rebuilt. A failure is
    /// counted and logged, and the previous sets stay in use.
    pub fn refresh(&self, conn: &mut kevy_client::Connection, now: i64) -> bool {
        match self.try_refresh(conn, now) {
            Ok(rebuilt) => rebuilt,
            Err(e) => {
                let total = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::warn!(event = "screening_failed", stage = "build", error = %e, failures_total = total, "the screening sets were not rebuilt; the last ones stay");
                false
            }
        }
    }

    fn try_refresh(&self, conn: &mut kevy_client::Connection, now: i64) -> std::io::Result<bool> {
        let v = version(conn)?;
        let last = *self.built.lock().expect("lock");
        if last.is_some_and(|(seen, at)| seen == v && now - at < REBUILD_SECS) {
            return Ok(false);
        }
        let never = NeverList::builtin(&self.own_domains);
        let reputation = Set::build(&observations(conn)?, &never, now);
        let inspector = Inspector::build(&hash_observations(conn)?, Limits::default(), now);
        *self.reputation.write().expect("lock") = Arc::new(reputation);
        *self.inspector.write().expect("lock") = Arc::new(inspector);
        *self.built.lock().expect("lock") = Some((v, now));
        Ok(true)
    }

    /// The reputation set in use.
    pub fn reputation(&self) -> Arc<Set> {
        self.reputation.read().expect("lock").clone()
    }

    /// The attachment inspector in use.
    pub fn inspector(&self) -> Arc<Inspector> {
        self.inspector.read().expect("lock").clone()
    }

    /// Count a failure outside a rebuild (a judgement that could not run).
    pub fn failed(&self, stage: &str, error: &str) -> u64 {
        let total = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::warn!(
            event = "screening_failed",
            stage,
            error,
            failures_total = total,
            "screening failed; the mail was let through"
        );
        total
    }

    /// Screening failures counted in this process.
    pub fn failures_total(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    /// What health shows.
    pub fn health(&self) -> serde_json::Value {
        let (r, s) = (self.reputation(), self.inspector());
        serde_json::json!({
            "reputation": {
                "computed_at": r.computed_at(),
                "observations": r.observations(),
                "never_list_sha256": NeverList::sha256(),
            },
            "attachment_scan": {
                "computed_at": self.built.lock().expect("lock").map_or(0, |(_, at)| at),
                "hashes": s.denied_hashes(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_reputation::{Kind, Message, Source, Stance};

    fn deny_ip(ip: &str) -> Observation {
        Observation {
            id: String::new(),
            at: 100,
            source: Source::Incident,
            kind: Kind::Ip,
            value: ip.into(),
            stance: Stance::Deny,
            retracts: None,
            reason: "seen in an incident".into(),
            actor: Some("admin@golia.jp".into()),
        }
    }

    #[test]
    fn an_added_fact_reaches_the_set_on_the_next_refresh() {
        let mut c = kevy_client::Connection::connect("mem://screening-live").unwrap();
        let live = Live::new(vec!["golia.jp".into()]);
        assert!(live.refresh(&mut c, 1_000));
        assert!(
            !live.refresh(&mut c, 1_001),
            "nothing changed, nothing rebuilt"
        );
        let id = add_observation(&mut c, deny_ip("200.181.82.134")).unwrap();
        assert_eq!(id, "r1");
        assert!(live.refresh(&mut c, 1_002));
        let m = Message {
            client_ip: Some("200.181.82.134".parse().unwrap()),
            ..Message::default()
        };
        assert_eq!(live.reputation().facts(&m).ip[0].id, "r1");
        assert!(
            live.refresh(&mut c, 1_002 + REBUILD_SECS),
            "an hour on, rebuilt so ages stay current"
        );
        assert_eq!(live.health()["reputation"]["observations"], 1);
    }

    #[test]
    fn the_rules_report_reads_back() {
        let mut c = kevy_client::Connection::connect("mem://screening-rules").unwrap();
        assert_eq!(rules_report(&mut c).unwrap(), None);
        let mut r = RulesReport {
            version: "lua:ab".into(),
            approved: false,
            failures_total: 0,
            reported_at: 5,
        };
        record_rules_report(&mut c, &r).unwrap();
        r.version = "lua:cd".into();
        r.approved = true;
        record_rules_report(&mut c, &r).unwrap();
        assert_eq!(rules_report(&mut c).unwrap(), Some(r));
    }

    #[test]
    fn junk_marks_are_added_once_and_withdrawn_with_the_conversation() {
        let mut c = kevy_client::Connection::connect("mem://screening-junk").unwrap();
        let subjects = mailrs_inbound::scan_input::JunkSubjects {
            senders: vec!["shop.example".into()],
            links: vec!["trk.example".into()],
        };
        assert_eq!(
            record_user_junk(&mut c, "qa@golia.jp", "t1", &subjects, 10).unwrap(),
            2
        );
        assert_eq!(
            record_user_junk(&mut c, "qa@golia.jp", "t1", &subjects, 11).unwrap(),
            0,
            "already marked"
        );
        assert_eq!(
            record_user_junk(&mut c, "ann@golia.jp", "t1", &subjects, 12).unwrap(),
            2,
            "another reader counts"
        );
        assert_eq!(
            withdraw_user_junk(&mut c, "qa@golia.jp", "t1", 13).unwrap(),
            2
        );
        assert_eq!(
            withdraw_user_junk(&mut c, "qa@golia.jp", "t1", 14).unwrap(),
            0
        );
        let set = Set::build(&observations(&mut c).unwrap(), &NeverList::builtin(&[]), 20);
        let m = mailrs_reputation::Message {
            header_domain: "shop.example".into(),
            ..mailrs_reputation::Message::default()
        };
        let marks = set.facts(&m).sender_marks.unwrap().marks;
        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].actor, "ann@golia.jp");
        assert_eq!(
            record_user_junk(&mut c, "qa@golia.jp", "t1", &subjects, 15).unwrap(),
            2,
            "filed again"
        );
    }
}
