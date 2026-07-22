//! Contact search + scoring, served from the network kevy so both cores
//! read the identical data:
//!   `mailrs:user:{user}:contacts`     hash email → display (autocomplete;
//!                                      live_sync + inbound pipeline write it)
//!   `mailrs:contact:{user}:{email}`   hash of scoring fields
//!
//! Both cores proxy these keys, so the contact routes behave identically.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;

use mailrs_core_api::method::contact::{
    ContactScoring, HasSentToResponse, SearchContactsQuery, SearchContactsResponse,
    SenderFeedbackRequest, UpsertInboundContactRequest,
};

use crate::NetKevy;

fn contact_key(user: &str, email: &str) -> String {
    format!("mailrs:contact:{user}:{email}")
}

fn hget_str(conn: &mut kevy_client::Connection, key: &str, field: &str) -> Option<String> {
    conn.hget(key.as_bytes(), field.as_bytes())
        .ok()
        .flatten()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
}

fn hget_bool(conn: &mut kevy_client::Connection, key: &str, field: &str) -> bool {
    hget_str(conn, key, field).as_deref() == Some("1")
}

fn hget_u32(conn: &mut kevy_client::Connection, key: &str, field: &str) -> u32 {
    hget_str(conn, key, field)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub async fn search_contacts<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path(user): Path<String>,
    Query(q): Query<SearchContactsQuery>,
) -> Json<SearchContactsResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(SearchContactsResponse { items: Vec::new() });
    };
    let flat = conn
        .hgetall(format!("mailrs:user:{user}:contacts").as_bytes())
        .unwrap_or_default();
    let needle = q.q.to_lowercase();
    let mut items = Vec::new();
    let mut i = 0;
    while i + 1 < flat.len() {
        let email = String::from_utf8_lossy(&flat[i]).into_owned();
        let display = String::from_utf8_lossy(&flat[i + 1]).to_lowercase();
        if needle.is_empty() || email.to_lowercase().contains(&needle) || display.contains(&needle)
        {
            items.push(email);
        }
        i += 2;
        if items.len() >= q.limit as usize {
            break;
        }
    }
    Json(SearchContactsResponse { items })
}

/// Record that `user` received a message from `email`.
///
/// Bumps `received_count` and refreshes the sender-class flags. In
/// process so the ingest path can call it directly; the HTTP handler
/// below is a thin wrapper.
///
/// The counter uses HINCRBY rather than read-modify-write: two messages
/// from the same sender arriving concurrently would otherwise lose one
/// increment (project rule `kevy/atomic-counter`).
pub fn record_inbound(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    display_name: &str,
    is_mailing_list: bool,
    is_automated: bool,
) -> std::io::Result<()> {
    let key = contact_key(user, email);
    let flags: [(&[u8], &[u8]); 3] = [
        (b"display_name", display_name.as_bytes()),
        (
            b"is_mailing_list",
            if is_mailing_list { b"1" } else { b"0" },
        ),
        (b"is_automated", if is_automated { b"1" } else { b"0" }),
    ];
    conn.hset(key.as_bytes(), &flags)?;
    conn.pipeline(|p| {
        p.cmd(&[b"HINCRBY", key.as_bytes(), b"received_count", b"1"]);
    })?;
    Ok(())
}

/// Record that `user` sent a message to `email`.
///
/// This is the counter that makes a relationship *mutual* — without it
/// `is_mutual` and `has_sent_to` can never be true, and the strongest
/// importance signals (+0.3 each) stay permanently off.
pub fn record_sent_to(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
) -> std::io::Result<()> {
    let key = contact_key(user, email);
    conn.pipeline(|p| {
        p.cmd(&[b"HINCRBY", key.as_bytes(), b"sent_count", b"1"]);
    })?;
    Ok(())
}

pub async fn upsert_inbound<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, email)): Path<(String, String)>,
    Json(req): Json<UpsertInboundContactRequest>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match record_inbound(
        &mut conn,
        &user,
        &email,
        &req.display_name,
        req.is_mailing_list,
        req.is_automated,
    ) {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// How the user engaged with a message from this sender.
///
/// These are the *implicit* signals — what the user did, not what they
/// declared. They are the raw material for per-user learning: a sender
/// whose mail is opened within minutes matters more than one whose mail
/// is archived unread, regardless of how bulk-ish the headers look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engagement {
    /// Thread was opened. `fast` when it happened close enough to
    /// arrival to read as "the user was waiting for this".
    Read { fast: bool },
    /// Archived while still unread — the user dismissed it unseen.
    ArchivedUnread,
    /// Explicitly starred.
    Starred,
    /// Explicitly marked junk.
    MarkedJunk,
}

