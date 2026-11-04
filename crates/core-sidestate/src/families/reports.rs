//! Scam reports. A report is a fact — who reported which message, when —
//! written once and never changed. What came of it (the other copies taken
//! back, or waiting for an administrator) is derived and kept apart, so it
//! can be decided again without touching the report.
//!
//! ```text
//! mailrs:report:{id}           hash    the report, written once
//! mailrs:reports               list    ids, newest first, kept KEPT deep
//! mailrs:report-outcome:{id}   string  JSON Outcome, overwritten
//! mailrs:reports-pending       hash    id → reported-at, while it waits
//! ```
//!
//! Both cores write here; the web tier reads outcomes through the core.

use std::io;

use serde::{Deserialize, Serialize};

/// Reports kept in the newest-first list; older ones keep their fact and
/// outcome but stop counting towards a second reporter.
pub const KEPT: i64 = 5_000;

pub const LIST: &str = "mailrs:reports";

/// The `fraud_hits` counter of reports whose message was too short to
/// fingerprint, and so could not look for other copies; `/api/health`
/// shows it beside the rules' hits.
pub const NO_FINGERPRINT: &str = "report-no-fingerprint";
pub const PENDING: &str = "mailrs:reports-pending";

pub fn key(id: &str) -> String {
    format!("mailrs:report:{id}")
}

pub fn outcome_key(id: &str) -> String {
    format!("mailrs:report-outcome:{id}")
}

/// One report, as made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub id: String,
    /// The reporting account.
    pub reporter: String,
    /// Whom the reporter's mail reaches: their own address, or the local
    /// account their mail is forwarded to, so one person counts once.
    pub person: String,
    pub message_id: String,
    pub thread_id: String,
    /// Unix seconds.
    pub at: i64,
    /// The message's fingerprint, 16 hex digits; empty when it had none.
    pub simhash: String,
    pub sender_domain: String,
}

/// What came of a report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// `taken_back`, `none_found`, `waiting` or `dismissed`.
    pub state: String,
    pub taken_back: u64,
    pub inboxes: u64,
    pub grounds: Vec<String>,
    pub waits: Vec<String>,
    /// 0 while it waits.
    pub decided_at: i64,
    /// The other copies, as `user\tthread\tmessage`, for an administrator
    /// to confirm against.
    pub targets: Vec<String>,
    /// The reporter's other conversations holding the template, held with
    /// the report.
    #[serde(default)]
    pub own_held: u64,
    /// The reported message was too short to fingerprint, so no other copy
    /// could be looked for.
    #[serde(default)]
    pub no_fingerprint: bool,
    /// The hold rule of the rules in force that held the reported message
    /// when it was reported (the `hold_rule` ground); `None` when none did.
    #[serde(default)]
    pub hold_rule: Option<String>,
    /// The version of the rule set `hold_rule` is in.
    #[serde(default)]
    pub rules_version: Option<String>,
}

/// A report and what came of it, as both cores answer it.
pub fn to_wire(r: &Report, o: &Outcome) -> mailrs_core_api::method::report::ReportOutcomeWire {
    use mailrs_core_api::method::report::{ReportOutcomeWire, ReportState};
    let state = match o.state.as_str() {
        "taken_back" => ReportState::TakenBack,
        "waiting" => ReportState::Waiting,
        "dismissed" => ReportState::Dismissed,
        _ => ReportState::NoneFound,
    };
    ReportOutcomeWire {
        report_id: r.id.clone(),
        message_id: r.message_id.clone(),
        thread_id: r.thread_id.clone(),
        state,
        taken_back: o.taken_back,
        inboxes: o.inboxes,
        grounds: o.grounds.clone(),
        waits: o.waits.clone(),
        at: r.at,
        decided_at: o.decided_at,
        own_held: o.own_held,
        no_fingerprint: o.no_fingerprint,
        hold_rule: o.hold_rule.clone(),
        rules_version: o.rules_version.clone(),
    }
}

