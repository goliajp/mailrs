//! Permission groups in the network kevy. Every reader and writer (the
//! contract handlers in `groups_admin`, webapi's admin pages and MCP tools,
//! fastcore's authorization check) uses these keys, so there is one
//! spelling:
//!   `admin:groups`                   hash id → group JSON
//!   `admin:groups:counter`           id counter
//!   `admin:groups:{id}:permissions`  set of permission strings
//!   `admin:groups:{id}:members`      set of member addresses
//!
//! Groups are few, so membership is answered by scanning every group; there
//! is no account → groups index to keep in step.

use std::fmt::Display;

use kevy_client::{Connection, KevyResult};
use mailrs_core_api::method::admin::GroupWire;

pub const GROUPS_KEY: &str = "admin:groups";
pub const GROUPS_COUNTER: &str = "admin:groups:counter";

pub fn permissions_key(id: impl Display) -> String {
    format!("admin:groups:{id}:permissions")
}

pub fn members_key(id: impl Display) -> String {
    format!("admin:groups:{id}:members")
}

/// A stored group row into the wire shape. Rows written before `domain` and
/// `is_builtin` existed lack them.
pub fn parse_group(v: &[u8]) -> Option<GroupWire> {
    let j: serde_json::Value = serde_json::from_slice(v).ok()?;
    let text = |k: &str| j.get(k).and_then(|x| x.as_str());
    Some(GroupWire {
        id: j.get("id")?.as_i64()?,
        name: text("name").unwrap_or("").into(),
        domain: text("domain").map(String::from),
        description: text("description").unwrap_or("").into(),
        is_builtin: j
            .get("is_builtin")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        created_at: j.get("created_at").and_then(|x| x.as_i64()).unwrap_or(0),
    })
}

pub fn all_groups(conn: &mut Connection) -> KevyResult<Vec<GroupWire>> {
    let flat = conn.hgetall(GROUPS_KEY.as_bytes())?;
    Ok(flat
        .into_iter()
        .skip(1)
        .step_by(2)
        .filter_map(|v| parse_group(&v))
        .collect())
}

fn strings(raw: Vec<Vec<u8>>) -> Vec<String> {
    raw.into_iter()
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .collect()
}

pub fn group_permissions(conn: &mut Connection, id: i64) -> KevyResult<Vec<String>> {
    Ok(strings(conn.smembers(permissions_key(id).as_bytes())?))
}

pub fn group_members(conn: &mut Connection, id: i64) -> KevyResult<Vec<String>> {
    Ok(strings(conn.smembers(members_key(id).as_bytes())?))
}

/// The groups `address` is a member of, by id.
pub fn groups_of(conn: &mut Connection, address: &str) -> KevyResult<Vec<GroupWire>> {
    // addresses are matched without regard to case, the way a login is
    let mut out = Vec::new();
    for g in all_groups(conn)? {
        if group_members(conn, g.id)?
            .iter()
            .any(|m| m.eq_ignore_ascii_case(address))
        {
            out.push(g);
        }
    }
    out.sort_by_key(|g| g.id);
    Ok(out)
}

/// What group membership grants an account.
#[derive(Debug, Default)]
pub struct Membership {
    pub groups: Vec<GroupWire>,
    /// Union of the groups' permissions, sorted, no duplicates.
    pub permissions: Vec<String>,
}

pub fn membership(conn: &mut Connection, address: &str) -> KevyResult<Membership> {
    let groups = groups_of(conn, address)?;
    let mut permissions = Vec::new();
    for g in &groups {
        permissions.extend(group_permissions(conn, g.id)?);
    }
    permissions.sort();
    permissions.dedup();
    Ok(Membership {
        groups,
        permissions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_keys_keep_their_prod_spelling() {
        assert_eq!(GROUPS_KEY, "admin:groups");
        assert_eq!(GROUPS_COUNTER, "admin:groups:counter");
        assert_eq!(permissions_key(9), "admin:groups:9:permissions");
        assert_eq!(members_key(9), "admin:groups:9:members");
    }

    fn group(conn: &mut Connection, id: i64, perms: &[&str], members: &[&str]) {
        let row = serde_json::json!({ "id": id, "name": format!("g{id}") }).to_string();
        conn.hset(
            GROUPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), row.as_bytes())],
        )
        .unwrap();
        let p: Vec<&[u8]> = perms.iter().map(|s| s.as_bytes()).collect();
        conn.sadd(permissions_key(id).as_bytes(), &p).unwrap();
        let m: Vec<&[u8]> = members.iter().map(|s| s.as_bytes()).collect();
        conn.sadd(members_key(id).as_bytes(), &m).unwrap();
    }

    #[test]
    fn membership_is_the_union_over_the_groups_holding_the_address() {
        let mut conn = Connection::connect("mem://sidestate-permission-groups").unwrap();
        group(
            &mut conn,
            1,
            &["admin.domains", "mail.send"],
            &["a@x", "b@x"],
        );
        group(&mut conn, 2, &["admin.queue", "mail.send"], &["a@x"]);
        group(&mut conn, 3, &["admin.accounts"], &["b@x"]);

        let a = membership(&mut conn, "a@x").unwrap();
        assert_eq!(a.groups.iter().map(|g| g.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(a.permissions, ["admin.domains", "admin.queue", "mail.send"]);

        let none = membership(&mut conn, "c@x").unwrap();
        assert!(none.groups.is_empty() && none.permissions.is_empty());
    }

    #[test]
    fn a_member_matches_whatever_case_either_side_uses() {
        let mut conn = Connection::connect("mem://sidestate-permission-groups-case").unwrap();
        group(&mut conn, 1, &["admin.domains"], &["Ops@X.test"]);
        let m = membership(&mut conn, "ops@x.test").unwrap();
        assert_eq!(m.permissions, ["admin.domains"]);
        assert_eq!(membership(&mut conn, "OPS@x.TEST").unwrap().groups.len(), 1);
        assert!(
            membership(&mut conn, "other@x.test")
                .unwrap()
                .groups
                .is_empty()
        );
    }
}
