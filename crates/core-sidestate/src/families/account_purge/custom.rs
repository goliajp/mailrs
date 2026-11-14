//! The families whose key does not name the address: found through a
//! reverse index, or by reading the value. Each counts on a dry run and
//! removes otherwise, and returns how many entries it found.

use std::io;

use kevy_client::Connection;

use super::shape::scan_all;
use crate::families::{identity_link, inline_images, masked_email, mock, uploads};
use crate::keys::{credentials, session};

fn same(a: &[u8], address: &str) -> bool {
    a.eq_ignore_ascii_case(address.as_bytes())
}

/// Sessions, through `session:by_addr:{user}`; the index itself is its own
/// registry entry, after this one.
pub(super) fn sessions(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    if dry_run {
        return Ok(c.scard(session::by_addr_key(address).as_bytes())? as u64);
    }
    Ok(session::end_every_session(c, address)? as u64)
}

/// Agent-key secret index entries: `{user, id}` JSON under the hash of the
/// key, so every entry is read. A handful per server.
pub(super) fn agent_key_index(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    json_field_is(c, "agent:key:secret:*", "user", address, dry_run)
}

/// Keys matching `pattern` holding a JSON string whose `field` is the
/// address.
fn json_field_is(
    c: &mut Connection,
    pattern: &str,
    field: &str,
    address: &str,
    dry_run: bool,
) -> io::Result<u64> {
    let mut n = 0;
    for key in scan_all(c, pattern)? {
        if c.type_of(&key)? != "string" {
            continue;
        }
        let Some(raw) = c.get(&key)? else {
            continue;
        };
        let owner = serde_json::from_slice::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get(field).and_then(|u| u.as_str()).map(String::from));
        if owner.is_some_and(|o| same(o.as_bytes(), address)) {
            n += 1;
            if !dry_run {
                c.del(&[key.as_slice()])?;
            }
        }
    }
    Ok(n)
}

/// Hash keys matching `pattern` whose `field` is the address.
fn hash_field_is(
    c: &mut Connection,
    pattern: &str,
    field: &str,
    address: &str,
    dry_run: bool,
) -> io::Result<u64> {
    let mut n = 0;
    for key in scan_all(c, pattern)? {
        if c.type_of(&key)? != "hash" {
            continue;
        }
        if c.hget(&key, field.as_bytes())?
            .is_some_and(|v| same(&v, address))
        {
            n += 1;
            if !dry_run {
                c.del(&[key.as_slice()])?;
            }
        }
    }
    Ok(n)
}

/// The password-reset token `pwreset_by_addr:{user}` points at.
pub(super) fn password_reset_token(
    c: &mut Connection,
    address: &str,
    dry_run: bool,
) -> io::Result<u64> {
    let Some(token) = c.get(credentials::pwreset_by_addr_key(address).as_bytes())? else {
        return Ok(0);
    };
    let key = credentials::pwreset_key(&String::from_utf8_lossy(&token));
    let n = c.exists(&[key.as_bytes()])? as u64;
    if n > 0 && !dry_run {
        c.del(&[key.as_bytes()])?;
    }
    Ok(n)
}

/// Single sign-on links, through `oidc:links:{user}`.
pub(super) fn identity_links(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    if dry_run {
        return Ok(identity_link::links_for(c, address)?.len() as u64);
    }
    Ok(identity_link::unlink_all(c, address)? as u64)
}

/// Tokens and codes this server issued as an identity provider: hashes
/// with a `user` field, keyed by the token, so every one is read. Counted,
/// never printed: the key is the credential.
pub(super) fn oidc_grants(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    let mut n = 0;
    for pattern in ["oidc:token:*", "oidc:refresh:*", "oidc:code:*"] {
        n += hash_field_is(c, pattern, "user", address, dry_run)?;
    }
    Ok(n)
}

/// Pasted images, hashes with an `owner` field.
pub(super) fn inline_uploads(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    hash_field_is(c, &inline_images::key("*"), "owner", address, dry_run)
}

/// The account's directory of uploads (draft files, JMAP uploads), on disk.
/// The directory is named by the lowercased address, so only that spelling
/// is walked; the other would count the same files twice.
pub(super) fn upload_dir(_: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    if address != address.to_lowercase() {
        return Ok(0);
    }
    uploads::purge_account(&uploads::root(), address, dry_run)
}

/// Masked addresses, through the owner's index of them; the index too.
pub(super) fn masks(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    let index = masked_email::user_key(address);
    let masks = c.hkeys(index.as_bytes())?;
    if !dry_run {
        for m in &masks {
            let k = masked_email::addr_key(&String::from_utf8_lossy(m));
            c.del(&[k.as_bytes()])?;
        }
        c.del(&[index.as_bytes()])?;
    }
    Ok(masks.len() as u64)
}

/// Disposable test mailboxes and their messages.
pub(super) fn mock_namespaces(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    mock::purge_owner(c, mock::root(), address, dry_run).map_err(io::Error::other)
}

/// Aliases whose target is the address: they would deliver to nobody, or
/// to whoever takes the address next.
/// Each goes from the network store's source set too, which lists them;
/// the embedded store has no such set and the removal does nothing there.
pub(super) fn aliases_to(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    use mailrs_alias_store::keys::{ALIAS_SOURCES_KEY, ALIAS_V2_PREFIX, alias_v2_key};
    let mut n = 0;
    for key in scan_all(c, &alias_v2_key("*"))? {
        if c.type_of(&key)? != "hash" {
            continue;
        }
        if !c.hget(&key, b"target")?.is_some_and(|v| same(&v, address)) {
            continue;
        }
        n += 1;
        if !dry_run {
            c.del(&[key.as_slice()])?;
            if let Some(source) = key.strip_prefix(ALIAS_V2_PREFIX) {
                c.srem(ALIAS_SOURCES_KEY, &[source])?;
            }
        }
    }
    Ok(n)
}

/// The pre-split admin panel's alias mirror, `admin:aliases`: JSON rows by
/// field.
pub(super) fn legacy_aliases_to(
    c: &mut Connection,
    address: &str,
    dry_run: bool,
) -> io::Result<u64> {
    const KEY: &[u8] = b"admin:aliases";
    let flat = c.hgetall(KEY)?;
    let mut ours = Vec::new();
    for pair in flat.chunks(2) {
        let [field, value] = pair else { continue };
        let target = serde_json::from_slice::<serde_json::Value>(value)
            .ok()
            .and_then(|v| {
                v.get("target_address")
                    .and_then(|t| t.as_str())
                    .map(String::from)
            });
        if target.is_some_and(|t| same(t.as_bytes(), address)) {
            ours.push(field.as_slice());
        }
    }
    if !ours.is_empty() && !dry_run {
        c.hdel(KEY, &ours)?;
    }
    Ok(ours.len() as u64)
}

/// Mailbox ids, `mailrs:mbid:{id}` holding `"{user}\n{name}"`.
pub(super) fn mailbox_ids(c: &mut Connection, address: &str, dry_run: bool) -> io::Result<u64> {
    let mut n = 0;
    for key in scan_all(c, "mailrs:mbid:*")? {
        let Some(v) = c.get(&key)? else { continue };
        let owner = v.split(|b| *b == b'\n').next().unwrap_or_default();
        if same(owner, address) {
            n += 1;
            if !dry_run {
                c.del(&[key.as_slice()])?;
            }
        }
    }
    Ok(n)
}
