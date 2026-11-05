//! What an account's delete does to its queued mail.
//!
//! Mail already released for delivery (on `pending-idx`, in flight, or on
//! [`RETRY_IDX`] between attempts) was sent by the person, and keeps being
//! delivered. Mail not yet released waits on [`SCHEDULED_IDX`]: a send
//! scheduled for later, the undo window of an immediate send, a JMAP
//! submission held with `HOLDFOR` / `HOLDUNTIL`. That is withheld at the
//! delete: taken out of the index and marked [`WITHHELD`], which neither
//! the sender's claim nor its recovery picks up, and listed under the
//! account ([`withheld_key`]) with the time it waited for. A restore puts
//! back what is still ahead; what lapsed meanwhile is not sent and comes
//! back as a draft (`withheld_draft`). The purge deletes it with the
//! account.
//!
//! The sender's due-sweep, a cancel and the delete can reach one job at the
//! same moment: whoever takes it out of [`SCHEDULED_IDX`] has it.

use std::io;

use super::*;

/// The state of a job withheld at its account's delete.
pub const WITHHELD: &[u8] = b"withheld";

/// Whose a job is: the account it was sent from when that is not the
/// envelope sender (a mask, a send-as address), the sender otherwise.
fn sent_from(conn: &mut kevy_client::Connection, id: i64, address: &str) -> io::Result<bool> {
    let Some(raw) = conn.hget(job_key(id).as_bytes(), b"blob")? else {
        return Ok(false);
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) else {
        return Ok(false);
    };
    let owner = v
        .get("account")
        .and_then(|a| a.as_str())
        .or_else(|| v.get("sender").and_then(|s| s.as_str()));
    Ok(owner.is_some_and(|o| o.eq_ignore_ascii_case(address)))
}

/// Withhold `address`'s jobs not yet released for delivery; how many.
pub fn withhold(conn: &mut kevy_client::Connection, address: &str) -> io::Result<usize> {
    let list = withheld_key(address);
    let mut n = 0;
    for m in conn.zrange(SCHEDULED_IDX, 0, -1)? {
        let Some(id) = std::str::from_utf8(&m).ok().and_then(|s| s.parse().ok()) else {
            continue;
        };
        if !sent_from(conn, id, address)? {
            continue;
        }
        let Some(at) = conn.zscore(SCHEDULED_IDX, &m)? else {
            continue;
        };
        // listed and marked before it leaves the index, so a job is never
        // pending in no index, which recovery would put on the queue
        let at = (at as i64).to_string();
        conn.hset(list.as_bytes(), &[(m.as_slice(), at.as_bytes())])?;
        let job = job_key(id);
        conn.hset(job.as_bytes(), &[(b"state" as &[u8], WITHHELD)])?;
        if conn.zrem(SCHEDULED_IDX, &[m.as_slice()])? == 0 {
            // released or cancelled meanwhile: as it was
            conn.hdel(list.as_bytes(), &[m.as_slice()])?;
            if conn.exists(&[job.as_bytes()])? == 1 {
                conn.hset(job.as_bytes(), &[(b"state" as &[u8], b"pending" as &[u8])])?;
            }
            continue;
        }
        n += 1;
    }
    Ok(n)
}

/// What becomes of withheld mail whose send time has passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lapsed {
    /// Back on the scheduled index, to go at the next due-sweep: the
    /// delete that withheld it did not happen.
    Send,
    /// Not sent: back to the account as a draft. A restore, after the
    /// account was gone past the time the mail was set to go.
    Draft,
}

/// What [`release`] did, by message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Released {
    pub rescheduled: u64,
    pub drafts: u64,
    /// Attachments those drafts could not carry, all together.
    pub attachments_not_included: u64,
}

