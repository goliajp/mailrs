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

/// One of `user`'s messages as the section reads it, from its file: when
/// it sorts, whether the user wrote it, its facts, and its sender's bare
/// address, lowercased. Read the way delivery reads it, except that the
/// file cannot say what the contact history was when it arrived: it is
/// never a first contact, and `mutual` is read from the counts on `net`
/// now, which also count the mail that came after it.
pub(crate) struct ReadArrival {
    pub at: i64,
    pub own: bool,
    pub facts: MessageFacts,
    pub from_addr: String,
}

pub(crate) fn read_arrival(
    user: &str,
    blob_ref: &str,
    raw: &[u8],
    category: &str,
    net: Option<&mut kevy_client::Connection>,
) -> ReadArrival {
    let head = &raw[..raw.len().min(16 * 1024)];
    // the headers and the time delivery recorded, so a refold recognises
    // the message delivery folded and keeps what it read then
    let (_, _, _, _, date, from, to) = mailrs_intake::headers::extract_headers(head);
    let at = mailrs_intake::headers::stored_date(date, blob_ref);
    let own = mailrs_mailbox::sent_copy::sent_by(&from, blob_ref, user);
    let mut facts =
        mailrs_intake::section_facts::facts(None, user, &from, &to, head, category, own);
    if !own {
        facts.invite = mailrs_intake::invite::find(raw).is_some();
        facts.mutual = net.is_some_and(|c| mailrs_intake::section_facts::mutual(c, user, &from));
    }
    ReadArrival {
        at,
        own,
        facts,
        from_addr: first_address(&from).to_ascii_lowercase(),
    }
}

/// [`SectionArrival`]s borrowing from `read`, for `user`'s thread.
pub(crate) fn as_arrivals<'a>(
    user: &'a str,
    thread_id: &'a str,
    read: &'a [ReadArrival],
) -> Vec<mailrs_mailbox_kevy::SectionArrival<'a>> {
    read.iter()
        .map(|r| mailrs_mailbox_kevy::SectionArrival {
            user,
            thread_id,
            at: r.at,
            is_own: r.own,
            facts: r.facts,
            from_addr: &r.from_addr,
        })
        .collect()
}

/// Decide `user`'s section for the thread again from the messages left in
/// it, each read from its file. For when a message went: a fold only moves
/// forward and cannot take one back. `home` is the user's Maildir.
pub(crate) fn refold(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    home: &std::path::Path,
) {
    let category = state
        .mailbox
        .get_thread_for_user(user, thread_id)
        .ok()
        .flatten()
        .map(|r| r.category)
        .unwrap_or_default();
    let mut net = state.net_conn();
    let mut read = Vec::new();
    for mid in state
        .mailbox
        .user_thread_message_ids(user, thread_id)
        .unwrap_or_default()
    {
        let Some(copy) = state.mailbox.user_message_facts(user, &mid).ok().flatten() else {
            continue;
        };
        let Some(raw) = mailrs_maildir::locate(home, &copy.blob_ref)
            .and_then(|(dir, id)| dir.fetch(&id).ok().flatten())
        else {
            continue;
        };
        read.push(read_arrival(
            user,
            &copy.blob_ref,
            &raw,
            &category,
            net.as_mut(),
        ));
    }
    let arrivals = as_arrivals(user, thread_id, &read);
    if let Err(e) = state.mailbox.refold_section(user, thread_id, &arrivals) {
        tracing::warn!(error = %e, %user, %thread_id, "section not decided again");
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
