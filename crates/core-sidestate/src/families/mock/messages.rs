use std::path::Path;

use kevy_client::Connection;
use mailrs_mock::parsed::Summary;
use serde::{Deserialize, Serialize};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// mail addressed to the namespace
    In,
    /// mail the namespace's sandbox sent, kept here instead of leaving
    CapturedOut,
}

/// What is kept beside a message's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageMeta {
    pub id: String,
    pub namespace: String,
    pub tag: String,
    pub direction: Direction,
    /// the envelope recipient this copy was for
    pub rcpt: String,
    pub size: usize,
    pub received_at: i64,
    /// IMAP UID: ascending in arrival order within the namespace, never reused
    #[serde(default)]
    pub uid: u32,
    #[serde(flatten)]
    pub summary: Summary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Stored,
    /// the same id was stored before, as when a spool file is drained twice
    AlreadyStored,
    NoNamespace,
    Full,
    /// mock mail is turned off in the settings
    Disabled,
}

/// A message id from the spool file it came in and the recipient's
/// position in it, so draining the same file again names the same message.
pub fn spool_message_id(spool_file: &str, rcpt_index: usize) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in spool_file.bytes().chain(rcpt_index.to_le_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A message id for mail that did not come through the spool.
pub fn fresh_message_id() -> String {
    let mut bytes = [0u8; 8];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    format!("m{:x}{}", now_ms(), hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Keep `raw` as message `meta.id` of `meta.namespace`. `meta.size` and
/// `meta.received_at` are set here.
pub fn store_message(
    conn: &mut Connection,
    root: &Path,
    mut meta: MessageMeta,
    raw: &[u8],
) -> MockResult<Stored> {
    let ns = meta.namespace.clone();
    let limits = settings(conn)?;
    if !limits.enabled {
        return Ok(Stored::Disabled);
    }
    if !namespace_exists(conn, &ns)? {
        return Ok(Stored::NoNamespace);
    }
    let key = msg_key(&meta.id);
    if conn.exists(&[key.as_bytes()])? > 0 {
        return Ok(Stored::AlreadyStored);
    }
    if total_bytes(conn)? >= limits.max_total_bytes() {
        return Ok(Stored::Full);
    }
    let path = message_path(root, &ns, &meta.id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, raw)?;
    meta.size = raw.len();
    meta.received_at = now_ms();
    meta.uid = conn
        .incr(uid_key(&ns).as_bytes())?
        .try_into()
        .unwrap_or(u32::MAX);
    let json = serde_json::to_vec(&meta).map_err(std::io::Error::other)?;
    conn.hset(
        key.as_bytes(),
        &[(b"ns".as_slice(), ns.as_bytes()), (b"meta", &json)],
    )?;
    conn.expire(key.as_bytes(), limits.retention())?;
    conn.zadd(
        msgs_key(&ns).as_bytes(),
        &[(meta.received_at as f64, meta.id.as_bytes())],
    )?;
    conn.incr_by(BYTES, raw.len() as i64)?;
    conn.incr(RECEIVED)?;
    drop_over_cap(conn, root, &ns, limits.max_messages)?;
    if meta.direction == Direction::In {
        notify(conn, &meta)?;
    }
    Ok(Stored::Stored)
}

fn drop_over_cap(conn: &mut Connection, root: &Path, ns: &str, cap: usize) -> MockResult<()> {
    let count = conn.zcard(msgs_key(ns).as_bytes())?;
    if count <= cap {
        return Ok(());
    }
    let excess = count - cap;
    let oldest = conn.zrange(msgs_key(ns).as_bytes(), 0, excess as i64 - 1)?;
    for id in oldest {
        remove_message(conn, root, ns, &utf8(id))?;
    }
    conn.incr_by(dropped_key(ns).as_bytes(), excess as i64)?;
    Ok(())
}

pub fn total_bytes(conn: &mut Connection) -> MockResult<i64> {
    Ok(conn
        .get(BYTES)?
        .and_then(|v| utf8(v).parse().ok())
        .unwrap_or(0))
}

/// Which messages a listing returns, newest first.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub tag: Option<String>,
    pub tag_prefix: Option<String>,
    /// an envelope recipient or a `To` / `Cc` address, any case
    pub to: Option<String>,
    pub from: Option<String>,
    pub subject: Option<String>,
    /// arrival in ms, inclusive
    pub since: Option<i64>,
    pub limit: usize,
}

impl Filter {
    fn admits(&self, m: &MessageMeta) -> bool {
        let contains =
            |hay: &str, needle: &str| hay.to_lowercase().contains(&needle.to_lowercase());
        self.tag.as_ref().is_none_or(|t| &m.tag == t)
            && self
                .tag_prefix
                .as_ref()
                .is_none_or(|p| m.tag.starts_with(p.as_str()))
            && self.to.as_ref().is_none_or(|to| {
                m.rcpt.eq_ignore_ascii_case(to)
                    || m.summary
                        .to
                        .iter()
                        .chain(&m.summary.cc)
                        .any(|a| contains(a, to))
            })
            && self
                .from
                .as_ref()
                .is_none_or(|f| contains(&m.summary.from, f))
            && self
                .subject
                .as_ref()
                .is_none_or(|s| contains(&m.summary.subject, s))
    }
}

pub fn list_messages(
    conn: &mut Connection,
    usable: &Usable,
    filter: &Filter,
) -> MockResult<Vec<MessageMeta>> {
    let ns = usable.name();
    let ids = conn.zrange(msgs_key(ns).as_bytes(), 0, -1)?;
    let mut out = Vec::new();
    for id in ids.into_iter().rev() {
        let id = utf8(id);
        let Some(meta) = load(conn, ns, &id)? else {
            continue;
        };
        // the zset is in arrival order, so nothing older can match
        if filter.since.is_some_and(|s| meta.received_at < s) {
            break;
        }
        if filter.admits(&meta) {
            out.push(meta);
            if out.len() >= filter.limit {
                break;
            }
        }
    }
    Ok(out)
}

/// The message's metadata, or `None` when it is not in `ns` or has expired.
pub fn get_message(
    conn: &mut Connection,
    usable: &Usable,
    id: &str,
) -> MockResult<Option<MessageMeta>> {
    let ns = usable.name();
    let meta = load(conn, ns, id)?;
    Ok(meta)
}

fn load(conn: &mut Connection, ns: &str, id: &str) -> MockResult<Option<MessageMeta>> {
    let Some(json) = conn.hget(msg_key(id).as_bytes(), b"meta")? else {
        // expired: the zset still lists it until now
        if conn
            .zscore(msgs_key(ns).as_bytes(), id.as_bytes())?
            .is_some()
        {
            conn.zrem(msgs_key(ns).as_bytes(), &[id.as_bytes()])?;
        }
        return Ok(None);
    };
    let meta: MessageMeta = serde_json::from_slice(&json).map_err(std::io::Error::other)?;
    if meta.namespace != ns {
        return Ok(None);
    }
    Ok(Some(meta))
}

pub fn read_raw(root: &Path, usable: &Usable, id: &str) -> std::io::Result<Vec<u8>> {
    let ns = usable.name();
    std::fs::read(message_path(root, ns, id))
}

/// Remove one message; `false` when it was not there.
pub fn delete_message(
    conn: &mut Connection,
    root: &Path,
    usable: &Usable,
    id: &str,
) -> MockResult<bool> {
    let ns = usable.name();
    if conn
        .zscore(msgs_key(ns).as_bytes(), id.as_bytes())?
        .is_none()
    {
        return Ok(false);
    }
    remove_message(conn, root, ns, id)?;
    Ok(true)
}

/// Remove every message of `ns`, or those with `tag`; returns how many.
pub fn delete_messages(
    conn: &mut Connection,
    root: &Path,
    usable: &Usable,
    tag: Option<&str>,
) -> MockResult<usize> {
    let ns = usable.name();
    let ids = conn.zrange(msgs_key(ns).as_bytes(), 0, -1)?;
    let mut removed = 0;
    for id in ids {
        let id = utf8(id);
        let keep = match (tag, load(conn, ns, &id)?) {
            (_, None) => continue,
            (Some(t), Some(m)) => m.tag != t,
            (None, Some(_)) => false,
        };
        if !keep {
            remove_message(conn, root, ns, &id)?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub(super) fn remove_message(
    conn: &mut Connection,
    root: &Path,
    ns: &str,
    id: &str,
) -> MockResult<()> {
    let path = message_path(root, ns, id);
    let size = std::fs::metadata(&path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if size > 0 {
        conn.incr_by(BYTES, -size)?;
    }
    conn.del(&[msg_key(id).as_bytes()])?;
    conn.zrem(msgs_key(ns).as_bytes(), &[id.as_bytes()])?;
    Ok(())
}
