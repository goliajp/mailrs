//! This core as the shared intake routes see it.

use std::sync::Arc;

use mailrs_core_routes::store::IntakeStore;

use crate::core_rpc::CoreRpcState;
use crate::intake::{PgCore, PgDirectory};

impl IntakeStore for CoreRpcState {
    type Core = PgCore;

    /// Built on a runtime thread: the directory keeps its handle.
    fn intake_core(self: &Arc<Self>) -> PgCore {
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
