//! Mock recipients leave the drain here, before anything that belongs to
//! a real mailbox runs: no maildir, alias, report hooks, sieve, vacation or
//! ingest.

use mailrs_core_sidestate::families::mock::{self, Direction, MessageMeta, Stored};
use mailrs_mock::Simulation;

/// Who handed the message in: a local account that signed in, a mock
/// namespace's sandbox login, or an outside server.
#[derive(Clone, Copy)]
pub(super) enum Sender<'a> {
    Account(&'a str),
    Sandbox(&'a str),
    Outside,
}

impl<'a> Sender<'a> {
    pub(super) fn of(env: &'a mailrs_core::spool::SpoolEnvelope) -> Self {
        match env.is_authenticated {
            true => Self::Account(&env.reverse_path),
            false => Self::Outside,
        }
    }
}

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
///
/// A simulation address answers a signed-in local sender in their own
/// INBOX; from anyone else the message is dropped, so the deployment never
/// sends a bounce or an auto-reply to an outside address.
pub(super) fn divert(
    state: &crate::FastcoreState,
    from: Sender<'_>,
    rcpt: &str,
    rcpt_index: usize,
    spool_file: &str,
    body: &[u8],
) -> Diverted {
    if !mock::is_mock(rcpt) {
        return Diverted::No;
    }
    if let Some(sim) = mock::simulation(rcpt) {
        return simulate(state, from, sim, rcpt, body);
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
        uid: 0,
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

/// A sandbox login's spool file, whoever it is addressed to: `None` when
/// the file is not one, else whether it was stored and removed.
pub(super) fn drain_sandbox(
    state: &crate::FastcoreState,
    env: &mailrs_core::spool::SpoolEnvelope,
    spool_file: &str,
    path: &std::path::Path,
    body: &[u8],
) -> Option<bool> {
    let ns = env.sandbox_ns.as_deref()?;
    let captured = capture_sandbox(state, ns, spool_file, &env.forward_paths, body);
    Some(matches!(captured, Diverted::Done) && std::fs::remove_file(path).is_ok())
}

/// A message from a namespace's sandbox SMTP login: one captured copy in
/// that namespace, plus the usual copy for each mock recipient. Nothing
/// is delivered to a mailbox or relayed.
fn capture_sandbox(
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
        uid: 0,
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
        if let Diverted::Retry = divert(state, Sender::Sandbox(ns), &rcpt, i, spool_file, body) {
            return Diverted::Retry;
        }
    }
    Diverted::Done
}

fn simulate(
    state: &crate::FastcoreState,
    from: Sender<'_>,
    sim: Simulation,
    rcpt: &str,
    body: &[u8],
) -> Diverted {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let Some(mut conn) = state.net_conn() else {
        return Diverted::Retry;
    };
    let answered = match from {
        Sender::Outside => {
            tracing::info!(%rcpt, "mock: simulation address from outside; dropped");
            return Diverted::Done;
        }
        Sender::Account(sender) => {
            let reply =
                mailrs_mock::simulation::reply(sim, mock::domain(), sender, rcpt, body, now);
            crate::bounce_queue::enqueue_notice(&mut conn, sender, &reply)
                .map_err(|e| e.to_string())
        }
        Sender::Sandbox(ns) => {
            let sender = format!("{ns}@{}", mock::domain());
            let reply =
                mailrs_mock::simulation::reply(sim, mock::domain(), &sender, rcpt, body, now);
            let meta = MessageMeta {
                id: mock::fresh_message_id(),
                namespace: ns.to_string(),
                tag: sim.name().to_string(),
                direction: Direction::In,
                rcpt: sender,
                size: 0,
                received_at: 0,
                uid: 0,
                summary: mailrs_mock::parse_message(&reply).summary,
            };
            mock::store_message(&mut conn, mock::root(), meta, &reply)
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    };
    match answered {
        Ok(()) => {
            tracing::info!(%rcpt, simulation = sim.name(), "mock: simulated reply sent");
            Diverted::Done
        }
        Err(e) => {
            tracing::warn!(%rcpt, error = %e, "mock: simulated reply failed; will retry");
            Diverted::Retry
        }
    }
}
