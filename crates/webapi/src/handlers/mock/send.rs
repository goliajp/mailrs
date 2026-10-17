//! Sending from a namespace. Nothing leaves the deployment: mock
//! recipients receive the message, every other recipient is only written
//! down in the sender's captured copy.

use std::sync::Arc;

use axum::http::StatusCode;
use mailrs_core_sidestate::families::mock::{self, Direction, MessageMeta, Stored};
use serde::Serialize;

use super::service::{MockApiError, MockApiResult, MockCaller, authorize, now_ms, store};
use crate::WebState;

#[derive(Debug, Clone)]
pub struct SendRequest {
    pub from_tag: Option<String>,
    pub to: Vec<String>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SendResult {
    /// id of the captured copy in the sending namespace
    pub id: String,
    /// mock recipients the message was stored for
    pub delivered: Vec<String>,
    /// recipients that received nothing, with the reason
    pub not_delivered: Vec<NotDelivered>,
}

#[derive(Debug, Serialize)]
pub struct NotDelivered {
    pub address: String,
    pub reason: &'static str,
}

pub const MAX_RECIPIENTS: usize = 50;

pub async fn send(
    state: &Arc<WebState>,
    caller: &MockCaller,
    ns: &str,
    req: SendRequest,
) -> MockApiResult<SendResult> {
    authorize(state, caller, ns).await?;
    let bad = |m: &str| MockApiError::new(StatusCode::BAD_REQUEST, m);
    let to: Vec<String> = req
        .to
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if to.is_empty() || to.len() > MAX_RECIPIENTS {
        return Err(bad("between 1 and 50 recipients"));
    }
    if to
        .iter()
        .any(|a| !a.contains('@') || a.contains(['\r', '\n', ',', '<', '>']))
    {
        return Err(bad("a recipient is a bare address such as a@b.example"));
    }
    let from_tag = req
        .from_tag
        .as_deref()
        .unwrap_or("sender")
        .trim()
        .to_ascii_lowercase();
    if !mailrs_mock::address::validate_tag(&from_tag) {
        return Err(bad(
            "from_tag is up to 31 characters of a-z, 0-9, '.', '_' and '-'",
        ));
    }
    let domain = mock::domain();
    let from = format!("{ns}.{from_tag}@{domain}");
    let id = mock::fresh_message_id();
    let mut builder = mailrs_mail_builder::MessageBuilder::new()
        .from(&from)
        .subject(req.subject.clone())
        .message_id(format!("<{id}@{domain}>"))
        .text_body(req.text.clone());
    for a in &to {
        builder = builder.to(a);
    }
    if let Some(html) = &req.html {
        builder = builder.html_body(html.clone());
    }
    let raw = builder.build();
    let summary = mailrs_mock::parse_message(&raw).summary;

    let (ns_owned, to_owned) = (ns.to_string(), to.clone());
    store(move |c| {
        let mut delivered = Vec::new();
        let mut not_delivered = Vec::new();
        let captured = MessageMeta {
            id: id.clone(),
            namespace: ns_owned.clone(),
            tag: from_tag.clone(),
            direction: Direction::CapturedOut,
            rcpt: to_owned.join(", "),
            size: 0,
            received_at: 0,
            uid: 0,
            summary: summary.clone(),
        };
        if mock::store_message(c, mock::root(), captured, &raw)? == Stored::Full {
            return Err(mock::MockError::StoreFull);
        }
        for (i, rcpt) in to_owned.iter().enumerate() {
            if let Some(sim) = mock::simulation(rcpt) {
                let now = now_ms() / 1000;
                let reply = mailrs_mock::simulation::reply(sim, domain, &from, rcpt, &raw, now);
                let answer = MessageMeta {
                    id: format!("{id}-{i}"),
                    namespace: ns_owned.clone(),
                    tag: sim.name().to_string(),
                    direction: Direction::In,
                    rcpt: from.clone(),
                    size: 0,
                    received_at: 0,
                    uid: 0,
                    summary: mailrs_mock::parse_message(&reply).summary,
                };
                mock::store_message(c, mock::root(), answer, &reply)?;
                match sim.delivered() {
                    true => delivered.push(rcpt.clone()),
                    false => not_delivered.push(NotDelivered {
                        address: rcpt.clone(),
                        reason: "simulated bounce; the report is in this namespace",
                    }),
                }
                continue;
            }
            let Some(addr) = mock::parse(rcpt) else {
                not_delivered.push(NotDelivered {
                    address: rcpt.clone(),
                    reason: match mock::is_mock(rcpt) {
                        true => "not a valid mock address",
                        false => "not a mock address; captured, never sent",
                    },
                });
                continue;
            };
            let copy = MessageMeta {
                id: format!("{id}-{i}"),
                namespace: addr.namespace,
                tag: addr.tag,
                direction: Direction::In,
                rcpt: rcpt.to_ascii_lowercase(),
                size: 0,
                received_at: 0,
                uid: 0,
                summary: summary.clone(),
            };
            match mock::store_message(c, mock::root(), copy, &raw)? {
                Stored::Stored | Stored::AlreadyStored => delivered.push(rcpt.clone()),
                Stored::NoNamespace => not_delivered.push(NotDelivered {
                    address: rcpt.clone(),
                    reason: "no such namespace",
                }),
                Stored::Full => not_delivered.push(NotDelivered {
                    address: rcpt.clone(),
                    reason: "mock store full",
                }),
            }
        }
        Ok(SendResult {
            id,
            delivered,
            not_delivered,
        })
    })
}
