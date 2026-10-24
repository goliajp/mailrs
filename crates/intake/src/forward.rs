//! The local forwards of a mailbox: once a message is in `mailbox`, a copy
//! goes straight into the mailbox of each account it forwards to, through
//! that account's own sieve and under the verdict the message already
//! carries. One hop only: a forwarded copy is not forwarded again, so two
//! accounts forwarding to each other cannot loop.

use mailrs_core_sidestate::families::local_forward;

use crate::drain::{Core, Inbound};
use crate::maildir::ensure_maildir;
use crate::route::is_mailbox;

impl<C: Core + ?Sized> Inbound<'_, C> {
    /// How many forwarded copies were placed. A store that cannot be read
    /// costs the forwarded copies, never the one already in `mailbox`.
    pub(crate) fn forward_locally(&self, mailbox: &str) -> usize {
        let Some(mut conn) = self.core.net_conn() else {
            return 0;
        };
        let targets = match local_forward::targets(&mut conn, mailbox) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(%mailbox, error = %e, "local forwards unreadable; not forwarded");
                return 0;
            }
        };
        let mut placed = 0;
        for target in targets.iter().filter(|t| !t.eq_ignore_ascii_case(mailbox)) {
            match is_mailbox(self.core, target) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(%mailbox, %target, "local forward target is not a mailbox here");
                    continue;
                }
                Err(e) => {
                    tracing::warn!(%mailbox, %target, error = %e, "local forward target lookup failed");
                    continue;
                }
            }
            if ensure_maildir(self.core.maildir_root(), target) && self.place(target) {
                tracing::info!(%mailbox, %target, "local forward delivered");
                placed += 1;
            } else {
                tracing::warn!(%mailbox, %target, "local forward not delivered");
            }
        }
        placed
    }
}
