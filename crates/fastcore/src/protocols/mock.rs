//! IMAP over a mock namespace: `{ns}@{mock domain}` logs in with the
//! namespace's SMTP token and reads the namespace's mail as a read-only
//! INBOX.
//!
//! Only the mail addressed to the namespace is listed. Mail its sandbox
//! sent is kept in the same namespace, but an app that reads its INBOX
//! and answers what it finds would otherwise answer its own outbound
//! mail.

use std::path::PathBuf;
use std::sync::Arc;

use mailrs_core_sidestate::families::mock as store;
use mailrs_mail_protocols::backend::{Error, Message, Result, Selected};

use super::account::{FastcoreAccount, FcMailbox};
use crate::FastcoreState;
use crate::imap::backend::MailboxInfo;

const UNAVAILABLE: &str = "[UNAVAILABLE] mock store unreachable";

pub(crate) struct MockAccount {
    pub(crate) state: Arc<FastcoreState>,
    pub(crate) user: String,
    ns: String,
}

/// Only the namespace's token is checked: a username on the mock domain
/// never reaches the account store.
pub(super) async fn login(
    state: &Arc<FastcoreState>,
    username: &str,
    password: &str,
) -> Result<Option<FastcoreAccount>> {
    let Some(addr) = store::parse(username).filter(|a| a.tag.is_empty()) else {
        return Ok(None);
    };
    let st = state.clone();
    let ns = addr.namespace.clone();
    let token = password.to_string();
    let verified = tokio::task::spawn_blocking(move || {
        let mut conn = st.net_conn()?;
        store::verify_smtp_token(&mut conn, &ns, &token).ok()
    })
    .await
    .map_err(|e| Error::Store(e.to_string()))?;
    match verified {
        Some(true) => Ok(Some(FastcoreAccount::Mock(MockAccount {
            state: state.clone(),
            user: format!("{}@{}", addr.namespace, store::domain()),
            ns: addr.namespace,
        }))),
        Some(false) => Ok(None),
        None => Err(Error::Refused(UNAVAILABLE.into())),
    }
}

impl MockAccount {
    pub(crate) fn mailbox(&self, name: &str) -> Result<FcMailbox> {
        if !name.eq_ignore_ascii_case("INBOX") {
            return Err(Error::Refused(
                "[NONEXISTENT] a mock namespace has only INBOX".into(),
            ));
        }
        Ok(MailboxInfo {
            name: "INBOX".into(),
            path: store::root().join(&self.ns),
        })
    }

    /// A QRESYNC parameter list has nothing to report: nothing is ever
    /// expunged by a client.
    pub(crate) async fn select(&self) -> Result<Selected<PathBuf>> {
        let st = self.state.clone();
        let ns = self.ns.clone();
        let loaded = tokio::task::spawn_blocking(move || load(&st, &ns))
            .await
            .map_err(|e| Error::Store(e.to_string()))?;
        let (uidvalidity, uidnext, messages) =
            loaded.ok_or_else(|| Error::Refused(UNAVAILABLE.into()))?;
        Ok(Selected {
            uidvalidity,
            uidnext,
            highest_modseq: 1,
            recent: 0,
            messages,
        })
    }
}

/// UIDVALIDITY and the inbound messages of `ns`, ascending by uid.
/// UIDVALIDITY is the creation second, so a namespace deleted and
/// created again under the same name invalidates what clients cached.
fn load(state: &FastcoreState, ns: &str) -> Option<(u32, u32, Vec<Message<PathBuf>>)> {
    let mut conn = state.net_conn()?;
    let info = store::get_namespace(&mut conn, ns).ok()??;
    let all = store::Filter {
        limit: usize::MAX,
        ..Default::default()
    };
    let mut metas = store::list_messages(&mut conn, ns, &all).ok()?;
    // uid 0: stored before uids were handed out, not addressable over IMAP
    metas.retain(|m| m.direction == store::Direction::In && m.uid != 0);
    metas.sort_by_key(|m| m.uid);
    let messages = metas
        .into_iter()
        .enumerate()
        // `size` is the length of the bytes the file was written with
        .map(|(i, m)| Message {
            uid: m.uid,
            seqno: i as u32 + 1,
            key: store::message_path(store::root(), ns, &m.id),
            flags: Vec::new(),
            internal_date: m.received_at / 1000,
            size: m.size as u64,
            modseq: 1,
        })
        .collect();
    let uidvalidity = u32::try_from(info.created_at / 1000)
        .unwrap_or(u32::MAX)
        .max(1);
    // from the counter, not the newest message: UIDNEXT must not go down
    // when that message is deleted or expires
    let uidnext = store::last_uid(&mut conn, ns).ok()?.saturating_add(1);
    Some((uidvalidity, uidnext, messages))
}
