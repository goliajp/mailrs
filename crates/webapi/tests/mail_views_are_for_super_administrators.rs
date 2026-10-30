//! The outbound queue and its suppressions, the audit log, the audit reads
//! of someone else's mailbox, sign-in events, per-user spam feedback and
//! anyone's sieve script all show other people's mail traffic, reading or
//! signing in. An administrator holding admin.* permissions used to read
//! them; only a super administrator may, over REST and over MCP alike. The
//! rest of the admin surface still answers an administrator.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-mail-views-are-for-super-administrators";
const ADMIN: &str = "admin@example.test";
const SUPER: &str = "root@example.test";
const TOOLS: [&str; 5] = [
    "get_audit_log",
    "list_admin_queue",
    "list_failed_outbound",
    "list_scheduled_outbound",
    "list_auth_events",
];

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
    live.make_super(SUPER);
    let admin = live.login(ADMIN, "pw").await;
    let root = live.login(SUPER, "pw").await;

    for path in [
        "/api/admin/queues",
        "/api/admin/suppressions",
        "/api/admin/audit-log",
        "/api/admin/auth-events",
        "/api/admin/spam-feedback-stats",
        "/api/admin/accounts/admin%40example.test/sieve",
    ] {
        let (status, _) = live.call(Method::GET, path, &admin, None).await;
        assert_eq!(status, 403, "{path} for an administrator");
        let (status, body) = live.call(Method::GET, path, &root, None).await;
        assert_ne!(status, 403, "{path} for a super administrator: {body}");
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

    // nor may an administrator change someone's sieve script
    let sieve = "/api/admin/accounts/admin%40example.test/sieve";
    live.confirm(&admin, "pw").await;
    let script = serde_json::json!({ "script": "keep;" });
    let (status, _) = live.call(Method::POST, sieve, &admin, Some(script)).await;
    assert_eq!(status, 403, "writing a sieve script");
    let (status, _) = live.call(Method::DELETE, sieve, &admin, None).await;
    assert_eq!(status, 403, "deleting a sieve script");

    let (status, body) = live
        .call(Method::GET, "/api/admin/domains", &admin, None)
        .await;
    assert_eq!(status, 200, "the rest of the admin surface: {body}");

    let mut as_admin = live.mcp(&admin).await;
    for tool in TOOLS {
        assert!(
            as_admin.tool(tool, serde_json::json!({})).await.is_err(),
            "{tool} for an administrator"
        );
    }
    let mut as_root = live.mcp(&root).await;
    for tool in [
        "get_audit_log",
        "list_admin_queue",
        "list_failed_outbound",
        "list_scheduled_outbound",
    ] {
        let answer = as_root.tool(tool, serde_json::json!({})).await;
        assert!(
            answer.is_ok(),
            "{tool} for a super administrator: {answer:?}"
        );
    }
}