/// A waiting report as an administrator sees it: the reported message's
/// subject and sender, and `(user, subject, from)` of some of the copies.
pub fn pending_to_wire(
    r: &Report,
    o: &Outcome,
    subject: String,
    from: String,
    samples: Vec<(String, String, String)>,
) -> mailrs_core_api::method::report::PendingReportWire {
    use mailrs_core_api::method::report::{PendingReportWire, ReportSampleWire};
    PendingReportWire {
        outcome: to_wire(r, o),
        reporter: r.reporter.clone(),
        sender_domain: r.sender_domain.clone(),
        subject,
        from,
        samples: samples
            .into_iter()
            .map(|(user, subject, from)| ReportSampleWire {
                user,
                subject,
                from,
            })
            .collect(),
    }
}

/// A fresh random id, `rp` and 16 hex digits.
pub fn new_id() -> String {
    let mut bytes = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("rp{hex}")
}

const FIELDS: [&str; 8] = [
    "id",
    "reporter",
    "person",
    "message_id",
    "thread_id",
    "at",
    "simhash",
    "sender_domain",
];

/// Records a report. A second write under the same id is refused: a
/// report is not edited.
///
/// # Errors
/// The id is taken, or the store could not be written.
pub fn append(conn: &mut kevy_client::Connection, r: &Report) -> io::Result<()> {
    let k = key(&r.id);
    if conn.exists(&[k.as_bytes()])? > 0 {
        return Err(io::Error::other(format!("report {} already exists", r.id)));
    }
    let at = r.at.to_string();
    let values = [
        r.id.as_str(),
        r.reporter.as_str(),
        r.person.as_str(),
        r.message_id.as_str(),
        r.thread_id.as_str(),
        at.as_str(),
        r.simhash.as_str(),
        r.sender_domain.as_str(),
    ];
    let pairs: Vec<(&[u8], &[u8])> = FIELDS
        .iter()
        .zip(values.iter())
        .map(|(f, v)| (f.as_bytes(), v.as_bytes()))
        .collect();
    conn.hset(k.as_bytes(), &pairs)?;
    let len = conn.lpush(LIST.as_bytes(), &[r.id.as_bytes()])? as i64;
    if len > KEPT {
        conn.rpop(LIST.as_bytes(), (len - KEPT) as usize)?;
    }
    Ok(())
}

/// One report by id.
///
/// # Errors
/// The store could not be read.
pub fn get(conn: &mut kevy_client::Connection, id: &str) -> io::Result<Option<Report>> {
    let flat = conn.hgetall(key(id).as_bytes())?;
    if flat.is_empty() {
        return Ok(None);
    }
    let mut r = Report::default();
    for pair in flat.chunks(2) {
        let [f, v] = pair else { continue };
        let v = String::from_utf8_lossy(v).into_owned();
        match f.as_slice() {
            b"id" => r.id = v,
            b"reporter" => r.reporter = v,
            b"person" => r.person = v,
            b"message_id" => r.message_id = v,
            b"thread_id" => r.thread_id = v,
            b"at" => r.at = v.parse().unwrap_or(0),
            b"simhash" => r.simhash = v,
            b"sender_domain" => r.sender_domain = v,
            _ => {}
        }
    }
    Ok(Some(r))
}

/// Reports made at `since` or later, newest first, from the kept list.
///
/// # Errors
/// The store could not be read.
pub fn since(conn: &mut kevy_client::Connection, since: i64) -> io::Result<Vec<Report>> {
    let ids = conn.lrange(LIST.as_bytes(), 0, KEPT - 1)?;
    let mut out = Vec::new();
    for id in ids {
        let id = String::from_utf8_lossy(&id).into_owned();
        let Some(r) = get(conn, &id)? else { continue };
        if r.at < since {
            break;
        }
        out.push(r);
    }
    Ok(out)
}

