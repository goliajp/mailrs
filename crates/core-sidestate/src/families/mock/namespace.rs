use std::path::Path;

use kevy_client::Connection;
use serde::Serialize;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Namespace {
    pub name: String,
    pub owner: String,
    pub created_at: i64,
    pub messages: usize,
    pub dropped: i64,
    pub smtp_enabled: bool,
    pub webhook_url: Option<String>,
}

/// Create `name` for `owner`. Names are global: the address grammar has no
/// room for the owner.
pub fn create_namespace(conn: &mut Connection, owner: &str, name: &str) -> MockResult<Namespace> {
    mailrs_mock::validate_namespace(name).map_err(MockError::BadName)?;
    let limits = settings(conn)?;
    if !limits.enabled {
        return Err(MockError::Disabled);
    }
    let owner_set = owner_key(owner);
    if conn.scard(owner_set.as_bytes())? >= limits.max_namespaces_per_user {
        return Err(MockError::TooManyNamespaces);
    }
    // the set insert is the claim: of two racing creates, one adds the member
    if conn.sadd(NS_INDEX, &[name.as_bytes()])? == 0 {
        return Err(MockError::Exists);
    }
    let created_at = now_ms();
    let owner = owner.to_ascii_lowercase();
    conn.hset(
        ns_key(name).as_bytes(),
        &[
            (b"owner".as_slice(), owner.as_bytes()),
            (b"created_at", created_at.to_string().as_bytes()),
        ],
    )?;
    conn.sadd(owner_set.as_bytes(), &[name.as_bytes()])?;
    Ok(Namespace {
        name: name.to_string(),
        owner,
        created_at,
        messages: 0,
        dropped: 0,
        smtp_enabled: false,
        webhook_url: None,
    })
}

pub fn namespace_exists(conn: &mut Connection, name: &str) -> MockResult<bool> {
    Ok(conn.sismember(NS_INDEX, name.as_bytes())?)
}

pub fn get_namespace(conn: &mut Connection, name: &str) -> MockResult<Option<Namespace>> {
    let fields = conn.hgetall(ns_key(name).as_bytes())?;
    if fields.is_empty() {
        return Ok(None);
    }
    let mut ns = Namespace {
        name: name.to_string(),
        owner: String::new(),
        created_at: 0,
        messages: conn.zcard(msgs_key(name).as_bytes())?,
        dropped: dropped(conn, name)?,
        smtp_enabled: false,
        webhook_url: None,
    };
    for pair in fields.chunks_exact(2) {
        match pair[0].as_slice() {
            b"owner" => ns.owner = utf8(pair[1].clone()),
            b"created_at" => ns.created_at = utf8(pair[1].clone()).parse().unwrap_or(0),
            b"smtp_token_hash" => ns.smtp_enabled = !pair[1].is_empty(),
            b"webhook_url" => ns.webhook_url = Some(utf8(pair[1].clone())),
            _ => {}
        }
    }
    Ok(Some(ns))
}

/// The namespaces `owner` owns, or every namespace when `owner` is `None`.
/// The namespace `name`, when `caller` may use it: its owner, or an
/// administrator. Anyone else is told it does not exist, so a name cannot
/// be probed for. Every REST and MCP entry point asks this, never
/// `get_namespace` directly.
pub fn namespace_for(
    conn: &mut Connection,
    name: &str,
    caller: &str,
    caller_is_admin: bool,
) -> MockResult<Option<Namespace>> {
    let found = get_namespace(conn, name)?;
    Ok(found.filter(|ns| caller_is_admin || ns.owner.eq_ignore_ascii_case(caller)))
}

