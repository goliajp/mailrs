//! The resource names CalDAV clients filed events under, where a name is
//! not the event's UID.
//!
//! ```text
//!   calendar_event_names:{user}   hash  resource name → uid
//!   calendar_event_hrefs:{user}   hash  uid → resource name
//! ```
//!
//! A row is keyed by its iCalendar `UID`, so the copy a phone saves and
//! the invitation filed from mail are one row. A client may still PUT it
//! to `{anything}.ics`, and expects to find it at that href again and to
//! see it listed there. Only such names are recorded; a row without an
//! entry is served as `{uid}.ics`.
//!
//! [`link`] is the one writer of both hashes and [`forget`] the one
//! remover, called by the row's own deleter; [`resolve`], [`name_of`] and
//! [`hrefs`] are the readers.

use std::collections::HashMap;
use std::io;

use kevy_client::Connection;

/// Resource name → uid.
pub fn names_key(user: &str) -> String {
    format!("calendar_event_names:{user}")
}

/// Uid → resource name.
pub fn hrefs_key(user: &str) -> String {
    format!("calendar_event_hrefs:{user}")
}

fn read(conn: &mut Connection, key: &str, field: &str) -> io::Result<Option<String>> {
    let raw = conn.hget(key.as_bytes(), field.as_bytes())?;
    Ok(raw.map(|v| String::from_utf8_lossy(&v).into_owned()))
}

/// Serve `uid` under `name` from now on. Whatever either was paired with
/// before is dropped, so a name never resolves to two rows and a row is
/// never listed twice. `name == uid` leaves no entry.
pub fn link(conn: &mut Connection, user: &str, name: &str, uid: &str) -> io::Result<()> {
    let names = names_key(user);
    let hrefs = hrefs_key(user);
    if let Some(prev_uid) = read(conn, &names, name)?
        && prev_uid != uid
    {
        conn.hdel(hrefs.as_bytes(), &[prev_uid.as_bytes()])?;
    }
    if let Some(prev_name) = read(conn, &hrefs, uid)?
        && prev_name != name
    {
        conn.hdel(names.as_bytes(), &[prev_name.as_bytes()])?;
    }
    match name == uid {
        true => {
            conn.hdel(names.as_bytes(), &[name.as_bytes()])?;
            conn.hdel(hrefs.as_bytes(), &[uid.as_bytes()])?;
        }
        false => {
            conn.hset(names.as_bytes(), &[(name.as_bytes(), uid.as_bytes())])?;
            conn.hset(hrefs.as_bytes(), &[(uid.as_bytes(), name.as_bytes())])?;
        }
    }
    Ok(())
}

/// Drop `uid`'s name, when its row goes.
pub fn forget(conn: &mut Connection, user: &str, uid: &str) -> io::Result<()> {
    let hrefs = hrefs_key(user);
    if let Some(name) = read(conn, &hrefs, uid)? {
        conn.hdel(names_key(user).as_bytes(), &[name.as_bytes()])?;
        conn.hdel(hrefs.as_bytes(), &[uid.as_bytes()])?;
    }
    Ok(())
}

/// The uid a request for `name` means: the one filed under it, else
/// `name` itself — unless the row with that uid is served under another
/// name, in which case `name` means nothing.
pub fn resolve(conn: &mut Connection, user: &str, name: &str) -> io::Result<Option<String>> {
    if let Some(uid) = read(conn, &names_key(user), name)? {
        return Ok(Some(uid));
    }
    match read(conn, &hrefs_key(user), name)? {
        Some(_) => Ok(None),
        None => Ok(Some(name.to_string())),
    }
}

/// The name `uid` is served under.
pub fn name_of(conn: &mut Connection, user: &str, uid: &str) -> io::Result<String> {
    Ok(read(conn, &hrefs_key(user), uid)?.unwrap_or_else(|| uid.to_string()))
}

/// Every recorded name, uid → name.
pub fn hrefs(conn: &mut Connection, user: &str) -> io::Result<HashMap<String, String>> {
    let flat = conn.hgetall(hrefs_key(user).as_bytes())?;
    Ok(flat
        .chunks(2)
        .filter_map(|pair| match pair {
            [k, v] => Some((
                String::from_utf8_lossy(k).into_owned(),
                String::from_utf8_lossy(v).into_owned(),
            )),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> Connection {
        Connection::connect(&format!("mem://calendar-event-names-{name}")).expect("kevy")
    }

    #[test]
    fn a_name_that_is_the_uid_records_nothing() {
        let mut c = conn("same");
        link(&mut c, "u", "e1", "e1").expect("link");
        assert!(hrefs(&mut c, "u").expect("hrefs").is_empty());
        assert_eq!(
            resolve(&mut c, "u", "e1").expect("resolve").as_deref(),
            Some("e1")
        );
        assert_eq!(name_of(&mut c, "u", "e1").expect("name"), "e1");
    }

    #[test]
    fn a_chosen_name_resolves_both_ways_and_hides_the_uid_href() {
        let mut c = conn("chosen");
        link(&mut c, "u", "phone-a", "uid-1").expect("link");
        assert_eq!(
            resolve(&mut c, "u", "phone-a").expect("resolve").as_deref(),
            Some("uid-1")
        );
        assert_eq!(name_of(&mut c, "u", "uid-1").expect("name"), "phone-a");
        assert_eq!(resolve(&mut c, "u", "uid-1").expect("resolve"), None);
        assert_eq!(
            resolve(&mut c, "other", "phone-a")
                .expect("resolve")
                .as_deref(),
            Some("phone-a")
        );
    }

    /// Re-pairing must not leave the old half behind, or one name would
    /// reach two rows, or one row be listed under two hrefs.
    #[test]
    fn relinking_drops_the_previous_pairing() {
        let mut c = conn("relink");
        link(&mut c, "u", "phone-a", "uid-1").expect("link");
        link(&mut c, "u", "phone-b", "uid-1").expect("move the row");
        assert_eq!(
            resolve(&mut c, "u", "phone-a").expect("resolve").as_deref(),
            Some("phone-a")
        );
        link(&mut c, "u", "phone-b", "uid-2").expect("reuse the name");
        assert_eq!(name_of(&mut c, "u", "uid-1").expect("name"), "uid-1");
        assert_eq!(
            hrefs(&mut c, "u").expect("hrefs"),
            HashMap::from([("uid-2".to_string(), "phone-b".to_string())])
        );
        link(&mut c, "u", "uid-2", "uid-2").expect("back to its uid");
        assert!(hrefs(&mut c, "u").expect("hrefs").is_empty());
        assert_eq!(
            resolve(&mut c, "u", "phone-b").expect("resolve").as_deref(),
            Some("phone-b")
        );
    }

    #[test]
    fn forget_removes_both_directions() {
        let mut c = conn("forget");
        link(&mut c, "u", "phone-a", "uid-1").expect("link");
        forget(&mut c, "u", "uid-1").expect("forget");
        forget(&mut c, "u", "uid-1").expect("forget again");
        assert!(hrefs(&mut c, "u").expect("hrefs").is_empty());
        assert_eq!(
            resolve(&mut c, "u", "phone-a").expect("resolve").as_deref(),
            Some("phone-a")
        );
    }
}
