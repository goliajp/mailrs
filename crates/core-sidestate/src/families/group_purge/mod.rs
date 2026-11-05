//! Deleting an email group, and purging its team working state after the
//! retention period.
//!
//! [`soft_delete`] takes the group out of use at once: its row and member
//! set go, so it is no longer listed, routed, or anyone's team, and mail to
//! its address is routed as to an address that is no group. What its team
//! worked on, [`shared_inbox`]'s `shared:{gid}:*` keys and the reply claims
//! members hold for it in `shared:claims:*`, stays, and the group is marked
//! in [`GROUPS_KEY`]. Once the retention period of deleted accounts
//! ([`pending::retention_days`]) has passed, [`run`] removes that state and
//! writes one audit row with the counts. The mail stays in the members'
//! mailboxes.
//!
//! Group ids come from a counter that only grows, so a group created
//! later, on the same address and name, never has a deleted group's id and
//! reads none of its state.
//!
//! Everything here is in the network kevy, so both cores run the same pass.

use std::io;

use kevy_client::Connection;
use mailrs_core_api::method::admin::{GroupPurgeResponse, GroupPurgeRow, PurgeCount};

use super::account_purge::pending::{self, GROUPS_KEY};
use super::account_purge::scan_all;
use super::{audit, email_groups, shared_inbox};

/// The audit action of the delete.
pub const SOFT_DELETED: &str = "email_group.soft_deleted";
/// The audit action of a purge.
pub const PURGED: &str = "email_group.purged";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Mark {
    deleted_at: i64,
    /// The group's row as it was.
    group: serde_json::Value,
}

/// Delete group `gid` by `actor` at `now`: out of use at once, its working
/// state marked for the purge. Whether there was such a group; deleting one
/// that is not there still clears its member set.
pub fn soft_delete(c: &mut Connection, gid: i64, actor: &str, now: i64) -> io::Result<bool> {
    let field = gid.to_string();
    let row = c
        .hget(email_groups::GROUPS_KEY.as_bytes(), field.as_bytes())?
        .and_then(|v| serde_json::from_slice::<serde_json::Value>(&v).ok());
    if let Some(group) = &row {
        // the mark before the row goes: a delete cut short leaves a marked
        // live group, which the purge skips, never unmarked state
        let first = c.hget(GROUPS_KEY.as_bytes(), field.as_bytes())?.is_none();
        if first {
            let mark = Mark {
                deleted_at: now,
                group: group.clone(),
            };
            let body = serde_json::to_vec(&mark).map_err(io::Error::other)?;
            c.hset(
                GROUPS_KEY.as_bytes(),
                &[(field.as_bytes(), body.as_slice())],
            )?;
        }
    }
    c.hdel(email_groups::GROUPS_KEY.as_bytes(), &[field.as_bytes()])?;
    c.del(&[email_groups::members_key(gid).as_bytes()])?;
    let Some(group) = row else {
        return Ok(false);
    };
    let detail = serde_json::json!({
        "address": group.get("address"),
        "name": group.get("name"),
        "shared": group.get("shared"),
    })
    .to_string();
    audit::append(c, actor, SOFT_DELETED, &field, &detail, now)?;
    Ok(true)
}

/// Every group waiting, with its mark, sorted by id.
fn all(c: &mut Connection) -> io::Result<Vec<(i64, Mark)>> {
    let flat = c.hgetall(GROUPS_KEY.as_bytes())?;
    let mut out: Vec<(i64, Mark)> = flat
        .chunks(2)
        .filter_map(|p| {
            let gid = String::from_utf8_lossy(p.first()?).trim().parse().ok()?;
            let mark = serde_json::from_slice(p.get(1)?).ok()?;
            Some((gid, mark))
        })
        .collect();
    out.sort_by_key(|(g, _)| *g);
    Ok(out)
}

fn bump(counts: &mut Vec<PurgeCount>, name: &str, n: u64) {
    match counts.iter_mut().find(|p| p.name == name) {
        Some(p) => p.count += n,
        None => counts.push(PurgeCount {
            name: name.to_string(),
            count: n,
        }),
    }
}

