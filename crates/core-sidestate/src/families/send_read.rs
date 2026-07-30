//! Reading the Send projection, and checking it before anything reads it
//! (RFC 20260730-send-status S3).
//!
//! ## One row per send, not per conversation
//!
//! Status is a property of an attempt, not of a conversation. Three sends
//! in one thread can be delivered, failed, and retrying at the same
//! moment, and a conversation-level row has nowhere to put that — nor
//! anywhere to hang "re-edit this one" when only one of the three
//! failed. So the Send view lists sends.
//!
//! ## What the shadow report is actually asking
//!
//! The old Sent axis lists threads; this lists sends. Comparing the two
//! as sets answers nothing useful, because every send that predates S1
//! has no row and always will.
//!
//! The question worth asking is narrower: **since the row-writing
//! shipped, has any send failed to get one?** That is the regression the
//! whole design exists to prevent, and it is invisible if historical
//! absence is counted alongside it. Same failure the thread-counter work
//! hit, where 64 expected divergences would have buried 182 actionable
//! ones had they been summed.
//!
//! So divergence is split by time, and `missing_since` is the number that
//! has to be zero.

use super::send::{RecipientState, Status, by_status_key, index_key, read_recipients, send_key};

/// A send as the list renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendListItem {
    pub send_id: String,
    pub thread_id: String,
    pub subject: String,
    pub to_csv: String,
    /// Cc as sent. Kept apart from `to_csv` because redraft has to put
    /// each address back in the field it came from, and because Bcc is
    /// recoverable only as `recipients - to - cc` — a Bcc header is not
    /// in the envelope, or it would not be blind.
    pub cc_csv: String,
    pub created_at: i64,
    pub status: Status,
    /// Empty when the maildir write failed and the bytes are not on disk.
    /// Resend and re-edit must refuse on those rather than act on an
    /// envelope they cannot read.
    pub envelope_ref: String,
    pub resent_from: Option<String>,
    pub recipients: Vec<RecipientState>,
}

impl SendListItem {
    /// Whether resend and re-edit have bytes to work from.
    ///
    /// A `kevy:` ref is the synthetic fallback the mirror writes when the
    /// maildir write failed — there is no file behind it. Offering the
    /// buttons anyway would give a control that silently does nothing.
    pub fn can_resend(&self) -> bool {
        !self.envelope_ref.is_empty() && !self.envelope_ref.starts_with("kevy:")
    }
}

/// Newest-first page of `user`'s sends, optionally one status only.
pub fn list_sends(
    conn: &mut kevy_client::Connection,
    user: &str,
    status: Option<Status>,
    offset: i64,
    limit: i64,
) -> std::io::Result<Vec<SendListItem>> {
    let key = match status {
        Some(s) => by_status_key(user, s),
        None => index_key(user),
    };
    // The network client has no zrevrange and its zrange returns members
    // without scores, so newest-first is a tail slice reversed here. Same
    // shape the DMARC report list uses for the same reason.
    let card = conn.zcard(key.as_bytes()).map_err(std::io::Error::other)? as i64;
    if card == 0 || offset >= card {
        return Ok(Vec::new());
    }
    let want_end = card - offset;
    let want_start = (want_end - limit.max(0)).max(0);
    let ids = conn
        .zrange(key.as_bytes(), want_start, want_end - 1)
        .map_err(std::io::Error::other)?;

    let mut out = Vec::with_capacity(ids.len());
    for raw in ids.into_iter().rev() {
        let Ok(send_id) = String::from_utf8(raw) else {
            continue;
        };
        if let Some(item) = read_one(conn, user, &send_id)? {
            out.push(item);
        }
    }
    Ok(out)
}

