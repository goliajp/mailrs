//! Email groups (distribution lists) in the network kevy:
//!   `admin:email-groups`              hash id → JSON row (id, address, domain, name, …)
//!   `admin:email-groups:counter`      id counter
//!   `admin:email-group:{id}:members`  set of member addresses
//!
//! Members live only in the set, never in the row. webapi writes these
//! keys; the spool drain reads them to expand a group address.

use kevy_client::Connection;

pub const GROUPS_KEY: &str = "admin:email-groups";
pub const COUNTER_KEY: &str = "admin:email-groups:counter";

pub fn members_key(id: impl std::fmt::Display) -> String {
    format!("admin:email-group:{id}:members")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: i64,
    pub address: String,
    pub members: Vec<String>,
}

/// The group whose address is `addr` (compared ASCII case-insensitively),
/// with its members as stored.
pub fn find_by_address(conn: &mut Connection, addr: &str) -> std::io::Result<Option<Group>> {
    let flat = conn.hgetall(GROUPS_KEY.as_bytes())?;
    let row = flat
        .iter()
        .skip(1)
        .step_by(2)
        .filter_map(|v| serde_json::from_slice::<serde_json::Value>(v).ok())
        .find(|j| {
            j.get("address")
                .and_then(|a| a.as_str())
                .is_some_and(|a| a.trim().eq_ignore_ascii_case(addr))
        });
    let Some(row) = row else {
        return Ok(None);
    };
    let Some(id) = row.get("id").and_then(|x| x.as_i64()) else {
        return Ok(None);
    };
    let mut members: Vec<String> = conn
        .smembers(members_key(id).as_bytes())?
        .into_iter()
        .filter_map(|m| String::from_utf8(m).ok())
        .collect();
    members.sort();
    Ok(Some(Group {
        id,
        address: addr.to_string(),
        members,
    }))
}

/// The addresses of every group `member` belongs to (member compared ASCII
/// case-insensitively), sorted.
pub fn addresses_with_member(conn: &mut Connection, member: &str) -> std::io::Result<Vec<String>> {
    let flat = conn.hgetall(GROUPS_KEY.as_bytes())?;
    let mut out = Vec::new();
    for row in flat
        .iter()
        .skip(1)
        .step_by(2)
        .filter_map(|v| serde_json::from_slice::<serde_json::Value>(v).ok())
    {
        let (Some(id), Some(address)) = (
            row.get("id").and_then(|x| x.as_i64()),
            row.get("address").and_then(|a| a.as_str()),
        ) else {
            continue;
        };
        let members = conn.smembers(members_key(id).as_bytes())?;
        if members
            .iter()
            .any(|m| std::str::from_utf8(m).is_ok_and(|m| m.trim().eq_ignore_ascii_case(member)))
        {
            out.push(address.trim().to_string());
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(conn: &mut Connection, id: i64, address: &str, members: &[&str]) {
        let row = serde_json::json!({ "id": id, "address": address, "name": "g" });
        let row = serde_json::to_vec(&row).unwrap();
        conn.hset(
            GROUPS_KEY.as_bytes(),
            &[(id.to_string().as_bytes(), row.as_slice())],
        )
        .unwrap();
        let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
        if !refs.is_empty() {
            conn.sadd(members_key(id).as_bytes(), &refs).unwrap();
        }
    }

    #[test]
    fn lists_the_groups_a_member_belongs_to() {
        let mut conn = Connection::connect("mem://email-groups-member").unwrap();
        put(&mut conn, 1, "all@x.test", &["a@x.test", "b@x.test"]);
        put(&mut conn, 2, "ops@x.test", &["A@X.test"]);
        put(&mut conn, 3, "sales@x.test", &["b@x.test"]);
        assert_eq!(
            addresses_with_member(&mut conn, "a@x.test").unwrap(),
            vec!["all@x.test", "ops@x.test"]
        );
        assert!(
            addresses_with_member(&mut conn, "c@x.test")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn finds_a_group_by_address_with_its_members() {
        let mut conn = Connection::connect("mem://email-groups-find").unwrap();
        put(
            &mut conn,
            1,
            "team@example.test",
            &["b@example.test", "a@example.test"],
        );
        put(&mut conn, 2, "other@example.test", &["c@example.test"]);
        let g = find_by_address(&mut conn, "TEAM@example.test")
            .unwrap()
            .unwrap();
        assert_eq!(g.id, 1);
        assert_eq!(g.members, ["a@example.test", "b@example.test"]);
    }

    #[test]
    fn an_address_that_is_no_group_finds_nothing() {
        let mut conn = Connection::connect("mem://email-groups-miss").unwrap();
        put(&mut conn, 1, "team@example.test", &["a@example.test"]);
        assert_eq!(
            find_by_address(&mut conn, "nobody@example.test").unwrap(),
            None
        );
    }
}
