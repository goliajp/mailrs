//! A transaction addressed only to mock addresses skips the antispam
//! pipeline, so none of its per-message writes (greylist triplets, reply
//! rotation, DMARC results, Bayes) see mock traffic. A transaction with any
//! real recipient is judged as a whole, mock copies included: one SMTP
//! reply cannot accept one recipient's copy and refuse another's.

use mailrs_core_sidestate::families::mock;
use mailrs_smtp_proto::EnhancedCode;
use mailrs_smtp_proto::response::Response;
use mailrs_smtp_proto::session::State;

use super::super::super::ConnectionContext;

pub(super) enum MockGate {
    /// some recipient is real, or the feature is off
    Mixed,
    /// every recipient is a mock address and at least one namespace exists
    AllMock,
    Refuse(Response),
}

/// The namespace whose sandbox this session logged in as. Only a namespace
/// token authenticates a mock-domain user, so the name is proof enough.
pub(super) fn sandbox_namespace(state: &State) -> Option<String> {
    let username = match state {
        State::Authenticated { username, .. } => Some(username),
        State::MailFrom { username, .. } | State::RcptTo { username, .. } => username.as_ref(),
        _ => None,
    }?;
    crate::smtp_session::credentials::sandbox_namespace(username)
}

/// A sandbox session's mail is all kept, whoever it is addressed to.
pub(super) async fn check(
    forward_paths: &[String],
    sandbox: bool,
    ctx: &ConnectionContext,
) -> MockGate {
    if sandbox {
        return MockGate::AllMock;
    }
    if forward_paths.is_empty() || !forward_paths.iter().all(|r| mock::is_mock(r)) {
        return MockGate::Mixed;
    }
    // a simulation address always exists; the drain decides who is answered
    if forward_paths.iter().any(|r| mock::simulation(r).is_some()) {
        return MockGate::AllMock;
    }
    let Some(client) = ctx.spam_lists_client.clone() else {
        return MockGate::AllMock;
    };
    let namespaces: Vec<String> = forward_paths
        .iter()
        .filter_map(|r| mock::parse(r).map(|a| a.namespace))
        .collect();
    let looked_up = tokio::task::spawn_blocking(move || {
        client.with_conn(|conn| {
            let mut any = false;
            for ns in &namespaces {
                any |= mock::namespace_exists(conn, ns).map_err(std::io::Error::other)?;
            }
            if !any {
                mock::count_rejected_unknown_ns(conn).map_err(std::io::Error::other)?;
                return Ok((false, false));
            }
            let full =
                mock::total_bytes(conn).map_err(std::io::Error::other)? >= mock::MAX_TOTAL_BYTES;
            Ok((true, full))
        })
    })
    .await;
    match looked_up {
        Ok(Ok((false, _))) => MockGate::Refuse(Response::new(
            550,
            Some(EnhancedCode {
                class: 5,
                subject: 1,
                detail: 1,
            }),
            "No such mock namespace",
        )),
        Ok(Ok((true, true))) => MockGate::Refuse(Response::new(
            452,
            Some(EnhancedCode {
                class: 4,
                subject: 3,
                detail: 1,
            }),
            "Mock mail storage is full",
        )),
        // an unreachable store is not a reason to refuse; the drain decides
        _ => MockGate::AllMock,
    }
}