/// Replaces what came of a report, and keeps the waiting set in step.
///
/// # Errors
/// The store could not be written.
pub fn set_outcome(conn: &mut kevy_client::Connection, r: &Report, o: &Outcome) -> io::Result<()> {
    let json = serde_json::to_vec(o).map_err(io::Error::other)?;
    conn.set(outcome_key(&r.id).as_bytes(), &json)?;
    match o.state.as_str() {
        "waiting" => {
            conn.hset(
                PENDING.as_bytes(),
                &[(r.id.as_bytes(), r.at.to_string().as_bytes())],
            )?;
        }
        _ => {
            conn.hdel(PENDING.as_bytes(), &[r.id.as_bytes()])?;
        }
    }
    Ok(())
}

/// What came of a report, if anything yet.
///
/// # Errors
/// The store could not be read, or holds something unreadable.
pub fn outcome(conn: &mut kevy_client::Connection, id: &str) -> io::Result<Option<Outcome>> {
    match conn.get(outcome_key(id).as_bytes())? {
        Some(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(io::Error::other),
        None => Ok(None),
    }
}

/// The ids waiting for an administrator, oldest first.
///
/// # Errors
/// The store could not be read.
pub fn pending(conn: &mut kevy_client::Connection) -> io::Result<Vec<String>> {
    let flat = conn.hgetall(PENDING.as_bytes())?;
    let mut v: Vec<(i64, String)> = flat
        .chunks(2)
        .filter_map(|p| match p {
            [id, at] => Some((
                String::from_utf8_lossy(at).parse().unwrap_or(0),
                String::from_utf8_lossy(id).into_owned(),
            )),
            _ => None,
        })
        .collect();
    v.sort();
    Ok(v.into_iter().map(|(_, id)| id).collect())
}

/// Whom `account`'s mail reaches: the one local account it is forwarded
/// to, else itself. Two reports from an account and the account it
/// forwards to are one person's.
///
/// # Errors
/// The store could not be read.
pub fn person_of(conn: &mut kevy_client::Connection, account: &str) -> io::Result<String> {
    let me = account.trim().to_ascii_lowercase();
    let targets = super::local_forward::targets(conn, &me)?;
    match targets.as_slice() {
        [only] => Ok(only.clone()),
        _ => Ok(me),
    }
}

/// Whether the operator's allow list (the white entries of the admin's
/// local sender lists) names `address` or its domain or a parent of it.
///
/// # Errors
/// The store could not be read.
pub fn allow_listed(conn: &mut kevy_client::Connection, address: &str) -> io::Result<bool> {
    let flat = conn.hgetall(crate::keys::admin::GREYLIST_LOCAL_LISTS)?;
    let address = address.trim().to_ascii_lowercase();
    let domain = address.rsplit_once('@').map(|(_, d)| d).unwrap_or(&address);
    Ok(flat
        .chunks(2)
        .filter_map(|p| match p {
            [_, v] => serde_json::from_slice::<serde_json::Value>(v).ok(),
            _ => None,
        })
        .any(|e| {
            let field = |a: &str, b: &str| {
                e.get(a)
                    .or_else(|| e.get(b))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase()
            };
            let white = matches!(field("list_type", "list").as_str(), "whitelist" | "white");
            let v = field("address_or_domain", "value");
            white && !v.is_empty() && (v == address || under(domain, &v))
        }))
}

fn under(domain: &str, listed: &str) -> bool {
    domain == listed || domain.ends_with(&format!(".{listed}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listed_domain_covers_its_subdomains_and_nothing_else() {
        assert!(under("mail.bank.example", "bank.example"));
        assert!(under("bank.example", "bank.example"));
        assert!(!under("evilbank.example", "bank.example"));
    }

    #[test]
    fn ids_are_distinct_and_shaped() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert!(a.starts_with("rp") && a.len() == 18, "{a}");
    }

    #[test]
    fn an_outcome_reads_back_from_its_json() {
        let o = Outcome {
            state: "waiting".into(),
            inboxes: 3,
            waits: vec!["no_ground".into()],
            targets: vec!["a@x\tt1\tm1".into()],
            ..Default::default()
        };
        let back: Outcome = serde_json::from_slice(&serde_json::to_vec(&o).unwrap()).unwrap();
        assert_eq!(back, o);
    }
}