pub fn list_namespaces(conn: &mut Connection, owner: Option<&str>) -> MockResult<Vec<Namespace>> {
    let names = match owner {
        Some(o) => conn.smembers(owner_key(o).as_bytes())?,
        None => conn.smembers(NS_INDEX)?,
    };
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        if let Some(ns) = get_namespace(conn, &utf8(name))? {
            out.push(ns);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Remove `name`, its messages and their files.
pub fn delete_namespace(conn: &mut Connection, root: &Path, name: &str) -> MockResult<()> {
    let Some(ns) = get_namespace(conn, name)? else {
        return Err(MockError::NotFound);
    };
    clear_webhook(conn, name)?;
    let ids = conn.zrange(msgs_key(name).as_bytes(), 0, -1)?;
    for id in &ids {
        conn.del(&[msg_key(&utf8(id.clone())).as_bytes()])?;
    }
    let dir = root.join(name);
    let bytes = dir_bytes(&dir);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if bytes > 0 {
        conn.incr_by(BYTES, -bytes)?;
    }
    conn.del(&[
        msgs_key(name).as_bytes(),
        ns_key(name).as_bytes(),
        dropped_key(name).as_bytes(),
        tags_key(name).as_bytes(),
        uid_key(name).as_bytes(),
    ])?;
    conn.srem(owner_key(&ns.owner).as_bytes(), &[name.as_bytes()])?;
    conn.srem(NS_INDEX, &[name.as_bytes()])?;
    Ok(())
}

/// Store the hash of the namespace's SMTP password, or clear it with `""`.
pub fn set_smtp_token_hash(conn: &mut Connection, name: &str, hash: &str) -> MockResult<()> {
    if !namespace_exists(conn, name)? {
        return Err(MockError::NotFound);
    }
    conn.hset(
        ns_key(name).as_bytes(),
        &[(b"smtp_token_hash".as_slice(), hash.as_bytes())],
    )?;
    Ok(())
}

fn smtp_token_hash(conn: &mut Connection, name: &str) -> MockResult<Option<String>> {
    Ok(conn
        .hget(ns_key(name).as_bytes(), b"smtp_token_hash")?
        .map(utf8)
        .filter(|h| !h.is_empty()))
}

// the token is 32 random bytes, so a plain digest is enough to keep the
// stored value useless to someone who reads kevy
fn hash_token(token: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Give the namespace a new SMTP password, replacing any earlier one, and
/// return it. Only its hash is kept.
pub fn rotate_smtp_token(conn: &mut Connection, name: &str) -> MockResult<String> {
    let mut bytes = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut bytes);
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    set_smtp_token_hash(conn, name, &hash_token(&token))?;
    Ok(token)
}

pub fn verify_smtp_token(conn: &mut Connection, name: &str, token: &str) -> MockResult<bool> {
    let Some(stored) = smtp_token_hash(conn, name)? else {
        return Ok(false);
    };
    let given = hash_token(token);
    // equal-length hex digests; compare every byte regardless
    let diff = stored
        .bytes()
        .zip(given.bytes())
        .fold(stored.len() ^ given.len(), |acc, (a, b)| {
            acc | usize::from(a ^ b)
        });
    Ok(diff == 0)
}

/// Note when `tag` was handed out, so waiting for its first message can
/// start from then rather than from the moment the wait began.
pub fn record_minted(conn: &mut Connection, ns: &str, tag: &str, at_ms: i64) -> MockResult<()> {
    let key = tags_key(ns);
    let retention = settings(conn)?.retention();
    conn.hset(
        key.as_bytes(),
        &[(tag.as_bytes(), at_ms.to_string().as_bytes())],
    )?;
    conn.hexpire(
        key.as_bytes(),
        &[tag.as_bytes()],
        retention,
        kevy_client::HExpireCond::Always,
    )?;
    Ok(())
}

pub fn minted_at(conn: &mut Connection, ns: &str, tag: &str) -> MockResult<Option<i64>> {
    Ok(conn
        .hget(tags_key(ns).as_bytes(), tag.as_bytes())?
        .and_then(|v| utf8(v).parse().ok()))
}

/// The highest IMAP UID handed out in the namespace; 0 before its first message.
pub fn last_uid(conn: &mut Connection, ns: &str) -> MockResult<u32> {
    Ok(conn
        .get(uid_key(ns).as_bytes())?
        .and_then(|v| utf8(v).parse().ok())
        .unwrap_or(0))
}

fn dropped(conn: &mut Connection, name: &str) -> MockResult<i64> {
    Ok(conn
        .get(dropped_key(name).as_bytes())?
        .and_then(|v| utf8(v).parse().ok())
        .unwrap_or(0))
}

/// Bytes of the message files `ns` holds under `root`.
pub fn namespace_bytes(root: &Path, ns: &str) -> i64 {
    dir_bytes(&root.join(ns))
}

pub(super) fn dir_bytes(dir: &Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len() as i64)
        .sum()
}
