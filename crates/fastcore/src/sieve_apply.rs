//! Sieve script application for the spool drain.
//!
//! For each spool file whose recipient resolves to a local maildir we
//! look up the recipient's `sieve:<address>` script in the network
//! kevy, populated by webapi's `PUT /api/admin/accounts/{addr}/sieve`
//! (and the ManageSieve session on port 4190 once that comes back).
//! When one is set, we compile it and evaluate against the raw message
//! bytes; the resulting actions become a delivery decision.
//!
//! Current coverage vs [`SieveAction`]:
//! - `Keep` → default deliver to INBOX (unchanged)
//! - `FileInto` → deliver to `<maildir>/.<folder>/new/`
//! - `Discard` → mark delivered without writing (silent drop)
//! - `Redirect` → enqueue outbound to the target, drop original
//! - `Reject` → log + fallback to Keep (DSN generation is a follow-up)
//! - `Vacation` → log + fallback to Keep (auto-reply dedup is a follow-up)

use kevy_client::Connection;
use mailrs_sieve::{SieveAction, compile_sieve, evaluate_sieve_with_envelope};

/// The delivery decision the drain applies after consulting sieve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Deliver to INBOX/{new} — the default when no sieve applies.
    Keep,
    /// Deliver into `<maildir>/.<folder>/{new}`. The folder string is
    /// verbatim from the script; the caller sanitises path separators.
    FileInto(String),
    /// Drop the file silently. `Ok(true)` back to the drain loop.
    Discard,
    /// Enqueue an outbound relay to the target address before dropping
    /// the local file.
    Redirect(String),
}

/// Load + compile + evaluate the sieve script for `recipient` against
/// the raw message. Returns `Decision::Keep` when there's no script,
/// the script fails to compile, or no action fires.
///
/// Runs sync — spool_drain is already sync, and network kevy calls
/// through kevy-client are blocking. Called at most a handful of
/// times per drain tick, so no need for pooling.
pub fn decide(recipient: &str, raw_message: &[u8], envelope_from: Option<&str>) -> Decision {
    let Some(script) = fetch_script(recipient) else {
        return Decision::Keep;
    };
    let compiled = match compile_sieve(&script) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(recipient, error = %e, "sieve: compile failed; keeping default delivery");
            return Decision::Keep;
        }
    };
    let actions =
        evaluate_sieve_with_envelope(&compiled, raw_message, envelope_from, Some(recipient));
    action_to_decision(&actions, recipient)
}

/// Reduce a script's action list to a single delivery decision. Sieve
/// scripts may emit multiple actions (e.g. `fileinto` + `keep`); we
/// honor the strongest one in this order:
/// Discard > Redirect > Reject/Vacation > FileInto > Keep.
fn action_to_decision(actions: &[SieveAction], recipient: &str) -> Decision {
    if actions.iter().any(|a| matches!(a, SieveAction::Discard)) {
        return Decision::Discard;
    }
    if let Some(target) = actions.iter().find_map(|a| match a {
        SieveAction::Redirect(t) => Some(t.clone()),
        _ => None,
    }) {
        return Decision::Redirect(target);
    }
    if actions
        .iter()
        .any(|a| matches!(a, SieveAction::Reject(_) | SieveAction::Vacation { .. }))
    {
        tracing::warn!(
            recipient,
            "sieve: Reject / Vacation not yet handled — falling back to Keep so mail isn't lost"
        );
        return Decision::Keep;
    }
    if let Some(folder) = actions.iter().find_map(|a| match a {
        SieveAction::FileInto { mailbox, .. } => Some(mailbox.clone()),
        _ => None,
    }) {
        return Decision::FileInto(folder);
    }
    Decision::Keep
}

/// Read `sieve:<address>` from the network kevy. Empty / unset → None.
fn fetch_script(address: &str) -> Option<String> {
    let url = std::env::var("MAILRS_KEVY_URL").ok()?;
    let mut conn = Connection::open(&url).ok()?;
    let key = format!("sieve:{address}");
    let raw = conn.get(key.as_bytes()).ok().flatten()?;
    let script = String::from_utf8(raw).ok()?;
    if script.trim().is_empty() {
        return None;
    }
    Some(script)
}

/// Turn a Maildir++ folder name from `fileinto` into a safe
/// disk-path segment. Dot-prefixed, and any embedded `/` is replaced
/// with `.` per the Maildir++ hierarchy convention.
pub fn maildir_subfolder(folder: &str) -> String {
    let cleaned = folder.trim().replace('/', ".").replace(['\\', '\0'], "");
    if cleaned.starts_with('.') {
        cleaned
    } else {
        format!(".{cleaned}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_when_no_actions() {
        assert_eq!(action_to_decision(&[], "a@x"), Decision::Keep);
    }

    #[test]
    fn discard_beats_all() {
        let a = [
            SieveAction::FileInto {
                mailbox: "Spam".into(),
                flags: vec![],
            },
            SieveAction::Discard,
        ];
        assert_eq!(action_to_decision(&a, "a@x"), Decision::Discard);
    }

    #[test]
    fn redirect_returns_redirect() {
        let a = [SieveAction::Redirect("b@y".into())];
        assert_eq!(
            action_to_decision(&a, "a@x"),
            Decision::Redirect("b@y".into())
        );
    }

    #[test]
    fn fileinto_returns_folder() {
        let a = [SieveAction::FileInto {
            mailbox: "Work".into(),
            flags: vec![],
        }];
        assert_eq!(
            action_to_decision(&a, "a@x"),
            Decision::FileInto("Work".into())
        );
    }

    #[test]
    fn reject_falls_back_to_keep() {
        let a = [SieveAction::Reject("no thanks".into())];
        assert_eq!(action_to_decision(&a, "a@x"), Decision::Keep);
    }

    #[test]
    fn vacation_falls_back_to_keep() {
        let a = [SieveAction::Vacation {
            reason: "OOO".into(),
            subject: None,
            from: None,
            handle: None,
            period_secs: None,
            addresses: vec![],
            mime: false,
        }];
        assert_eq!(action_to_decision(&a, "a@x"), Decision::Keep);
    }

    #[test]
    fn subfolder_maps_slashes_to_dots() {
        assert_eq!(maildir_subfolder("Work/Client"), ".Work.Client");
    }

    #[test]
    fn subfolder_preserves_leading_dot() {
        assert_eq!(maildir_subfolder(".Junk"), ".Junk");
    }

    #[test]
    fn subfolder_strips_backslash_and_nul() {
        assert_eq!(maildir_subfolder("Work\\x\0y"), ".Workxy");
    }
}
