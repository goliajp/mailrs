//! `archived` and `pinned`, written where they cannot be rebuilt away.
//!
//! Step 4 of `.claude/rfcs/20260814-the-maildir-is-the-store.md`. Both are
//! a person's decisions, so by the rule in §1 they live next to the mail —
//! as Maildir++ keyword bits in each message's `:2,` suffix, with
//! `mailrs-keywords` saying which letter means what. They stay on the
//! membership row as well, because that is the index the conversation list
//! reads; what changes is which of the two is the authority.
//!
//! **Both directions, in the same change.** The verb writes the bit; the
//! self-heal reads it back onto the row. A file written and never read is
//! the `one-side-of-the-wire` shape, and a bit set by a verb that a
//! rebuild cannot see is exactly the loss this step exists to stop.
//!
//! The store may not touch the filesystem, so the split is the one A3
//! settled: `mailbox` reports which files are involved, and this renames
//! them.

use std::sync::Arc;

use crate::FastcoreState;

/// The keyword mailrs writes for "the user archived this".
pub(crate) const ARCHIVED: &str = "archived";
/// The keyword mailrs writes for "the user pinned this".
pub(crate) const PINNED: &str = "pinned";

use crate::maildir_scan::mailbox_dir;

/// Read a mailbox's keyword map, or an empty one.
pub(crate) fn load(user: &str) -> mailrs_keywords::Keywords {
    let Some(dir) = mailbox_dir(user) else {
        return mailrs_keywords::Keywords::new();
    };
    mailrs_keywords::read(&dir).unwrap_or_else(|e| {
        tracing::warn!(err = %e, %user, "keywords unreadable — treating as none");
        mailrs_keywords::Keywords::new()
    })
}

/// The letter for `name` in this mailbox, assigning one if it is new.
///
/// Writes the file when it assigns, because the letter is meaningless
/// without it: a message carrying an unrecorded bit is a fact nobody can
/// read back.
fn letter_for(user: &str, name: &str) -> Option<char> {
    let dir = mailbox_dir(user)?;
    let mut kw = load(user);
    if let Some(c) = kw.letter(name) {
        return Some(c);
    }
    let c = kw.intern(name)?;
    if let Err(e) = mailrs_keywords::write(&dir, &kw) {
        // Without the file the bit cannot be read back, so do not write it.
        tracing::warn!(err = %e, %user, %name, "keyword file write failed");
        return None;
    }
    Some(c)
}

/// Set or clear `name` on every message `user` holds in `thread_id`.
///
/// Per message, because that is where a Maildir++ keyword lives, and per
/// user, because the thread is shared and the decision is not.
pub(crate) fn set_on_thread(
    state: &Arc<FastcoreState>,
    user: &str,
    thread_id: &str,
    name: &str,
    on: bool,
) {
    let Some(letter) = letter_for(user, name) else {
        return;
    };
    let Some(dir) = mailbox_dir(user) else {
        return;
    };
    let md_root = dir;
    for mid in state
        .mailbox
        .user_thread_message_ids(user, thread_id)
        .unwrap_or_default()
    {
        let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
            continue;
        };
        if facts.blob_ref.is_empty() {
            continue;
        }
        let Some((md, id)) = mailrs_maildir::locate(&md_root, &facts.blob_ref) else {
            continue;
        };
        let (Ok(Some(flags)), Ok(Some(current))) = (md.flags_of(&id), md.keywords_of(&id)) else {
            continue;
        };
        let has = current.contains(&letter);
        if has == on {
            continue;
        }
        let mut want = current;
        if on {
            want.push(letter);
        } else {
            want.retain(|c| *c != letter);
        }
        if let Err(e) = md.mark_processed_with_keywords(&id, &flags, &want) {
            tracing::warn!(err = %e, %user, %thread_id, %name, "keyword rename failed");
        }
    }
}

/// Whether a scanned file carries `name`, given the mailbox's map.
pub(crate) fn file_has(keywords: &mailrs_keywords::Keywords, letters: &[char], name: &str) -> bool {
    keywords.letter(name).is_some_and(|c| letters.contains(&c))
}

/// Whether **any** message `user` holds in `thread_id` carries `name`.
///
/// Per thread, because that is the granularity the decision was made at,
/// and any-of because the bit is written to every message in the thread —
/// so one surviving bit is the decision, and requiring all of them would
/// lose it to a single message that arrived after the archive.
///
/// Reads the files rather than the scan, so a caller outside the sweep can
/// ask. `None` when the mailbox does not name the keyword at all: a bit
/// nobody has a meaning for is not a `false`, it is a question that cannot
/// be asked.
pub(crate) fn thread_has(
    state: &Arc<FastcoreState>,
    keywords: &mailrs_keywords::Keywords,
    user: &str,
    thread_id: &str,
    name: &str,
) -> Option<bool> {
    let letter = keywords.letter(name)?;
    let md_root = mailbox_dir(user)?;
    let mut saw_a_file = false;
    for mid in state
        .mailbox
        .user_thread_message_ids(user, thread_id)
        .unwrap_or_default()
    {
        let Ok(Some(facts)) = state.mailbox.user_message_facts(user, &mid) else {
            continue;
        };
        let Some((md, id)) = mailrs_maildir::locate(&md_root, &facts.blob_ref) else {
            continue;
        };
        let Ok(Some(letters)) = md.keywords_of(&id) else {
            continue;
        };
        saw_a_file = true;
        if letters.contains(&letter) {
            return Some(true);
        }
    }
    // No file read means nothing was asked, which is not the same as "no".
    saw_a_file.then_some(false)
}
