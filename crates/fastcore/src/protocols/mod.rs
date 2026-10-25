//! IMAP, POP3 and ManageSieve over fastcore's stores: the sessions are
//! `mailrs-mail-protocols`; this is the storage they run on — accounts in
//! the embedded kevy, mail in the Maildir, UIDs and modseqs in the
//! embedded kevy, Sieve scripts in the network kevy.
//!
//! A mock namespace address (`{ns}@{mock domain}`) signs in to IMAP only,
//! with the namespace's SMTP token, and reads its mail read-only.

mod account;
mod mock;
mod real;
mod sieve;
#[cfg(test)]
mod tests;
mod watch;

use std::path::PathBuf;
use std::sync::Arc;

use mailrs_core_sidestate::families::mock as mock_store;
use mailrs_mail_protocols::backend::{Directory, Protocol, Result};
use mailrs_tls_reload::TlsState;

use account::FastcoreAccount;

use crate::FastcoreState;
use crate::imap::backend;

/// Start the five listeners over `state`, mail under `MAILRS_MAILDIR`.
pub(crate) fn spawn(state: Arc<FastcoreState>, tls: Option<TlsState>) {
    let dir = FastcoreDirectory {
        state,
        maildir_root: backend::maildir_root(),
    };
    let url = std::env::var("MAILRS_KEVY_URL").ok();
    let guard = mailrs_auth_guard::kevy::guard_for(url.as_deref());
    let guarded = mailrs_mail_protocols::Guarded::new(Arc::new(dir), guard);
    mailrs_mail_protocols::spawn_all(Arc::new(guarded), tls);
}

/// The accounts IMAP, POP3 and ManageSieve sign in, over fastcore's
/// stores.
pub struct FastcoreDirectory {
    pub(crate) state: Arc<FastcoreState>,
    pub(crate) maildir_root: PathBuf,
}

impl FastcoreDirectory {
    /// Mail under `maildir_root` rather than `MAILRS_MAILDIR`.
    pub fn new(state: Arc<FastcoreState>, maildir_root: PathBuf) -> Self {
        Self {
            state,
            maildir_root,
        }
    }
}

impl Directory for FastcoreDirectory {
    type Account = FastcoreAccount;

    fn hostname(&self) -> String {
        self.state
            .mailbox
            .store_ref()
            .get(b"hostname")
            .ok()
            .flatten()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| "mailrs".into())
    }

    async fn login(
        &self,
        user: &str,
        password: &str,
        protocol: Protocol,
    ) -> Result<Option<FastcoreAccount>> {
        if protocol == Protocol::Imap && mock_store::is_mock(user) {
            return mock::login(&self.state, user, password).await;
        }
        if !backend::verify_password(&self.state, user, password) {
            return Ok(None);
        }
        Ok(Some(FastcoreAccount::Real(real::RealAccount {
            state: self.state.clone(),
            user: user.to_string(),
            maildir_root: self.maildir_root.clone(),
        })))
    }
}
