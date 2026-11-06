//! At most one ping per account each [`WINDOW`], without losing the last
//! mail of a burst.
//!
//! A ping carries nothing; the phone refetches the unread count when it
//! gets one. Dropping every ping inside a window would lose mail: the
//! first ping's refetch can run before the second message is stored, and
//! with the second ping dropped the phone would not look again until its
//! 15-minute check. So the first mail of a window pings at once, and if
//! anything arrives while the window is open, exactly one more ping goes
//! out when it closes. That ping opens the next window.
//!
//! The state is two keys in the network kevy, so every process that
//! delivers mail shares one window per account:
//!
//! - the window: a counter that expires with the window. The mail whose
//!   increment made it 1 sends the leading ping
//! - the trailing claim: the first mail to find the window open takes it
//!   and sends the trailing ping. Later mails find the claim and send
//!   nothing: the claim is only released just before that ping goes out,
//!   so the ping's refetch comes after they were stored
//!
//! Every device of the account is pinged in the same round, so one window
//! per account is at most one ping per device per window.

use std::time::Duration;

use mailrs_core_sidestate::keys::push::{unified_push_trailing_key, unified_push_window_key};

/// The shortest gap between two pings to one account.
pub const WINDOW: Duration = Duration::from_secs(5);

/// How long a trailing claim outlives its window. A process that dies
/// holding one leaves later mail of the burst to the phone's own periodic
/// check until it expires.
const TRAILING_SLACK: Duration = Duration::from_secs(2);

/// What one delivered message does about pinging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// Ping now.
    Lead,
    /// Ping once the window closes, after this long.
    Trail(Duration),
    /// A ping still to come covers this message.
    Covered,
}

/// Decide this message's turn.
///
/// # Errors
/// The store could not be read or written.
pub fn claim(
    conn: &mut kevy_client::Connection,
    user: &str,
    window: Duration,
) -> std::io::Result<Turn> {
    let w = unified_push_window_key(user);
    if conn.incr(w.as_bytes())? == 1 {
        conn.expire(w.as_bytes(), window)?;
        return Ok(Turn::Lead);
    }
    let left = conn.ttl_ms(w.as_bytes())?;
    // a process that died between the increment and the expiry left a
    // window that would never close and no ping would ever lead again
    if left == -1 {
        conn.expire(w.as_bytes(), window)?;
    }
    let t = unified_push_trailing_key(user);
    if conn.incr(t.as_bytes())? == 1 {
        conn.expire(t.as_bytes(), window + TRAILING_SLACK)?;
        let wait = match left {
            -1 => window,
            ms if ms > 0 => Duration::from_millis(ms.unsigned_abs()),
            _ => Duration::ZERO,
        };
        return Ok(Turn::Trail(wait));
    }
    // same for a trailing claim without an expiry: nothing would trail again
    if conn.ttl_ms(t.as_bytes())? == -1 {
        conn.expire(t.as_bytes(), window + TRAILING_SLACK)?;
    }
    Ok(Turn::Covered)
}

/// The trailing ping is about to go: open the next window with it, then
/// release the claim. In this order, a message arriving in between finds
/// the claim still held and is covered by this ping.
///
/// # Errors
/// The store could not be written.
pub fn open_next(
    conn: &mut kevy_client::Connection,
    user: &str,
    window: Duration,
) -> std::io::Result<()> {
    conn.set_with_ttl(unified_push_window_key(user).as_bytes(), b"1", window)?;
    conn.del(&[unified_push_trailing_key(user).as_bytes()])?;
    Ok(())
}

/// Whether this message should ping, waiting out the window first when it
/// is the one to send the trailing ping.
///
/// # Errors
/// The store could not be read or written, or a blocking task failed.
pub async fn gate<C>(connect: C, user: &str, window: Duration) -> std::io::Result<bool>
where
    C: Fn() -> std::io::Result<kevy_client::Connection> + Clone + Send + 'static,
{
    let turn = {
        let (connect, user) = (connect.clone(), user.to_string());
        tokio::task::spawn_blocking(move || claim(&mut connect()?, &user, window))
            .await
            .map_err(std::io::Error::other)??
    };
    match turn {
        Turn::Lead => Ok(true),
        Turn::Covered => Ok(false),
        Turn::Trail(wait) => {
            tokio::time::sleep(wait).await;
            let user = user.to_string();
            tokio::task::spawn_blocking(move || open_next(&mut connect()?, &user, window))
                .await
                .map_err(std::io::Error::other)??;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests;
