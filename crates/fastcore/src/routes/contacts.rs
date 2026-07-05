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

use crate::FastcoreState;

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

pub async fn search_contacts(
    State(state): State<Arc<FastcoreState>>,
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

pub async fn upsert_inbound(
    State(state): State<Arc<FastcoreState>>,
    Path((user, email)): Path<(String, String)>,
    Json(req): Json<UpsertInboundContactRequest>,
) -> StatusCode {
    let Some(mut conn) = state.net_conn() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let key = contact_key(&user, &email);
    let received_str = (hget_u32(&mut conn, &key, "received_count") + 1).to_string();
    let fields: [(&[u8], &[u8]); 4] = [
        (b"display_name", req.display_name.as_bytes()),
        (
            b"is_mailing_list",
            if req.is_mailing_list { b"1" } else { b"0" },
        ),
        (b"is_automated", if req.is_automated { b"1" } else { b"0" }),
        (b"received_count", received_str.as_bytes()),
    ];
    match conn.hset(key.as_bytes(), &fields) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub async fn contact_scoring(
    State(state): State<Arc<FastcoreState>>,
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
    let key = contact_key(&user, &email);
    let received = hget_u32(&mut conn, &key, "received_count");
    let sent = hget_u32(&mut conn, &key, "sent_count");
    Json(ContactScoring {
        is_mutual: received > 0 && sent > 0,
        is_mailing_list: hget_bool(&mut conn, &key, "is_mailing_list"),
        is_vip: hget_bool(&mut conn, &key, "is_vip"),
        is_blocked: hget_bool(&mut conn, &key, "is_blocked"),
        importance_bias: hget_str(&mut conn, &key, "importance_bias")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0),
        received_count: received,
        sent_count: sent,
    })
}

pub async fn has_sent_to(
    State(state): State<Arc<FastcoreState>>,
    Path((user, email)): Path<(String, String)>,
) -> Json<HasSentToResponse> {
    let Some(mut conn) = state.net_conn() else {
        return Json(HasSentToResponse { has_sent: false });
    };
    let sent = hget_u32(&mut conn, &contact_key(&user, &email), "sent_count");
    Json(HasSentToResponse { has_sent: sent > 0 })
}

pub async fn sender_feedback(
    State(state): State<Arc<FastcoreState>>,
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
