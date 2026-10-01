//! Removing Send rows.
//!
//! A row is removed with every key that names it — the hash, its
//! recipients, the index and whichever `by_status` zset holds it — so
//! nothing is left for the list to find. Removing from every status zset
//! rather than the one the hash names keeps a row whose status field and
//! zset disagree from surviving the delete.

use super::{Status, by_status_key, index_key, rcpt_key, send_key};

const ALL_STATUSES: [Status; 6] = [
    Status::Scheduled,
    Status::Sending,
    Status::Delivered,
    Status::Failed,
    Status::Partial,
    Status::Cancelled,
];

/// Remove one send row. Returns whether it existed.
pub fn delete_send(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<bool> {
    let removed = conn.del(&[
        send_key(user, send_id).as_bytes(),
        rcpt_key(user, send_id).as_bytes(),
    ])?;
    let indexed = conn.zrem(index_key(user).as_bytes(), &[send_id.as_bytes()])?;
    for status in ALL_STATUSES {
        conn.zrem(
            by_status_key(user, status).as_bytes(),
            &[send_id.as_bytes()],
        )?;
    }
    Ok(removed > 0 || indexed > 0)
}

/// Remove a send and every resend of it (`{root}#r1`, `{root}#r2`, …).
///
/// The list shows one row per message — the newest attempt — so deleting
/// only that attempt would bring the previous one back into view.
/// Returns how many rows were removed.
pub fn delete_send_chain(
    conn: &mut kevy_client::Connection,
    user: &str,
    send_id: &str,
) -> std::io::Result<usize> {
    let root = send_id.split_once("#r").map_or(send_id, |(root, _)| root);
    let resend_prefix = format!("{root}#r");
    let mut removed = 0;
    for id in indexed_ids(conn, user)? {
        if (id == root || id.starts_with(&resend_prefix)) && delete_send(conn, user, &id)? {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Remove every send row that belongs to `thread_id`.
///
/// Deleting a conversation deletes the mail in it; the Send rows for that
/// mail would otherwise stay listed with nothing behind them.
pub fn delete_sends_in_thread(
    conn: &mut kevy_client::Connection,
    user: &str,
    thread_id: &str,
) -> std::io::Result<usize> {
    let mut removed = 0;
    for id in indexed_ids(conn, user)? {
        let tid = conn.hget(send_key(user, &id).as_bytes(), b"thread_id")?;
        if tid.as_deref() == Some(thread_id.as_bytes()) && delete_send(conn, user, &id)? {
            removed += 1;
        }
    }
    Ok(removed)
}

fn indexed_ids(conn: &mut kevy_client::Connection, user: &str) -> std::io::Result<Vec<String>> {
    Ok(conn
        .zrange(index_key(user).as_bytes(), 0, -1)?
        .into_iter()
        .filter_map(|raw| String::from_utf8(raw).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::super::{SendRow, write_send};
    use super::*;

    fn row(send_id: &str, thread_id: &str, resent_from: Option<&str>) -> SendRow {
        SendRow {
            send_id: send_id.into(),
            message_id: send_id.split('#').next().unwrap().into(),
            thread_id: thread_id.into(),
            subject: "s".into(),
            to_csv: "a@x.com".into(),
            cc_csv: String::new(),
            created_at: 1,
            status: Status::Sending,
            envelope_ref: String::new(),
            resent_from: resent_from.map(String::from),
        }
    }

    fn ids(conn: &mut kevy_client::Connection, key: &str) -> Vec<String> {
        conn.zrange(key.as_bytes(), 0, -1)
            .unwrap()
            .into_iter()
            .map(|v| String::from_utf8(v).unwrap())
            .collect()
    }

    #[test]
    fn a_chain_goes_with_its_resends_and_leaves_other_sends() {
        let mut c = kevy_client::Connection::connect("mem://send-delete-chain").unwrap();
        let rcpt = vec!["a@x.com".to_string()];
        write_send(&mut c, "u", &row("m1@x", "t1", None), &rcpt).unwrap();
        write_send(&mut c, "u", &row("m1@x#r1", "t1", Some("m1@x")), &rcpt).unwrap();
        write_send(&mut c, "u", &row("m2@x", "t1", None), &rcpt).unwrap();

        assert_eq!(delete_send_chain(&mut c, "u", "m1@x#r1").unwrap(), 2);
        assert_eq!(ids(&mut c, &index_key("u")), vec!["m2@x"]);
        assert_eq!(
            ids(&mut c, &by_status_key("u", Status::Sending)),
            vec!["m2@x"]
        );
        assert!(
            c.hgetall(send_key("u", "m1@x").as_bytes())
                .unwrap()
                .is_empty()
        );
        assert!(
            c.hgetall(rcpt_key("u", "m1@x").as_bytes())
                .unwrap()
                .is_empty()
        );
        assert_eq!(delete_send_chain(&mut c, "u", "m1@x").unwrap(), 0);
    }

    #[test]
    fn deleting_a_thread_takes_only_its_own_sends() {
        let mut c = kevy_client::Connection::connect("mem://send-delete-thread").unwrap();
        let rcpt = vec!["a@x.com".to_string()];
        write_send(&mut c, "u", &row("m1@x", "t1", None), &rcpt).unwrap();
        write_send(&mut c, "u", &row("m2@x", "t2", None), &rcpt).unwrap();
        write_send(&mut c, "v", &row("m3@x", "t1", None), &rcpt).unwrap();

        assert_eq!(delete_sends_in_thread(&mut c, "u", "t1").unwrap(), 1);
        assert_eq!(ids(&mut c, &index_key("u")), vec!["m2@x"]);
        assert_eq!(ids(&mut c, &index_key("v")), vec!["m3@x"]);
    }
}