impl Engagement {
    /// Counter fields this event bumps.
    fn fields(self) -> &'static [&'static [u8]] {
        match self {
            Engagement::Read { fast: true } => &[b"read_count", b"read_fast_count"],
            Engagement::Read { fast: false } => &[b"read_count"],
            Engagement::ArchivedUnread => &[b"archived_unread_count"],
            Engagement::Starred => &[b"starred_count"],
            Engagement::MarkedJunk => &[b"marked_junk_count"],
        }
    }
}

/// Record one engagement event against a sender's relationship record.
///
/// Counters, not an event log: the learner needs rates per sender
/// (opened 9 of 10, archived unread 8 of 10), and a rate is all a raw
/// log would be reduced to anyway. HINCRBY keeps concurrent marks from
/// losing increments (`kevy/atomic-counter`).
///
/// Best-effort by contract — the caller is servicing a user action that
/// must not fail because a derived counter could not be written.
pub fn record_engagement(
    conn: &mut kevy_client::Connection,
    user: &str,
    email: &str,
    event: Engagement,
) -> std::io::Result<()> {
    let key = contact_key(user, email);
    conn.pipeline(|p| {
        for f in event.fields() {
            p.cmd(&[b"HINCRBY", key.as_bytes(), f, b"1"]);
        }
    })?;
    Ok(())
}

/// Read one sender's scoring record, in process.
///
/// The HTTP handler below is a thin wrapper over this; the fastcore
/// ingest path calls it directly to score inbound importance without a
/// loopback request. Keeping one implementation means the served
/// numbers and the numbers importance scoring uses can never disagree.
///
/// A sender with no record yields the all-false / zero default, which is
/// the correct reading: no relationship is known yet.
pub fn scoring_for(conn: &mut kevy_client::Connection, user: &str, email: &str) -> ContactScoring {
    let key = contact_key(user, email);
    let received = hget_u32(conn, &key, "received_count");
    let sent = hget_u32(conn, &key, "sent_count");
    ContactScoring {
        // Mutual means traffic has flowed both ways.
        is_mutual: received > 0 && sent > 0,
        is_mailing_list: hget_bool(conn, &key, "is_mailing_list"),
        is_vip: hget_bool(conn, &key, "is_vip"),
        is_blocked: hget_bool(conn, &key, "is_blocked"),
        importance_bias: hget_str(conn, &key, "importance_bias")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        received_count: received,
        sent_count: sent,
    }
}

/// True when the user has ever sent to this address — the "this is a
/// reply to something I started" relationship signal. In process, for
/// the same reason as [`scoring_for`].
pub fn has_sent_to_addr(conn: &mut kevy_client::Connection, user: &str, email: &str) -> bool {
    hget_u32(conn, &contact_key(user, email), "sent_count") > 0
}

pub async fn contact_scoring<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, email)): Path<(String, String)>,
) -> Json<ContactScoring> {
    let Some(mut conn) = state.net_conn() else {
        return Json(ContactScoring {
            is_mutual: false,
            is_mailing_list: false,
            is_vip: false,
            is_blocked: false,
            importance_bias: 0.0,
            received_count: 0,
            sent_count: 0,
        });
    };
    Json(scoring_for(&mut conn, &user, &email))
}

pub async fn has_sent_to<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, email)): Path<(String, String)>,
) -> Json<HasSentToResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(HasSentToResponse { has_sent: false });
    };
    Json(HasSentToResponse {
        has_sent: has_sent_to_addr(&mut conn, &user, &email),
    })
}

pub async fn sender_feedback<S: NetKevy>(
    State(state): State<Arc<S>>,
    Path((user, email)): Path<(String, String)>,
    Json(req): Json<SenderFeedbackRequest>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let key = contact_key(&user, &email);
    match req.action.as_str() {
        "vip" => {
            let _ = conn.hset(key.as_bytes(), &[(b"is_vip".as_slice(), b"1".as_slice())]);
        }
        "block" => {
            let _ = conn.hset(
                key.as_bytes(),
                &[(b"is_blocked".as_slice(), b"1".as_slice())],
            );
        }
        _ => {}
    }
    if let Some(delta) = req.bias_delta {
        let cur: f32 = hget_str(&mut conn, &key, "importance_bias")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        let next = (cur + delta).to_string();
        let _ = conn.hset(
            key.as_bytes(),
            &[(b"importance_bias".as_slice(), next.as_bytes())],
        );
    }
    StatusCode::NO_CONTENT
}
