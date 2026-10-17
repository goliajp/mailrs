//! Mock recipients leave the drain here, before anything that belongs to
//! a real mailbox runs: no maildir, alias, report hooks, sieve, vacation or
//! ingest.

use mailrs_core_sidestate::families::mock::{self, Direction, MessageMeta, Stored};

pub(super) enum Diverted {
    /// not a mock address
    No,
    /// stored, or dropped for good (no such namespace, store full)
    Done,
    /// the store could not be reached; keep the spool file for the next tick
    Retry,
}

/// Hand the copy for `rcpt` (already lowercased) to the mock store when it
/// is a mock address. The message id comes from the spool file and the
/// recipient's position in it, so a file drained twice stores one copy.
pub(super) fn divert(
    state: &crate::FastcoreState,
    rcpt: &str,
    rcpt_index: usize,
    spool_file: &str,
    body: &[u8],
) -> Diverted {
    if !mock::is_mock(rcpt) {
        return Diverted::No;
    }
    let Some(addr) = mock::parse(rcpt) else {
        tracing::info!(%rcpt, "mock: address outside the grammar; dropped");
        return Diverted::Done;
    };
    let Some(mut conn) = state.net_conn() else {
        return Diverted::Retry;
    };
    let meta = MessageMeta {
        id: mock::spool_message_id(spool_file, rcpt_index),
        namespace: addr.namespace,
        tag: addr.tag,
        direction: Direction::In,
        rcpt: rcpt.to_string(),
        size: 0,
        received_at: 0,
        summary: mailrs_mock::parse_message(body).summary,
    };
    let ns = meta.namespace.clone();
    match mock::store_message(&mut conn, mock::root(), meta, body) {
        Ok(Stored::Stored) => {
            tracing::info!(%rcpt, "mock: stored");
            Diverted::Done
        }
        Ok(Stored::AlreadyStored) => Diverted::Done,
        Ok(Stored::NoNamespace) => {
            tracing::info!(%rcpt, %ns, "mock: no such namespace; dropped");
            Diverted::Done
        }
        Ok(Stored::Full) => {
            tracing::warn!(%rcpt, "mock: store full; dropped");
            Diverted::Done
        }
        Err(e) => {
            tracing::warn!(%rcpt, error = %e, "mock: store failed; will retry");
            Diverted::Retry
        }
    }
}

/// A message from a namespace's sandbox SMTP login: one captured copy in
/// that namespace, plus the usual copy for each mock recipient. Nothing
/// is delivered to a mailbox or relayed.
pub(super) fn capture_sandbox(
    state: &crate::FastcoreState,
    ns: &str,
    spool_file: &str,
    forward_paths: &[String],
    body: &[u8],
) -> Diverted {
    let Some(mut conn) = state.net_conn() else {
        return Diverted::Retry;
    };
    let meta = MessageMeta {
        id: mock::spool_message_id(spool_file, usize::MAX),
        namespace: ns.to_string(),
        tag: "smtp".to_string(),
        direction: Direction::CapturedOut,
        rcpt: forward_paths.join(", "),
        size: 0,
        received_at: 0,
        summary: mailrs_mock::parse_message(body).summary,
    };
    match mock::store_message(&mut conn, mock::root(), meta, body) {
        Ok(_) => tracing::info!(%ns, rcpts = forward_paths.len(), "mock: sandbox mail captured"),
        Err(e) => {
            tracing::warn!(%ns, error = %e, "mock: sandbox capture failed; will retry");
            return Diverted::Retry;
        }
    }
    for (i, raw) in forward_paths.iter().enumerate() {
        let rcpt = super::canonical_recipient(raw);
        if let Diverted::Retry = divert(state, &rcpt, i, spool_file, body) {
            return Diverted::Retry;
        }
    }
    Diverted::Done
}
