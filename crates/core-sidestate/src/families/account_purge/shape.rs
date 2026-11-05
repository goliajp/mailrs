//! How one registered family's entries for an address are found, counted
//! and removed. The same walk counts on a dry run and removes otherwise,
//! so the counts of a dry run are what a purge would remove.

use std::io;

use kevy_client::Connection;

/// Where a family's entries for one address are.
#[derive(Debug, Clone, Copy)]
pub enum Shape {
    /// One key, built from the address.
    Key(fn(&str) -> String),
    /// Every key matching one of these globs, built from the address (each
    /// builder escapes it).
    Globs(fn(&str) -> Vec<String>),
    /// Members (set, zset) or fields (hash) of the keys matching `keys` (a
    /// glob, or one key) that are the address, or start `address + sep`;
    /// compared ASCII case-insensitively, as group membership is.
    Members {
        keys: fn() -> String,
        sep: Option<&'static str>,
    },
    /// Entries the key alone cannot attribute: a function that finds the
    /// address's through what does (a reverse index, a value), and counts or
    /// removes them.
    Custom(fn(&mut Connection, &str, bool) -> io::Result<u64>),
}

/// `address` with every glob metacharacter escaped, for a `KEYS`/`SCAN`
/// pattern.
pub fn glob_escape(address: &str) -> String {
    let mut out = String::with_capacity(address.len());
    for ch in address.chars() {
        if matches!(ch, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Every key matching `pattern`, all `SCAN` pages.
pub fn scan_all(c: &mut Connection, pattern: &str) -> io::Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, batch) = c.scan(cursor, Some(pattern.as_bytes()), Some(1000))?;
        out.extend(batch);
        if next == 0 {
            break;
        }
        cursor = next;
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn is_ours(member: &[u8], address: &str, sep: Option<&str>) -> bool {
    let a = address.as_bytes();
    if member.eq_ignore_ascii_case(a) {
        return true;
    }
    match sep {
        Some(s) => {
            member.len() > a.len()
                && member[..a.len()].eq_ignore_ascii_case(a)
                && member[a.len()..].starts_with(s.as_bytes())
        }
        None => false,
    }
}

/// Remove `address`'s members or fields from `key`, or count them.
fn members_of(
    c: &mut Connection,
    key: &[u8],
    address: &str,
    sep: Option<&str>,
    dry_run: bool,
) -> io::Result<u64> {
    let kind = c.type_of(key)?;
    let all = match kind.as_str() {
        "set" => c.smembers(key)?,
        "zset" => c.zrange(key, 0, -1)?,
        "hash" => c.hkeys(key)?,
        _ => return Ok(0),
    };
    let ours: Vec<&[u8]> = all
        .iter()
        .map(Vec::as_slice)
        .filter(|m| is_ours(m, address, sep))
        .collect();
    if ours.is_empty() || dry_run {
        return Ok(ours.len() as u64);
    }
    let n = match kind.as_str() {
        "set" => c.srem(key, &ours)?,
        "zset" => c.zrem(key, &ours)?,
        _ => c.hdel(key, &ours)?,
    };
    Ok(n as u64)
}

fn delete_keys(c: &mut Connection, keys: &[Vec<u8>]) -> io::Result<()> {
    for chunk in keys.chunks(500) {
        let refs: Vec<&[u8]> = chunk.iter().map(Vec::as_slice).collect();
        c.del(&refs)?;
    }
    Ok(())
}

impl Shape {
    /// Count `address`'s entries, or remove them when `dry_run` is false:
    /// keys, or members and fields.
    pub fn apply(self, c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
        let keys = match self {
            Shape::Key(build) => {
                let key = build(address).into_bytes();
                match c.exists(&[key.as_slice()])? {
                    0 => Vec::new(),
                    _ => vec![key],
                }
            }
            Shape::Globs(build) => {
                let mut keys = Vec::new();
                for pattern in build(address) {
                    keys.extend(scan_all(c, &pattern)?);
                }
                keys.sort();
                keys.dedup();
                keys
            }
            Shape::Members { keys, sep } => {
                let pattern = keys();
                let names: Vec<Vec<u8>> = match pattern.contains('*') {
                    true => scan_all(c, &pattern)?,
                    false => vec![pattern.into_bytes()],
                };
                let mut count = 0;
                for k in names {
                    count += members_of(c, &k, address, sep, dry_run)?;
                }
                return Ok(count);
            }
            Shape::Custom(f) => return f(c, address, dry_run),
        };
        if !dry_run {
            delete_keys(c, &keys)?;
        }
        Ok(keys.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_metacharacter_in_an_address_matches_only_itself() {
        let mut c = Connection::connect("mem://purge-shape-glob").unwrap();
        c.set(b"zz:a*b@x.test:1", b"1").unwrap();
        c.set(b"zz:aXb@x.test:1", b"1").unwrap();
        let s = Shape::Globs(|a| vec![format!("zz:{}:*", glob_escape(a))]);
        assert_eq!(s.apply(&mut c, "a*b@x.test", false).unwrap(), 1);
        assert_eq!(c.exists(&[b"zz:aXb@x.test:1"]).unwrap(), 1);
    }

    #[test]
    fn members_are_matched_whole_or_up_to_the_separator() {
        let mut c = Connection::connect("mem://purge-shape-members").unwrap();
        c.sadd(
            b"q",
            &[b"a@x.test\t1", b"A@X.test", b"a@x.testy\t1", b"b@x.test\t1"],
        )
        .unwrap();
        let s = Shape::Members {
            keys: || "q".into(),
            sep: Some("\t"),
        };
        assert_eq!(s.apply(&mut c, "a@x.test", true).unwrap(), 2);
        assert_eq!(c.scard(b"q").unwrap(), 4, "a dry run removed members");
        assert_eq!(s.apply(&mut c, "a@x.test", false).unwrap(), 2);
        assert_eq!(c.scard(b"q").unwrap(), 2);
    }
}
