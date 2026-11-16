//! This core as the shared intake routes see it.

use std::sync::Arc;

use mailrs_core_routes::store::IntakeStore;

use crate::core_rpc::CoreRpcState;
use crate::intake::{PgCore, PgDirectory};

impl CoreRpcState {
    /// This core as the shared intake and re-judge code see it. Built on a
    /// runtime thread: the directory keeps its handle.
    pub(crate) fn pg_core(&self) -> PgCore {
        PgCore {
            dir: PgDirectory {
                authserv_id: self.authserv_id.clone(),
                ..PgDirectory::new(
                    self.domain.clone(),
                    self.maildir_root.clone(),
                    self.net_url.clone(),
                )
            },
            mailbox: self.mailbox.clone(),
            event_bus: self.event_bus.clone(),
        }
    }
}

impl IntakeStore for CoreRpcState {
    type Core = PgCore;

    fn intake_core(self: &Arc<Self>) -> PgCore {
        self.pg_core()
    }
}

impl mailrs_core_routes::rejudge::RejudgeHost for CoreRpcState {
    type Store = PgCore;

    fn rejudge_store(self: &Arc<Self>) -> Arc<PgCore> {
        Arc::new(self.pg_core())
    }
}
