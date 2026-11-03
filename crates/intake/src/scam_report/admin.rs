//! An administrator deciding on the copies a report waits on: they see the
//! reported message, how many inboxes hold it and from which domain, and
//! up to ten of the copies, then take them back or leave them.

use mailrs_core_sidestate::families::reports;

use super::{ReportStore, Target, notify};

const SAMPLES: usize = 10;

/// One waiting report, as an administrator sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub report: reports::Report,
    pub outcome: reports::Outcome,
    pub subject: String,
    pub from: String,
    /// `(user, subject, from)` of up to ten of the other copies.
    pub samples: Vec<(String, String, String)>,
}

fn headers<S: ReportStore + ?Sized>(core: &S, user: &str, mid: &str) -> (String, String) {
    let raw = core.raw_of(user, mid).unwrap_or_default();
    (
        mailrs_inbound::subject_header(&raw),
        mailrs_inbound::from_header(&raw),
    )
}

type Found = Result<Option<(reports::Report, reports::Outcome)>, String>;

/// Every report waiting, oldest first.
pub fn pending<S: ReportStore + ?Sized>(core: &S) -> Result<Vec<Pending>, String> {
    let mut conn = core.net_conn().ok_or("the shared store is unreachable")?;
    let mut out = Vec::new();
    for id in reports::pending(&mut conn).map_err(|e| e.to_string())? {
        let (Some(report), Some(outcome)) = (
            reports::get(&mut conn, &id).map_err(|e| e.to_string())?,
            reports::outcome(&mut conn, &id).map_err(|e| e.to_string())?,
        ) else {
            continue;
        };
        let (subject, from) = headers(core, &report.reporter, &report.message_id);
        let samples = outcome
            .targets
            .iter()
            .filter_map(|t| Target::decode(t))
            .take(SAMPLES)
            .map(|t| {
                let (subject, from) = headers(core, &t.user, &t.mid);
                (t.user, subject, from)
            })
            .collect();
        out.push(Pending {
            report,
            outcome,
            subject,
            from,
            samples,
        });
    }
    Ok(out)
}

fn waiting(conn: &mut kevy_client::Connection, id: &str) -> Found {
    let report = reports::get(conn, id).map_err(|e| e.to_string())?;
    let outcome = reports::outcome(conn, id).map_err(|e| e.to_string())?;
    Ok(report.zip(outcome))
}

/// Take a waiting report's copies back. `None` when there is no such
/// report; one not waiting is returned as it stands.
pub fn confirm<S: ReportStore + ?Sized>(core: &S, id: &str, actor: &str) -> Found {
    let mut conn = core.net_conn().ok_or("the shared store is unreachable")?;
    let Some((r, mut o)) = waiting(&mut conn, id)? else {
        return Ok(None);
    };
    if o.state != "waiting" {
        return Ok(Some((r, o)));
    }
    let targets: Vec<Target> = o.targets.iter().filter_map(|t| Target::decode(t)).collect();
    o.grounds.push("confirmed".into());
    o.taken_back = super::take_back(core, &mut conn, &r, &targets, actor, &o.grounds);
    o.state = "taken_back".into();
    o.decided_at = chrono::Utc::now().timestamp();
    reports::set_outcome(&mut conn, &r, &o).map_err(|e| e.to_string())?;
    Ok(Some((r, o)))
}

/// Leave a waiting report's copies where they are.
pub fn dismiss<S: ReportStore + ?Sized>(core: &S, id: &str, actor: &str) -> Found {
    let mut conn = core.net_conn().ok_or("the shared store is unreachable")?;
    let Some((r, mut o)) = waiting(&mut conn, id)? else {
        return Ok(None);
    };
    if o.state != "waiting" {
        return Ok(Some((r, o)));
    }
    o.state = "dismissed".into();
    o.decided_at = chrono::Utc::now().timestamp();
    reports::set_outcome(&mut conn, &r, &o).map_err(|e| e.to_string())?;
    let detail = format!("report={} inboxes={}", r.id, o.inboxes);
    notify::audit(&mut conn, actor, "report.dismiss", &r.thread_id, &detail);
    Ok(Some((r, o)))
}

/// What came of `user`'s own report `id`; `None` when it is not theirs.
pub fn outcome_for<S: ReportStore + ?Sized>(core: &S, user: &str, id: &str) -> Found {
    let mut conn = core.net_conn().ok_or("the shared store is unreachable")?;
    let r = reports::get(&mut conn, id).map_err(|e| e.to_string())?;
    let Some(r) = r.filter(|r| r.reporter.eq_ignore_ascii_case(user)) else {
        return Ok(None);
    };
    let o = reports::outcome(&mut conn, id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(Some((r, o)))
}
