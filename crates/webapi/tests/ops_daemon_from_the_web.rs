//! The admin page's ops routes, against a real ops daemon: only a super
//! administrator reads the state, changes need a recent confirmation and
//! name the administrator, a silence and maintenance cannot be set without
//! an end, and when the daemon is gone the state says so instead of
//! answering an empty list. The shared secret appears in no answer and in
//! no log line.

mod live;

use std::io::Write;
use std::sync::{Arc, Mutex};

use reqwest::Method;
use serde_json::json;
use sha2::Digest;

const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";
const SECRET: &str = "ops-admin-secret-7f3a9c";

/// Every log line any part of the test writes.
#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

async fn daemon(data: &std::path::Path) -> (String, tokio::task::JoinHandle<()>) {
    let mut cfg = mailrs_ops::config::Config::for_tests(data.to_path_buf());
    cfg.admin_secret_sha256 = Some(hex::encode(sha2::Sha256::digest(SECRET.as_bytes())));
    let app = mailrs_ops::app::App::open(
        cfg,
        mailrs_ops::clock::Clock::System,
        mailrs_ops::store::Store::open(&data.join("ops.sqlite")).unwrap(),
    )
    .unwrap();
    // one completed cycle, so the state has a last cycle to show
    app.cycle().await;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let router = mailrs_ops::api::tcp_router(app);
    let server = tokio::spawn(async move {
        let _ = axum::serve(l, router).await;
    });
    (format!("http://{addr}"), server)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_admin_page_reads_and_changes_the_ops_daemon() {
    let logs = Logs::default();
    let writer = logs.clone();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .init();

    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    let data = tempfile::tempdir().expect("ops data");
    let (ops_url, ops_server) = daemon(data.path()).await;
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
        std::env::set_var("MAILRS_OPS_URL", &ops_url);
        std::env::set_var("MAILRS_OPS_ADMIN_SECRET", SECRET);
        std::env::set_var("MAILRS_OPS_SILENT_SECS", "600");
    }
    let live = live::start(&kevy).await;
    for who in [ADMIN, ALICE] {
        live.add_account(who, "pw").await;
    }
    live.make_super(ADMIN);
    let admin = live.login(ADMIN, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let mut answers = Vec::new();

    let (status, state) = live
        .call(Method::GET, "/api/admin/ops/state", &admin, None)
        .await;
    assert_eq!(status, 200, "{state}");
    assert_eq!(state["reach"], "ok", "{state}");
    assert_eq!(state["stale"], false, "a cycle just completed: {state}");
    assert_eq!(state["silent_after_secs"], 600);
    assert!(
        state["state"]["last_cycle"]["started_at"].is_i64(),
        "{state}"
    );
    assert!(
        !state["state"]["rules"].as_array().unwrap().is_empty(),
        "{state}"
    );
    answers.push(state);

    // a person who is not a super administrator sees nothing of it
    for path in ["/api/admin/ops/state", "/api/admin/ops/history"] {
        let (status, _) = live.call(Method::GET, path, &alice, None).await;
        assert_eq!(status, 403, "{path}");
    }
    let silence = json!({
        "rule": "spool_quarantined", "object": "spool", "for_secs": 3600, "reason": "known file",
    });
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/ops/silences",
            &alice,
            Some(silence.clone()),
        )
        .await;
    assert_eq!(status, 403);

    // changes wait for a recent confirmation
    let (status, got) = live
        .call(
            Method::POST,
            "/api/admin/ops/silences",
            &admin,
            Some(silence.clone()),
        )
        .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (403, Some("confirm_required"))
    );
    live.confirm(&admin, "pw").await;

    // no silence without an end, and the caller cannot name someone else
    let (status, got) = live
        .call(
            Method::POST,
            "/api/admin/ops/silences",
            &admin,
            Some(json!({ "rule": "spool_quarantined", "for_secs": 0, "reason": "x" })),
        )
        .await;
    assert_eq!(status, 400, "{got}");
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/ops/silences",
            &admin,
            Some(json!({ "rule": "spool_quarantined", "for_secs": 60, "reason": "x", "by": "someone" })),
        )
        .await;
    assert_eq!(status, 422, "a request may not say who made it");
    // the daemon's own refusal comes back with its reason
    let (status, got) = live
        .call(
            Method::POST,
            "/api/admin/ops/silences",
            &admin,
            Some(json!({ "rule": "no_such_rule", "for_secs": 60, "reason": "x" })),
        )
        .await;
    assert_eq!(status, 400);
    assert!(
        got["error"].as_str().unwrap().contains("no rule named"),
        "{got}"
    );

    let (status, set) = live
        .call(
            Method::POST,
            "/api/admin/ops/silences",
            &admin,
            Some(silence),
        )
        .await;
    assert_eq!(status, 200, "{set}");
    assert_eq!(set["by"], format!("webapi:{ADMIN}"));
    let id = set["id"].as_str().unwrap().to_string();
    answers.push(set);

    // maintenance needs an end to start or extend
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/ops/maintenance",
            &admin,
            Some(json!({ "action": "start", "reason": "upgrade" })),
        )
        .await;
    assert_eq!(status, 400);
    for (action, for_secs) in [("start", Some(1800)), ("extend", Some(3600)), ("end", None)] {
        let (status, got) = live
            .call(
                Method::POST,
                "/api/admin/ops/maintenance",
                &admin,
                Some(json!({ "action": action, "for_secs": for_secs, "reason": "upgrade" })),
            )
            .await;
        assert_eq!(status, 200, "{action}: {got}");
        answers.push(got);
    }
    let (status, got) = live
        .call(
            Method::POST,
            "/api/admin/ops/maintenance",
            &admin,
            Some(json!({ "action": "end", "reason": "again" })),
        )
        .await;
    assert_eq!(status, 409, "ending maintenance that is not on: {got}");

    let end = format!("/api/admin/ops/silences/{id}/end");
    let (status, got) = live
        .call(
            Method::POST,
            &end,
            &admin,
            Some(json!({ "reason": "fixed" })),
        )
        .await;
    assert_eq!(status, 200, "{got}");
    let (status, _) = live
        .call(
            Method::POST,
            &end,
            &admin,
            Some(json!({ "reason": "fixed" })),
        )
        .await;
    assert_eq!(status, 404, "a silence already ended");

    let (status, history) = live
        .call(Method::GET, "/api/admin/ops/history", &admin, None)
        .await;
    assert_eq!(status, 200, "{history}");
    let controls = history["controls"].as_array().unwrap();
    let by_admin = |kind: &str, action: &str| {
        controls.iter().any(|c| {
            c["kind"] == kind && c["action"] == action && c["by"] == format!("webapi:{ADMIN}")
        })
    };
    for (kind, action) in [
        ("silence", "set"),
        ("silence", "removed"),
        ("maintenance", "start"),
        ("maintenance", "extend"),
        ("maintenance", "end"),
    ] {
        assert!(by_admin(kind, action), "{kind} {action}: {history}");
    }
    answers.push(history);

    // the same through MCP, with the same checks
    let mut mcp = live.mcp(&admin).await;
    let state = mcp.tool("get_ops_state", json!({})).await.expect("state");
    assert_eq!(state["reach"], "ok", "{state}");
    let zero = json!({ "rule": "spool_quarantined", "for_secs": 0, "reason": "x" });
    assert!(mcp.tool("set_ops_silence", zero).await.is_err());
    let set = mcp
        .tool(
            "set_ops_silence",
            json!({ "rule": "spool_quarantined", "for_secs": 600, "reason": "through mcp" }),
        )
        .await
        .expect("silence");
    assert_eq!(set["by"], format!("webapi:{ADMIN}"));
    let id = set["id"].as_str().unwrap();
    mcp.tool("end_ops_silence", json!({ "id": id, "reason": "done" }))
        .await
        .expect("end silence");
    let mut not_super = live.mcp(&alice).await;
    assert!(not_super.tool("get_ops_state", json!({})).await.is_err());
    answers.push(state);
    answers.push(set);

    // the daemon goes away: the state says so, writes are refused
    ops_server.abort();
    let _ = ops_server.await;
    let (status, state) = live
        .call(Method::GET, "/api/admin/ops/state", &admin, None)
        .await;
    assert_eq!(status, 200);
    assert_eq!(state["reach"], "unreachable", "{state}");
    assert!(state.get("state").is_none(), "{state}");
    answers.push(state);
    let (status, got) = live
        .call(
            Method::POST,
            "/api/admin/ops/maintenance",
            &admin,
            Some(json!({ "action": "start", "for_secs": 60, "reason": "x" })),
        )
        .await;
    assert_eq!(status, 502, "{got}");
    assert!(
        got["error"].as_str().unwrap().contains("cannot reach"),
        "{got}"
    );
    answers.push(got);

    for a in &answers {
        assert!(
            !a.to_string().contains(SECRET),
            "the secret in an answer: {a}"
        );
    }
    let logged = String::from_utf8_lossy(&logs.0.lock().unwrap()).into_owned();
    assert!(
        logged.contains("ops daemon not reached"),
        "the log capture saw nothing"
    );
    assert!(!logged.contains(SECRET), "the secret in the log");

    let mut c = kevy_client::Connection::connect(&kevy).unwrap();
    let audit: Vec<serde_json::Value> = c
        .lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice(r).ok())
        .collect();
    for action in [
        "ops.silence.set",
        "ops.silence.end",
        "ops.maintenance.start",
        "ops.maintenance.extend",
        "ops.maintenance.end",
    ] {
        assert!(
            audit
                .iter()
                .any(|r| r["action"] == action && r["actor"] == ADMIN),
            "{action} not audited"
        );
    }
    assert!(!serde_json::json!(audit).to_string().contains(SECRET));
}
