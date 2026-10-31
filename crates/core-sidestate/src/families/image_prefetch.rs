//! The remote images waiting to be fetched, for the SQL core, which has no
//! store of its own to keep the queue in. fastcore keeps the same queue in
//! its embedded store.

use std::io;

/// Set of `user \t message id \t url`.
pub const PENDING: &[u8] = b"mailrs:image-prefetch:pending";

fn member(user: &str, message_id: &str, url: &str) -> String {
    format!("{user}\t{message_id}\t{url}")
}

/// Queue `urls`, found in `user`'s message `message_id`.
pub fn queue(
    c: &mut kevy_client::Connection,
    user: &str,
    message_id: &str,
    urls: &[String],
) -> io::Result<()> {
    if urls.is_empty() {
        return Ok(());
    }
    let members: Vec<String> = urls.iter().map(|u| member(user, message_id, u)).collect();
    let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
    c.sadd(PENDING, &refs)?;
    Ok(())
}

/// Up to `limit` queued `(user, message id, url)`, left in the queue.
pub fn pending(
    c: &mut kevy_client::Connection,
    limit: usize,
) -> io::Result<Vec<(String, String, String)>> {
    Ok(c.smembers(PENDING)?
        .into_iter()
        .filter_map(|m| {
            let m = String::from_utf8(m).ok()?;
            let mut parts = m.splitn(3, '\t');
            Some((
                parts.next()?.to_string(),
                parts.next()?.to_string(),
                parts.next()?.to_string(),
            ))
        })
        .take(limit)
        .collect())
}

/// Take one fetched or given-up image off the queue.
pub fn done(
    c: &mut kevy_client::Connection,
    user: &str,
    message_id: &str,
    url: &str,
) -> io::Result<()> {
    c.srem(PENDING, &[member(user, message_id, url).as_bytes()])?;
    Ok(())
}
