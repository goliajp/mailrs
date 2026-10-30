//! IMAP, POP3 and ManageSieve over the SQL store: the sessions are
//! `mailrs-mail-protocols`, the same ones fastcore serves; this is the
//! storage they run on — accounts in [`DomainStore`] (then `users.toml`,
//! then LDAP), mailboxes and messages in [`PgMailboxStore`], bodies in the
//! Maildir, Sieve scripts in the network kevy.

mod account;
#[cfg(test)]
mod tests;
mod watch;

use std::sync::Arc;

use mailrs_mail_protocols::backend::{Directory, Error, Protocol, Result};
use mailrs_mailbox::PgMailboxStore;

use crate::domain_store::DomainStore;
use crate::event_bus::EventBus;
use crate::ldap_auth::LdapConfig;
use crate::message_store::MessageStore;
use crate::users::UserStore;
pub(crate) use account::PgAccount;

pub(crate) struct PgDirectory {
    pub(crate) mailbox: Arc<PgMailboxStore>,
    pub(crate) domain: Option<Arc<DomainStore>>,
    pub(crate) users: Arc<UserStore>,
    pub(crate) ldap: Option<Arc<LdapConfig>>,
    pub(crate) message_store: Arc<dyn MessageStore>,
    pub(crate) events: EventBus,
    pub(crate) maildir_root: String,
    pub(crate) hostname: String,
    /// `MAILRS_KEVY_URL`, where Sieve scripts are kept.
    pub(crate) net_url: Option<String>,
}

impl PgDirectory {
    /// The account row's hash first, then `users.toml`, then LDAP. An
    /// inactive account, or one without a hash, signs in only through
    /// LDAP.
    async fn verify(&self, user: &str, password: &str) -> Result<bool> {
        let row = match &self.domain {
            Some(ds) => ds
                .get_account_with_hash(user)
                .await
                .map_err(|e| Error::Store(e.to_string()))?,
            None => None,
        };
        let by_row = match row {
            Some((account, hash)) => account.active && UserStore::verify_hash(password, &hash),
            None => {
                // the same argon2 work as a real account takes
                crate::users::dummy_verify(password);
                self.users.verify(user, password)
            }
        };
        if by_row {
            return Ok(true);
        }
        Ok(match &self.ldap {
            Some(ldap) => ldap.authenticate(user, password).await,
            None => false,
        })
    }
}

impl Directory for PgDirectory {
    type Account = PgAccount;

    async fn password_cleared(&self, account: &str) -> bool {
        let Some(domain) = &self.domain else {
            return false;
        };
        matches!(
            domain.get_account_with_hash(account).await,
            Ok(Some((a, _))) if a.password_cleared_at.is_some()
        )
    }

    fn hostname(&self) -> String {
        self.hostname.clone()
    }

    async fn login(&self, user: &str, password: &str, _: Protocol) -> Result<Option<PgAccount>> {
        if !self.verify(user, password).await? {
            return Ok(None);
        }
        self.mailbox
            .ensure_default_mailboxes(user)
            .await
            .map_err(|e| Error::Store(e.to_string()))?;
        Ok(Some(PgAccount::new(self, user)))
    }
}
