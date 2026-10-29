//! The outbound queue, the audit log and the audit reads of someone else's
//! mailbox all show other people's mail or what they did with it. An
//! administrator holding admin.* permissions used to read them; only a
//! super administrator may, over REST and over MCP alike. The rest of the
//! admin surface still answers an administrator.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-mail-views-are-for-super-administrators";
const ADMIN: &str = "admin@example.test";
const SUPER: &str = "root@example.test";

#[tokio::test]
async fn only_a_super_administrator_sees_other_peoples_mail() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    live.add_account(ADMIN, "pw").await;
    live.add_account(SUPER, "pw").await;
    for p in ["admin.queue", "admin.accounts", "admin.domains"] {
        live.grant(ADMIN, p).await;
    }
    live.mailbox
        .upsert_permissions(
            SUPER,
            &serde_json::json!({
                "address": SUPER, "permissions": [], "groups": [], "is_super": true, "send_as": [],
            })
            .to_string(),
        )
        .expect("super");
    let admin = live.login(ADMIN, "pw").await;
    let root = live.login(SUPER, "pw").await;

    for path in ["/api/admin/queues", "/api/admin/audit-log"] {
        let (status, _) = live.call(Method::GET, path, &admin, None).await;
        assert_eq!(status, 403, "{path} for an administrator");
        let (status, body) = live.call(Method::GET, path, &root, None).await;
        assert_eq!(status, 200, "{path} for a super administrator: {body}");
    }

    // the audit reads also want a recent confirmation, asked only once the
    // caller is let in: a super administrator is asked, an administrator is
    // refused before the question
    let (status, body) = live
        .call(Method::GET, "/api/admin/audit/accounts", &admin, None)
        .await;
    assert_eq!(status, 403);
    assert_ne!(
        body["error"], "confirm_required",
        "the administrator got past the gate"
    );
    let (status, body) = live
        .call(Method::GET, "/api/admin/audit/accounts", &root, None)
        .await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("confirm_required"))
    );

    let (status, body) = live
        .call(Method::GET, "/api/admin/domains", &admin, None)
        .await;
    assert_eq!(status, 200, "the rest of the admin surface: {body}");

    let mut as_admin = live.mcp(&admin).await;
    for tool in ["get_audit_log", "list_admin_queue", "list_failed_outbound"] {
        assert!(
            as_admin.tool(tool, serde_json::json!({})).await.is_err(),
            "{tool} for an administrator"
        );
    }
    let mut as_root = live.mcp(&root).await;
    for tool in ["get_audit_log", "list_admin_queue", "list_failed_outbound"] {
        let answer = as_root.tool(tool, serde_json::json!({})).await;
        assert!(
            answer.is_ok(),
            "{tool} for a super administrator: {answer:?}"
        );
    }
}