/// Put `address`'s withheld mail back: a message whose send time is still
/// ahead waits on the scheduled index again; one whose time has passed is
/// handled as `lapsed` says. Jobs are taken message by message, the jobs
/// of one send (one per recipient) together.
pub fn release(
    conn: &mut kevy_client::Connection,
    address: &str,
    now: i64,
    lapsed: Lapsed,
) -> io::Result<Released> {
    let list = withheld_key(address);
    let flat = conn.hgetall(list.as_bytes())?;
    // send → (send time, its jobs with their rows)
    let mut sends: std::collections::BTreeMap<String, (i64, Vec<(i64, serde_json::Value)>)> =
        Default::default();
    for pair in flat.chunks(2) {
        let [m, at] = pair else { continue };
        conn.hdel(list.as_bytes(), &[m.as_slice()])?;
        let Some(id) = std::str::from_utf8(m)
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
        else {
            continue;
        };
        let job = job_key(id);
        if conn.hget(job.as_bytes(), b"state")?.as_deref() != Some(WITHHELD) {
            continue;
        }
        let blob: serde_json::Value = conn
            .hget(job.as_bytes(), b"blob")?
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let send = blob
            .get("send_id")
            .and_then(|s| s.as_str())
            .map_or_else(|| format!("job {id}"), String::from);
        let at = String::from_utf8_lossy(at).parse::<i64>().unwrap_or(0);
        sends
            .entry(send)
            .or_insert((at, Vec::new()))
            .1
            .push((id, blob));
    }
    let mut done = Released::default();
    for (send, (at, jobs)) in sends {
        if at <= now && lapsed == Lapsed::Draft {
            done.attachments_not_included +=
                super::withheld_draft::return_as_draft(conn, address, &send, at, &jobs, now)?;
            done.drafts += 1;
            continue;
        }
        let now = now.to_string();
        for (id, _) in &jobs {
            // pending first, with a fresh time so recovery waits for the
            // index entry that follows
            conn.hset(
                job_key(id).as_bytes(),
                &[
                    (b"state" as &[u8], b"pending" as &[u8]),
                    (b"updated_at", now.as_bytes()),
                ],
            )?;
            conn.zadd(SCHEDULED_IDX, &[(at as f64, id.to_string().as_bytes())])?;
        }
        done.rescheduled += 1;
    }
    Ok(done)
}

