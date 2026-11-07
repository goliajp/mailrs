//! A withheld send whose time passed while its account was deleted goes
//! back to the account as a draft at the restore, unsent: the message's
//! recipients, subject and text, and the time it was set to go
//! (`withheld_send_at`), which the draft shows. Its jobs leave the queue,
//! and its Send row says it was cancelled, by the restore.
//!
//! The draft keeps the message's attachments: each is copied into the
//! account's upload store, so the draft stands on its own whatever becomes
//! of the copy in Sent. Images the HTML draws in place by `cid:` are not
//! attachments here, as the reader is not shown them as such. Neither the
//! upload store's limits nor the 25 MiB a draft may hold are checked: a
//! restore must not fail, and a draft over that is refused when sent.

use std::io;
use std::path::Path;
use std::time::SystemTime;

use mailrs_core_api::method::admin::{DraftAttachment, DraftWire};
use mailrs_rfc5322::{addr_key, addr_spec, split_list};

use super::*;
use crate::families::prefs::drafts_counter_key;
use crate::families::send;
use crate::families::uploads;

/// What a send returned as a draft says on each recipient.
pub const RETURNED_AS_DRAFT: &str = "not sent: returned as a draft when the account was restored";

/// `name`'s value in `raw`'s header, unfolded and decoded.
fn header(raw: &[u8], name: &str) -> String {
    let Some(v) = mailrs_rfc5322::Message::new(raw).header(name) else {
        return String::new();
    };
    let unfolded: Vec<u8> = v
        .iter()
        .copied()
        .filter(|b| *b != b'\r' && *b != b'\n')
        .collect();
    mailrs_rfc2047::decode(&unfolded).trim().to_string()
}

/// The text the message says, as the composer holds it.
fn text(raw: &[u8]) -> String {
    let root = mailrs_mime::parse(raw);
    root.find_by_content_type("text/plain")
        .or_else(|| root.find_by_content_type("text/html"))
        .and_then(|p| p.body_text())
        .unwrap_or_default()
}

/// Copy the message's attachments into `user`'s uploads, in message order.
fn copy_attachments(uploads: &Path, user: &str, raw: &[u8]) -> io::Result<Vec<DraftAttachment>> {
    let message = mailrs_mime::parse(raw);
    message
        .shown_attachments()
        .map(|p| {
            let sha = uploads::keep(uploads, user, &p.body, SystemTime::now())?;
            Ok(DraftAttachment {
                blob_id: uploads::blob_id(&sha),
                name: p.attachment_filename().unwrap_or("unnamed").to_string(),
                content_type: p.content_type.mime_type(),
                size: p.body.len() as u64,
            })
        })
        .collect()
}

/// Turn the withheld `jobs` of `address`'s send `send` (set to go at `at`)
/// into one draft, its attachments copied into `uploads`, and take them out
/// of the queue. Returns how many attachments the draft carries.
pub(super) fn return_as_draft(
    conn: &mut kevy_client::Connection,
    uploads: &Path,
    address: &str,
    send: &str,
    at: i64,
    jobs: &[(i64, serde_json::Value)],
    now: i64,
) -> io::Result<u64> {
    use base64::Engine as _;
    let raw = jobs
        .iter()
        .find_map(|(_, b)| b.get("message_data_b64").and_then(|d| d.as_str()))
        .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
        .unwrap_or_default();
    let attachments = copy_attachments(uploads, address, &raw)?;
    let (to, cc) = (header(&raw, "To"), header(&raw, "Cc"));
    let addressed: Vec<String> = split_list(&to)
        .chain(split_list(&cc))
        .map(addr_key)
        .collect();
    let bcc: Vec<&str> = jobs
        .iter()
        .filter_map(|(_, b)| b.get("recipient").and_then(|r| r.as_str()))
        .filter(|r| !addressed.contains(&addr_key(r)))
        .collect();
    let from_header = header(&raw, "From");
    let from = addr_spec(&from_header);
    let from = match from.eq_ignore_ascii_case(address) {
        true => String::new(),
        false => from.to_string(),
    };
    let row = send::send_key(address, send);
    let thread = conn
        .hget(row.as_bytes(), b"thread_id")?
        .map(|t| String::from_utf8_lossy(&t).into_owned())
        .filter(|t| !t.is_empty());
    let id = conn.incr(drafts_counter_key(address).as_bytes())?;
    let draft = DraftWire {
        id,
        to,
        cc,
        bcc: bcc.join(", "),
        subject: header(&raw, "Subject"),
        body: text(&raw),
        reply_to_thread_id: thread,
        from,
        scheduled_at: None,
        withheld_send_at: Some(at),
        attachments_not_included: Vec::new(),
        attachments,
        created_at: now,
        updated_at: now,
    };
    crate::families::draft_store::put(conn, address, &draft)?;
    let carried = draft.attachments.len() as u64;
    for (id, _) in jobs {
        let (job, legacy) = (job_key(id), legacy_row_key(id));
        conn.del(&[job.as_bytes(), legacy.as_bytes()])?;
    }
    if conn.exists(&[row.as_bytes()])? == 1 {
        send::cancel_send(conn, address, send, RETURNED_AS_DRAFT)?;
        let event = send::SendEvent {
            kind: "cancelled".into(),
            at: now,
            by: "restore".into(),
            send_at: Some(at),
        };
        send::record_event(conn, address, send, &event)?;
    }
    Ok(carried)
}

#[cfg(test)]
#[path = "withheld_draft_tests.rs"]
mod tests;