/// One send, with its recipients.
pub fn read_one(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<Option<SendListItem>> {
    let flat = conn
        .hgetall(send_key(user, send_id).as_bytes())
        .map_err(std::io::Error::other)?;
    if flat.is_empty() {
        return Ok(None);
    }
    let mut f = std::collections::BTreeMap::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        f.insert(
            String::from_utf8_lossy(&flat[i]).to_string(),
            String::from_utf8_lossy(&flat[i + 1]).to_string(),
        );
        i += 2;
    }
    let get = |k: &str| f.get(k).cloned().unwrap_or_default();
    let recipients = read_recipients(conn, user, send_id)?;
    Ok(Some(SendListItem {
        send_id: send_id.to_string(),
        thread_id: get("thread_id"),
        subject: get("subject"),
        to_csv: get("to_csv"),
        cc_csv: get("cc_csv"),
        created_at: get("created_at").parse().unwrap_or(0),
        // The stored status is what the writers maintain; deriving here
        // instead would disagree with `by_status` and put a send in one
        // bucket while the row said another.
        status: Status::parse(&get("status")).unwrap_or(Status::Sending),
        envelope_ref: get("envelope_ref"),
        resent_from: Some(get("resent_from")).filter(|s| !s.is_empty()),
        recipients,
    }))
}

/// How the projection compares to the threads the old Sent axis holds.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SendShadowReport {
    /// Threads on the old Sent axis.
    pub axis_threads: u64,
    /// Sends in the projection.
    pub projection_sends: u64,
    /// Threads with no send row, created before `since`. Every send that
    /// predates the row-writing is here and always will be; this is
    /// history, not a fault.
    pub missing_before: u64,
    /// Threads with no send row, created at or after `since`. **This is
    /// the gate: it must be zero.** A send after the cutover with no row
    /// is the regression the design exists to prevent.
    pub missing_since: u64,
    /// Up to eight `missing_since` thread ids, so the report names rows
    /// rather than only counting them.
    pub samples: Vec<String>,
}

const MAX_SAMPLES: usize = 8;

/// Compare the projection against the threads the old axis lists.
///
/// `axis` is the (thread_id, latest_date) pairs the caller read from the
/// old Sent axis — passed in rather than read here because that axis
/// lives in the embedded store and this family talks to the network one.
pub fn shadow_report(
    conn: &mut kevy_client::Connection,
    user: &str,
    axis: &[(String, i64)],
    since: i64,
) -> std::io::Result<SendShadowReport> {
    let mut report = SendShadowReport {
        axis_threads: axis.len() as u64,
        projection_sends: conn
            .zcard(index_key(user).as_bytes())
            .map_err(std::io::Error::other)? as u64,
        ..Default::default()
    };

    // Thread ids the projection covers. Read once; the alternative is a
    // lookup per axis entry, and this axis runs to a few hundred rows.
    let ids = conn
        .zrange(index_key(user).as_bytes(), 0, -1)
        .map_err(std::io::Error::other)?;
    let mut covered = std::collections::HashSet::new();
    for raw in ids {
        let Ok(send_id) = String::from_utf8(raw) else {
            continue;
        };
        if let Some(item) = read_one(conn, user, &send_id)? {
            covered.insert(item.thread_id);
        }
    }

    for (tid, latest) in axis {
        if covered.contains(tid) {
            continue;
        }
        if *latest >= since {
            report.missing_since += 1;
            if report.samples.len() < MAX_SAMPLES {
                report.samples.push(tid.clone());
            }
        } else {
            report.missing_before += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(envelope_ref: &str) -> SendListItem {
        SendListItem {
            send_id: "m1".into(),
            thread_id: "t1".into(),
            subject: "s".into(),
            to_csv: "a@x.com".into(),
            cc_csv: String::new(),
            created_at: 100,
            status: Status::Delivered,
            envelope_ref: envelope_ref.into(),
            resent_from: None,
            recipients: Vec::new(),
        }
    }

    /// A control that silently does nothing is worse than an absent one.
    #[test]
    fn resend_is_refused_when_the_bytes_are_not_on_disk() {
        assert!(item("cur/1785342413.M667051P1Q10.host").can_resend());
        assert!(
            !item("kevy:848da09d68bcd8a6@golia.jp").can_resend(),
            "a synthetic ref means the maildir write failed"
        );
        assert!(!item("").can_resend());
    }
}
