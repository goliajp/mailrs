//! The sectioned inbox in this core: recording each message's facts on
//! the reader's row, and reading the section back onto the list rows.

use std::sync::Arc;

use mailrs_core_api::types::ConversationSummaryWire;
use mailrs_core_sidestate::families::correspondent::first_address;
use mailrs_mailbox_kevy::section::MessageFacts;

use crate::FastcoreState;

/// Fold one message into `user`'s section for the thread. A failure is
/// logged and delivery goes on: the section is a derivation the backfill
/// can rebuild, and the mail must not wait on it.
pub(crate) fn record(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    at: i64,
    is_own: bool,
    facts: MessageFacts,
    from: &str,
) {
    let from_addr = first_address(from).to_ascii_lowercase();
    let arrival = mailrs_mailbox_kevy::SectionArrival {
        user,
        thread_id,
        at,
        is_own,
        facts,
        from_addr: &from_addr,
    };
    if let Err(e) = state.mailbox.record_section_arrival(&arrival) {
        tracing::warn!(error = %e, %user, %thread_id, "section not recorded");
    }
}

/// Each row's section and the time it started waiting, read off the
/// reader's own membership row.
pub(crate) fn with_sections(
    state: &FastcoreState,
    user: &str,
    mut items: Vec<ConversationSummaryWire>,
) -> Vec<ConversationSummaryWire> {
    for item in &mut items {
        let (section, since) = state
            .mailbox
            .section_fields(user, &item.thread_id)
            .unwrap_or_default();
        item.section = section;
        item.awaiting_since = since;
    }
    items
}

/// `POST /v1/users/{user}/senders:accept`: the acceptance is recorded on
/// each contact (the network store, which both cores' delivery reads),
/// then the conversations in the New section from these senders are
/// decided again.
pub(crate) async fn accept_senders(
    axum::extract::State(state): axum::extract::State<Arc<FastcoreState>>,
    axum::extract::Path(user): axum::extract::Path<String>,
    axum::Json(req): axum::Json<mailrs_core_api::method::senders::AcceptSendersRequest>,
) -> Result<
    axum::Json<mailrs_core_api::method::senders::AcceptSendersResponse>,
    axum::http::StatusCode,
> {
    use axum::http::StatusCode;
    let addresses = mailrs_core_api::method::senders::normalized(&req.addresses);
    let Some(mut conn) = state.net_conn() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    for address in &addresses {
        mailrs_core_sidestate::families::contacts::accept(
            &mut conn,
            &user,
            address,
            req.accepted_at,
        )
        .map_err(|e| {
            tracing::error!(error = %e, %user, "accepting a sender failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }
    state
        .mailbox
        .accept_senders(&user, &addresses)
        .map_err(|e| {
            tracing::error!(error = %e, %user, "re-deciding accepted senders' sections failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(axum::Json(
        mailrs_core_api::method::senders::AcceptSendersResponse {
            accepted: addresses.len() as u32,
        },
    ))
}

#[cfg(test)]
#[path = "sections_tests.rs"]
mod tests;