/// Count `address`'s withheld jobs, or delete them with their list.
pub fn purge_withheld(
    conn: &mut kevy_client::Connection,
    address: &str,
    dry_run: bool,
) -> io::Result<u64> {
    let list = withheld_key(address);
    let ids = conn.hkeys(list.as_bytes())?;
    if dry_run {
        return Ok(ids.len() as u64);
    }
    for m in &ids {
        let id = String::from_utf8_lossy(m);
        let job = job_key(&*id);
        if conn.hget(job.as_bytes(), b"state")?.as_deref() == Some(WITHHELD) {
            let legacy = legacy_row_key(&*id);
            conn.del(&[job.as_bytes(), legacy.as_bytes()])?;
        }
    }
    conn.del(&[list.as_bytes()])?;
    Ok(ids.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAVE: &str = "dave@x.test";

    /// A job as the enqueue writes it (its pipeline is not in the
    /// in-process store): scheduled for `at`, or released when `None`.
    fn job_as(
        c: &mut kevy_client::Connection,
        sender: &str,
        account: Option<&str>,
        at: Option<i64>,
    ) -> i64 {
        let id = c.incr(COUNTER).unwrap();
        let mut blob = serde_json::json!({ "id": id, "sender": sender });
        if let Some(a) = account {
            blob["account"] = a.into();
        }
        let blob = blob.to_string();
        c.hset(
            job_key(id).as_bytes(),
            &[
                (b"state" as &[u8], b"pending" as &[u8]),
                (b"blob", blob.as_bytes()),
            ],
        )
        .unwrap();
        let m = id.to_string();
        match at {
            Some(at) => {
                c.zadd(SCHEDULED_IDX, &[(at as f64, m.as_bytes())]).unwrap();
            }
            None => {
                c.lpush(PENDING_IDX, &[m.as_bytes()]).unwrap();
            }
        }
        id
    }

    fn job(c: &mut kevy_client::Connection, sender: &str, at: Option<i64>) -> i64 {
        job_as(c, sender, None, at)
    }

    fn state(c: &mut kevy_client::Connection, id: i64) -> Option<Vec<u8>> {
        c.hget(job_key(id).as_bytes(), b"state").unwrap()
    }

    fn scheduled(c: &mut kevy_client::Connection, id: i64) -> Option<f64> {
        c.zscore(SCHEDULED_IDX, id.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn only_what_is_not_yet_released_is_withheld_and_a_restore_gives_it_back() {
        let mut c = kevy_client::Connection::connect("mem://outbound-withheld").unwrap();
        let later = job(&mut c, DAVE, Some(5_000));
        let now = job(&mut c, DAVE, None);
        let others = job(&mut c, "erin@x.test", Some(5_000));
        let masked = job_as(&mut c, "mask-1@x.test", Some(DAVE), Some(6_000));
        assert_eq!(withhold(&mut c, DAVE).unwrap(), 2);
        for id in [later, masked] {
            assert_eq!(state(&mut c, id).as_deref(), Some(WITHHELD));
            assert_eq!(scheduled(&mut c, id), None);
        }
        assert_eq!(state(&mut c, now).as_deref(), Some(b"pending" as &[u8]));
        assert_eq!(scheduled(&mut c, others), Some(5_000.0));
        assert_eq!(purge_withheld(&mut c, DAVE, true).unwrap(), 2);

        assert_eq!(
            release(&mut c, DAVE, 10, Lapsed::Draft)
                .unwrap()
                .rescheduled,
            2
        );
        assert_eq!(scheduled(&mut c, later), Some(5_000.0));
        assert_eq!(scheduled(&mut c, masked), Some(6_000.0));
        assert_eq!(state(&mut c, later).as_deref(), Some(b"pending" as &[u8]));
        assert_eq!(c.hlen(withheld_key(DAVE).as_bytes()).unwrap(), 0);
    }

    #[test]
    fn a_purge_deletes_the_withheld_and_a_restore_leaves_one_that_moved_on() {
        let mut c = kevy_client::Connection::connect("mem://outbound-withheld-purge").unwrap();
        let a = job(&mut c, DAVE, Some(5_000));
        let b = job(&mut c, DAVE, Some(5_000));
        assert_eq!(withhold(&mut c, DAVE).unwrap(), 2);
        // b was claimed by a sender regardless: it is not put back
        c.hset(
            job_key(b).as_bytes(),
            &[(b"state" as &[u8], b"inflight" as &[u8])],
        )
        .unwrap();
        assert_eq!(
            release(&mut c, DAVE, 10, Lapsed::Draft)
                .unwrap()
                .rescheduled,
            1
        );
        assert_eq!(scheduled(&mut c, b), None);
        assert_eq!(scheduled(&mut c, a), Some(5_000.0));

        assert_eq!(withhold(&mut c, DAVE).unwrap(), 1);
        assert_eq!(purge_withheld(&mut c, DAVE, false).unwrap(), 1);
        assert_eq!(state(&mut c, a), None);
        assert_eq!(state(&mut c, b).as_deref(), Some(b"inflight" as &[u8]));
        assert_eq!(c.exists(&[withheld_key(DAVE).as_bytes()]).unwrap(), 0);
    }

    #[test]
    fn a_send_whose_time_passed_is_a_draft_after_the_restore_and_never_queued() {
        use base64::Engine as _;
        let mut c = kevy_client::Connection::connect("mem://outbound-withheld-draft").unwrap();
        let raw = b"From: Mask <mask-1@x.test>\r\nTo: erin@y.test\r\nSubject: hi\r\n\r\nbody\r\n";
        let data = base64::engine::general_purpose::STANDARD.encode(raw);
        let mut ids = Vec::new();
        for rcpt in ["erin@y.test", "gus@y.test"] {
            let id = job_as(&mut c, "mask-1@x.test", Some(DAVE), Some(5_000));
            let blob = serde_json::json!({ "sender": "mask-1@x.test", "account": DAVE,
                "recipient": rcpt, "send_id": "s@x.test", "message_data_b64": data })
            .to_string();
            c.hset(
                job_key(id).as_bytes(),
                &[(b"blob" as &[u8], blob.as_bytes())],
            )
            .unwrap();
            ids.push(id);
        }
        assert_eq!(withhold(&mut c, DAVE).unwrap(), 2);
        let done = release(&mut c, DAVE, 10_000, Lapsed::Draft).unwrap();
        assert_eq!(
            done,
            Released {
                rescheduled: 0,
                drafts: 1,
                attachments_not_included: 0,
            }
        );
        for id in ids {
            assert_eq!(
                state(&mut c, id),
                None,
                "the job of a draft is still queued"
            );
            assert_eq!(scheduled(&mut c, id), None);
        }
        let drafts = c
            .hgetall(crate::families::prefs::drafts_key(DAVE).as_bytes())
            .unwrap();
        assert_eq!(drafts.len(), 2, "one draft for the send");
        let d: mailrs_core_api::method::admin::DraftWire =
            serde_json::from_slice(&drafts[1]).unwrap();
        assert_eq!(
            (d.to.as_str(), d.bcc.as_str()),
            ("erin@y.test", "gus@y.test")
        );
        assert_eq!(
            (d.from.as_str(), d.subject.as_str()),
            ("mask-1@x.test", "hi")
        );
        assert_eq!(d.body.trim(), "body");
        assert_eq!((d.withheld_send_at, d.scheduled_at), (Some(5_000), None));
        // nothing was left behind, so the draft says nothing about it
        assert!(d.attachments_not_included.is_empty());
        let raw_json = String::from_utf8_lossy(&drafts[1]).into_owned();
        assert!(!raw_json.contains("attachments_not_included"), "{raw_json}");
    }

    #[test]
    fn a_draft_names_the_attachments_it_did_not_carry() {
        use base64::Engine as _;
        let mut c = kevy_client::Connection::connect("mem://outbound-withheld-files").unwrap();
        let raw = b"From: dave@x.test\r\nTo: erin@y.test\r\nSubject: files\r\n\
            MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
            --b\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
            --b\r\nContent-Type: application/pdf\r\n\
            Content-Disposition: attachment; filename=\"agenda.pdf\"\r\n\r\n%PDF\r\n\
            --b\r\nContent-Type: image/png; name=\"map.png\"\r\n\r\npng\r\n--b--\r\n";
        let id = job_as(&mut c, DAVE, None, Some(5_000));
        let blob = serde_json::json!({ "sender": DAVE, "recipient": "erin@y.test",
            "send_id": "f@x.test",
            "message_data_b64": base64::engine::general_purpose::STANDARD.encode(raw) })
        .to_string();
        c.hset(
            job_key(id).as_bytes(),
            &[(b"blob" as &[u8], blob.as_bytes())],
        )
        .unwrap();
        withhold(&mut c, DAVE).unwrap();
        let done = release(&mut c, DAVE, 10_000, Lapsed::Draft).unwrap();
        assert_eq!((done.drafts, done.attachments_not_included), (1, 2));
        let drafts = c
            .hgetall(crate::families::prefs::drafts_key(DAVE).as_bytes())
            .unwrap();
        let d: mailrs_core_api::method::admin::DraftWire =
            serde_json::from_slice(&drafts[1]).unwrap();
        assert_eq!(d.attachments_not_included, ["agenda.pdf", "map.png"]);
        assert_eq!(d.body.trim(), "see attached");
    }

    #[test]
    fn a_save_keeps_what_the_restore_wrote_on_the_draft() {
        let mut c = kevy_client::Connection::connect("mem://outbound-withheld-kept").unwrap();
        let draft = serde_json::json!({ "id": 3, "to": "", "cc": "", "bcc": "", "subject": "",
            "body": "", "withheld_send_at": 5_000, "attachments_not_included": ["a.pdf"],
            "created_at": 1, "updated_at": 1 })
        .to_string();
        c.hset(
            crate::families::prefs::drafts_key(DAVE).as_bytes(),
            &[(b"3".as_slice(), draft.as_bytes())],
        )
        .unwrap();
        let kept = crate::families::prefs::kept_restore_notes(&mut c, DAVE, 3);
        assert_eq!(kept.withheld_send_at, Some(5_000));
        assert_eq!(kept.attachments_not_included, ["a.pdf"]);
        assert_eq!(
            crate::families::prefs::kept_restore_notes(&mut c, DAVE, 4),
            crate::families::prefs::RestoreNotes::default()
        );
    }
}