/// Count group `gid`'s working state, and remove it unless `dry_run`.
fn walk(c: &mut Connection, gid: i64, dry_run: bool) -> io::Result<Vec<PurgeCount>> {
    let mut counts = Vec::new();
    let keys = scan_all(c, &shared_inbox::group_glob(gid))?;
    for key in &keys {
        bump(
            &mut counts,
            &shared_inbox::group_family(gid, &String::from_utf8_lossy(key)),
            1,
        );
    }
    if !dry_run {
        for chunk in keys.chunks(500) {
            let refs: Vec<&[u8]> = chunk.iter().map(Vec::as_slice).collect();
            c.del(&refs)?;
        }
    }
    let mut claims = 0;
    for key in scan_all(c, shared_inbox::CLAIMS_GLOB)? {
        let flat = c.hgetall(&key)?;
        let ours: Vec<&[u8]> = flat
            .chunks(2)
            .filter(|kv| {
                kv.get(1)
                    .and_then(|v| serde_json::from_slice::<shared_inbox::HeldClaim>(v).ok())
                    .is_some_and(|h| h.gid == gid)
            })
            .map(|kv| kv[0].as_slice())
            .collect();
        claims += ours.len() as u64;
        if !dry_run && !ours.is_empty() {
            c.hdel(&key, &ours)?;
        }
    }
    if claims > 0 {
        bump(&mut counts, shared_inbox::GROUP_CLAIMS, claims);
    }
    counts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(counts)
}

/// One pass: every pending group whose `retention_days` have passed at
/// `now` (or only `only`), counted, and removed unless `dry_run`. With
/// nothing pending it reads one hash and writes nothing.
pub fn run(
    c: &mut Connection,
    dry_run: bool,
    only: Option<i64>,
    now: i64,
    retention_days: u64,
) -> io::Result<GroupPurgeResponse> {
    let all = all(c)?;
    let cutoff = now - (retention_days as i64).saturating_mul(86_400);
    let due: Vec<&(i64, Mark)> = all
        .iter()
        .filter(|(g, m)| m.deleted_at <= cutoff && only.is_none_or(|o| o == *g))
        .collect();
    let mut rows = Vec::new();
    let mut skipped_live = Vec::new();
    for (gid, mark) in &due {
        let field = gid.to_string();
        if c.hget(email_groups::GROUPS_KEY.as_bytes(), field.as_bytes())?
            .is_some()
        {
            skipped_live.push(*gid);
            continue;
        }
        let removed = walk(c, *gid, dry_run)?;
        let row = GroupPurgeRow {
            gid: *gid,
            address: mark
                .group
                .get("address")
                .and_then(|a| a.as_str())
                .unwrap_or_default()
                .to_string(),
            deleted_at: mark.deleted_at,
            removed,
        };
        if !dry_run {
            finish(c, &row, now)?;
            tracing::info!(gid, address = %row.address, "deleted group's team state purged");
        }
        rows.push(row);
    }
    Ok(GroupPurgeResponse {
        dry_run,
        retention_days,
        pending: all.len(),
        due: due.len(),
        rows,
        skipped_live,
    })
}

/// [`run`] under the retention in force.
pub fn run_now(
    c: &mut Connection,
    dry_run: bool,
    only: Option<i64>,
    now: i64,
) -> io::Result<GroupPurgeResponse> {
    run(c, dry_run, only, now, pending::retention_days())
}

/// The purge's audit row, then the mark goes. Last, so a purge that failed
/// part-way is retried by the next pass.
fn finish(c: &mut Connection, row: &GroupPurgeRow, now: i64) -> io::Result<()> {
    let detail = serde_json::json!({
        "address": row.address,
        "deleted_at": row.deleted_at,
        "removed": row.removed,
    })
    .to_string();
    let field = row.gid.to_string();
    audit::append(c, "system", PURGED, &field, &detail, now)?;
    c.hdel(GROUPS_KEY.as_bytes(), &[field.as_bytes()])?;
    Ok(())
}

#[cfg(test)]
mod tests;
