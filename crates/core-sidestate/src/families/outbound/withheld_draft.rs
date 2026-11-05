//! A withheld send whose time passed while its account was deleted goes
//! back to the account as a draft at the restore, unsent: the message's
//! recipients, subject and text, and the time it was set to go
//! (`withheld_send_at`), which the draft shows. Its jobs leave the queue,
//! and its Send row says it was cancelled, by the restore.
//!
//! A draft holds no attachments; a message that had some keeps them in the
//! Sent copy its send filed.

use std::io;

use mailrs_core_api::method::admin::DraftWire;
use mailrs_rfc5322::{addr_key, addr_spec, split_list};

use super::*;
use crate::families::prefs::{drafts_counter_key, drafts_key};
use crate::families::send;

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

/// Turn the withheld `jobs` of `address`'s send `send` (set to go at `at`)
/// into one draft, and take them out of the queue.
pub(super) fn return_as_draft(
    conn: &mut kevy_client::Connection,
    address: &str,
    send: &str,
    at: i64,
    jobs: &[(i64, serde_json::Value)],
    now: i64,
) -> io::Result<()> {
    use base64::Engine as _;
    let raw = jobs
        .iter()
        .find_map(|(_, b)| b.get("message_data_b64").and_then(|d| d.as_str()))
        .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d).ok())
        .unwrap_or_default();
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
        created_at: now,
        updated_at: now,
    };
    let json = serde_json::to_vec(&draft).map_err(io::Error::other)?;
    conn.hset(
        drafts_key(address).as_bytes(),
        &[(id.to_string().as_bytes(), json.as_slice())],
    )?;
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
    Ok(())
}
