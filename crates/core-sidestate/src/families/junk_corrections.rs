//! A reader's correction of the server's Junk: the observations written
//! when a conversation comes out of Junk, and the conversations a reader
//! filed as Junk themselves, read back from the observations.
//!
//! Facts in `rep:obs` beside the screening ones (see [`super::screening`]):
//! added, never changed.

use mailrs_reputation::Observation;

use super::screening::{add_observation, observations, withdraw_user_junk};

pub(super) fn not_junk_reason(thread_id: &str) -> String {
    format!("took conversation {thread_id} out of Junk")
}

/// `user` took `thread_id` out of Junk where the server had put it: one
/// `UserNotJunk` vouch per sender domain, unless that user already
/// vouched for it from the same conversation. Returns how many were added.
///
/// A vouch is a reader's correction of the server, kept as evidence; it
/// exempts nothing (see [`mailrs_reputation::Stance::Vouch`]).
///
/// # Errors
/// The store could not be read or written.
pub fn vouch_not_junk(
    conn: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
    subjects: &mailrs_inbound::scan_input::JunkSubjects,
    now: i64,
) -> std::io::Result<usize> {
    use mailrs_reputation::{Kind, Source, Stance};
    let all = observations(conn)?;
    let reason = not_junk_reason(thread_id);
    let mut added = 0;
    for domain in &subjects.senders {
        let have = all.iter().any(|o| {
            o.source == Source::UserNotJunk
                && o.actor.as_deref() == Some(user)
                && o.reason == reason
                && &o.value == domain
        });
        if have {
            continue;
        }
        add_observation(
            conn,
            Observation {
                id: String::new(),
                at: now,
                source: Source::UserNotJunk,
                kind: Kind::SenderDomain,
                value: domain.clone(),
                stance: Stance::Vouch,
                retracts: None,
                reason: reason.clone(),
                actor: Some(user.to_string()),
            },
        )?;
        added += 1;
    }
    Ok(added)
}

/// `user` took `thread_id` out of Junk, whoever had put it there. When it
/// was the user, the marks their filing made are retracted; when it was
/// the server, there is nothing of theirs to retract and the correction
/// is recorded as a vouch. Returns how many observations were added.
///
/// # Errors
/// The store could not be read or written.
pub fn record_not_junk(
    conn: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
    raws: &[Vec<u8>],
    now: i64,
) -> std::io::Result<usize> {
    match withdraw_user_junk(conn, user, thread_id, now)? {
        0 => {
            let subjects = mailrs_inbound::scan_input::junk_subjects(raws, user);
            vouch_not_junk(conn, user, thread_id, &subjects, now)
        }
        n => Ok(n),
    }
}

/// The conversations `user` filed as Junk and has not taken out again:
/// those with a live `UserJunk` mark of theirs. Read from `all`, every
/// observation.
pub fn user_junked_threads(all: &[Observation], user: &str) -> std::collections::BTreeSet<String> {
    let retracted: std::collections::HashSet<&str> = all
        .iter()
        .filter(|o| o.stance == mailrs_reputation::Stance::Retract)
        .filter_map(|o| o.retracts.as_deref())
        .collect();
    all.iter()
        .filter(|o| {
            o.source == mailrs_reputation::Source::UserJunk
                && o.stance == mailrs_reputation::Stance::Deny
                && o.actor.as_deref() == Some(user)
                && !retracted.contains(o.id.as_str())
        })
        .filter_map(|o| {
            o.reason
                .strip_prefix("moved conversation ")
                .and_then(|r| r.strip_suffix(" to Junk"))
                .map(str::to_string)
        })
        .collect()
}

#[cfg(test)]
#[path = "junk_corrections_tests.rs"]
mod tests;
