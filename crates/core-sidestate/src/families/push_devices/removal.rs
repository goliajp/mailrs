//! How a device stops receiving pushes, other than by the cap.
//!
//! - A person removes it ([`remove`]) from another device: the removal is
//!   kept for [`REMOVED_KEPT`], and [`super::register`] refuses the device
//!   until its own person turns pushes on again. The apps register on their
//!   own (iOS on every launch, Android when it finds itself missing), and
//!   would otherwise put back what was just removed. A device removing
//!   itself, from the session that registered it, leaves no such record.
//! - Its session ends ([`drop_for_session`], [`drop_all`]): signing out,
//!   ending a session from the sessions list, a password change or reset.
//!   Nothing is kept: signing in again registers again.
//! - Its session expired after a week unused, which nothing is told of: the
//!   senders call [`drop_signed_out`] before each push and drop it then,
//!   the same way.
//!
//! The cap's evictions and the push services' 404 / 410 leave no record
//! either: the device did nothing a person asked to undo.

use std::collections::HashMap;
use std::time::Duration;

use kevy_client::HExpireCond;

use super::{Channel, evict, held, used_field};
use crate::keys::push::{removed_key, session_link_key, used_key};
use crate::keys::session::session_key_by_ref;

/// How long a removal keeps a device from registering itself back.
pub const REMOVED_KEPT: Duration = Duration::from_secs(30 * 24 * 3600);

pub(super) fn is_removed(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
) -> std::io::Result<bool> {
    let field = used_field(channel, id);
    Ok(conn
        .hget(removed_key(user).as_bytes(), field.as_bytes())?
        .is_some())
}

pub(super) fn forget_removal(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
) -> std::io::Result<()> {
    let field = used_field(channel, id);
    conn.hdel(removed_key(user).as_bytes(), &[field.as_bytes()])?;
    Ok(())
}

/// Note which session registered the device; one registered with an agent
/// key belongs to no session.
pub(super) fn link(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
    session: Option<&str>,
) -> std::io::Result<()> {
    let key = session_link_key(user);
    let field = used_field(channel, id);
    match session {
        Some(s) => {
            conn.hset(key.as_bytes(), &[(field.as_bytes(), s.as_bytes())])?;
        }
        None => {
            conn.hdel(key.as_bytes(), &[field.as_bytes()])?;
        }
    }
    Ok(())
}

/// Forget a device's link and last use once it is gone.
fn unlink(conn: &mut kevy_client::Connection, user: &str, field: &str) -> std::io::Result<()> {
    for key in [session_link_key(user), used_key(user)] {
        conn.hdel(key.as_bytes(), &[field.as_bytes()])?;
    }
    Ok(())
}

/// A person asked to stop pushes to `id` on `channel`, from the session
/// `by_session`. Unless that is the session that registered it, the device
/// is kept from registering itself back for [`REMOVED_KEPT`]. Whether it
/// was there.
///
/// # Errors
/// The store could not be read or written.
pub fn remove(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
    id: &str,
    by_session: Option<&str>,
    now: i64,
) -> std::io::Result<bool> {
    let field = used_field(channel, id);
    let linked = conn.hget(session_link_key(user).as_bytes(), field.as_bytes())?;
    if !evict(conn, user, channel, id)? {
        return Ok(false);
    }
    unlink(conn, user, &field)?;
    let by_itself = matches!((&linked, by_session), (Some(l), Some(s)) if l == s.as_bytes());
    if !by_itself {
        let key = removed_key(user);
        conn.hset(
            key.as_bytes(),
            &[(field.as_bytes(), now.to_string().as_bytes())],
        )?;
        conn.hexpire(
            key.as_bytes(),
            &[field.as_bytes()],
            REMOVED_KEPT,
            HExpireCond::Always,
        )?;
    }
    Ok(true)
}

/// The ids on `channel` a person removed within [`REMOVED_KEPT`].
///
/// # Errors
/// The store could not be read.
pub fn removed(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
) -> std::io::Result<Vec<String>> {
    let prefix = format!("{}:", channel.as_str());
    let mut out: Vec<String> = conn
        .hgetall(removed_key(user).as_bytes())?
        .chunks_exact(2)
        .filter_map(|p| {
            let field = String::from_utf8_lossy(&p[0]);
            field.strip_prefix(&prefix).map(str::to_string)
        })
        .collect();
    out.sort();
    Ok(out)
}

/// The session `session` ended: stop pushes to every device it registered.
/// How many went.
///
/// # Errors
/// The store could not be read or written.
pub fn drop_for_session(
    conn: &mut kevy_client::Connection,
    user: &str,
    session: &str,
) -> std::io::Result<usize> {
    let mut gone = 0;
    for pair in conn
        .hgetall(session_link_key(user).as_bytes())?
        .chunks_exact(2)
    {
        if pair[1] != session.as_bytes() {
            continue;
        }
        let field = String::from_utf8_lossy(&pair[0]).into_owned();
        let Some((channel, id)) = field
            .split_once(':')
            .and_then(|(c, id)| Some((Channel::parse(c)?, id)))
        else {
            continue;
        };
        if evict(conn, user, channel, id)? {
            gone += 1;
        }
        unlink(conn, user, &field)?;
    }
    Ok(gone)
}

/// Drop the devices on `channel` whose registering session no longer
/// exists, as if it had been ended: a session that expires after a week
/// unused ends with no one to call [`drop_for_session`]. A device linked to
/// no session (registered before devices were linked, or with an agent key)
/// is kept: no link is not a sign of an ended session. How many went.
///
/// One read of the links, then one existence check per distinct session.
///
/// # Errors
/// The store could not be read or written.
pub fn drop_signed_out(
    conn: &mut kevy_client::Connection,
    user: &str,
    channel: Channel,
) -> std::io::Result<usize> {
    let prefix = format!("{}:", channel.as_str());
    let mut alive: HashMap<Vec<u8>, bool> = HashMap::new();
    let mut gone = 0;
    for pair in conn
        .hgetall(session_link_key(user).as_bytes())?
        .chunks_exact(2)
    {
        let field = String::from_utf8_lossy(&pair[0]).into_owned();
        let Some(id) = field.strip_prefix(&prefix) else {
            continue;
        };
        let live = match alive.get(&pair[1]) {
            Some(live) => *live,
            None => {
                let key = session_key_by_ref(&String::from_utf8_lossy(&pair[1]));
                let live = conn.exists(&[key.as_bytes()])? > 0;
                alive.insert(pair[1].clone(), live);
                live
            }
        };
        if live {
            continue;
        }
        if evict(conn, user, channel, id)? {
            gone += 1;
        }
        unlink(conn, user, &field)?;
    }
    Ok(gone)
}

/// Every session of `user` ended: stop pushes to every device, including
/// those registered before devices were linked to sessions. How many went.
///
/// # Errors
/// The store could not be read or written.
pub fn drop_all(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<usize> {
    let devices = held(conn, user)?;
    for d in &devices {
        evict(conn, user, d.channel, &d.id)?;
    }
    conn.del(&[session_link_key(user).as_bytes(), used_key(user).as_bytes()])?;
    Ok(devices.len())
}

#[cfg(test)]
mod tests;
