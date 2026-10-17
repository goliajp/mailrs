//! Reading someone else's mail through the audit routes and the audit MCP
//! tools leaves its audit row by the time the mail comes back.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-audit-read-records-first";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const TID: &str = "t-1@partner.example";

fn rows(kv: &mut kevy_client::Connection) -> Vec<serde_json::Value> {
    kv.lrange(b"admin:audit_log", 0, -1)
        .expect("audit log")
        .iter()
        .map(|r| serde_json::from_slice(r).expect("row"))
        .collect()
}

fn has_row(rows: &[serde_json::Value], action: &str, detail: &str) -> bool {
    rows.iter().any(|r| {
        r["action"] == action
            && r["actor"] == ADMIN
            && r["target"] == ALICE
            && r["detail"] == detail
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_audit_read_is_recorded_before_it_answers() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.add_account(ALICE, "pw").await;
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    live.mailbox.ensure_thread_table();
    live.mailbox
        .record_message_arrival(&mailrs_mailbox_kevy::MessageArrival {
            category: "inbox",
            is_own: false,
            latest_date: 1_786_000_000,
            latest_preview: "hello",
            senders_csv: "Friend <friend@partner.example>",
            subject: "hello",
            thread_id: TID,
            unread: true,
            user: ALICE,
        })
        .expect("arrival");
    let mut kv = kevy_client::Connection::connect(KEVY).expect("kevy");
    assert!(
        rows(&mut kv)
            .iter()
            .all(|r| !r["action"].as_str().unwrap_or("").starts_with("audit."))
    );

    let (status, listed) = live
        .call(
            Method::GET,
            "/api/admin/audit/conversations?user=alice%40example.test",
            &admin,
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert_eq!(listed["items"].as_array().map(Vec::len), Some(1));
    assert!(has_row(
        &rows(&mut kv),
        "audit.list_conversations",
        "via web"
    ));

    let (status, _) = live
        .call(
            Method::GET,
            &format!("/api/admin/audit/conversations/{TID}/messages?user=alice%40example.test"),
            &admin,
            None,
        )
        .await;
    assert_eq!(status, 200);
    assert!(has_row(&rows(&mut kv), "audit.read_thread", TID));

    let mut mcp = live.mcp(&admin).await;
    let before = rows(&mut kv).len();
    mcp.tool(
        "audit_list_conversations",
        serde_json::json!({ "target_user": ALICE }),
    )
    .await
    .expect("audit_list_conversations");
    let after = rows(&mut kv);
    assert_eq!(after.len(), before + 1);
    assert!(has_row(&after, "audit.list_conversations", "via mcp"));
}
