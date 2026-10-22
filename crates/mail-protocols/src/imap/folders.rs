//! CREATE, DELETE, RENAME, SUBSCRIBE and UNSUBSCRIBE. Every account has
//! the same fixed set of folders, and every one of them counts as
//! subscribed.

use mailrs_imap_proto::{format_no, format_ok};

use super::session::{State, no};
use crate::backend::Account;

/// CREATE, DELETE and RENAME (RFC 5530 `CANNOT`).
pub(super) fn fixed<A: Account>(session: &State<A>, tag: &str) -> Vec<String> {
    match session.account() {
        Some(_) => vec![format_no(
            tag,
            "[CANNOT] folders cannot be created, deleted or renamed",
        )],
        None => vec![format_no(tag, "not authenticated")],
    }
}

/// SUBSCRIBE / UNSUBSCRIBE (`verb`) of a folder that exists changes
/// nothing and succeeds.
pub(super) async fn subscription<A: Account>(
    session: &State<A>,
    tag: &str,
    mailbox: &str,
    verb: &str,
) -> Vec<String> {
    let Some(account) = session.account() else {
        return vec![format_no(tag, "not authenticated")];
    };
    match account.mailbox(mailbox).await {
        Ok(Some(_)) => vec![format_ok(tag, &format!("{verb} completed"))],
        Ok(None) => vec![format_no(tag, "[NONEXISTENT] no such mailbox")],
        Err(e) => vec![no(tag, verb, &e)],
    }
}
