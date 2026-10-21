//! Which domains this core treats as ours: `MAILRS_LOCAL_DOMAINS` together
//! with the domain store the admin Domains page writes.

use std::sync::Arc;

pub use mailrs_intake::domains::LocalDomainsCache;

use crate::FastcoreState;

pub(super) fn local_domains(state: &FastcoreState) -> Arc<Vec<String>> {
    state.local_domains.get(|| {
        state
            .mailbox
            .list_domains()
            .map(|rows| rows.into_iter().map(|(name, _)| name).collect())
    })
}
