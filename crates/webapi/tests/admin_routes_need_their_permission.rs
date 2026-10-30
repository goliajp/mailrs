//! Every admin route needs the permission `route_permissions` declares
//! for it, and nothing else lets a caller through: an account holding
//! only that permission is let in, one holding any other admin permission
//! is refused, and so is one holding none. A super administrator is let
//! in everywhere. The table and the router list the same routes.
//!
//! "Let in" is anything but the gate's bare 403: the request carries no
//! real body, so the handler may well answer 400, 404 or a 403 that names
//! its own reason, but the gate opened.

mod live;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mailrs_webapi::handlers::route_permissions::{ADMIN_ROUTES, Need};
use reqwest::Method;

const KEVY: &str = "mem://webapi-admin-routes-need-their-permission";
const SUPER: &str = "super@example.test";
const NOBODY: &str = "nobody@example.test";

/// Every admin route in the router sources, with the methods it answers.
fn admin_routes() -> BTreeMap<String, BTreeSet<&'static str>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("src/router.rs")];
    for entry in std::fs::read_dir(root.join("src/router")).expect("router dir") {
        files.push(entry.expect("entry").path());
    }
    let mut out: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    for file in files {
        let code: String = std::fs::read_to_string(&file)
            .expect("router source")
            .lines()
            .map(|l| l.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let bytes = code.as_bytes();
        let mut from = 0;
        while let Some(at) = code[from..].find(".route(") {
            let start = from + at + ".route(".len();
            let (mut depth, mut end) = (1, start);
            while depth > 0 && end < bytes.len() {
                match bytes[end] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                end += 1;
            }
            let call = code[start..end.saturating_sub(1)].trim_start();
            from = end;
            let Some(rest) = call.strip_prefix('"') else {
                continue;
            };
            let Some(close) = rest.find('"') else {
                continue;
            };
            let path = &rest[..close];
            if !path.starts_with("/api/admin") {
                continue;
            }
            let spec = &rest[close + 1..];
            let methods = out.entry(path.to_string()).or_default();
            for m in ["get", "post", "put", "delete", "patch"] {
                if spec.contains(&format!("{m}(")) {
                    methods.insert(m);
                }
            }
        }
    }
    out
}

/// A concrete path for a route template.
fn concrete(template: &str) -> String {
    template
        .replace("{address}", "someone%40example.test")
        .replace("{name}", "example.test")
        .replace("{client_id}", "c1")
        .replace("{thread_id}", "t1")
        .replace("{key}", "no_such_key")
        // ids nothing has: a delete that hit a real row would remove what
        // the test granted its accounts with
        .replace("{uid}", "999999")
        .replace("{id}", "999999")
}

fn holder(permission: &str) -> String {
    format!("{}@example.test", permission.replace(['.', '_'], "-"))
}

#[test]
fn the_table_and_the_router_name_the_same_routes() {
    let routed: BTreeSet<String> = admin_routes().into_keys().collect();
    let declared: BTreeSet<String> = ADMIN_ROUTES.iter().map(|(t, _)| t.to_string()).collect();
    let missing: Vec<_> = routed.difference(&declared).collect();
    let stale: Vec<_> = declared.difference(&routed).collect();
    assert!(
        missing.is_empty(),
        "routes route_permissions does not declare: {missing:?}"
    );
    assert!(
        stale.is_empty(),
        "declared routes the router no longer has: {stale:?}"
    );
}

/// Written out here, not read from the table: what shows other people's
/// mail, what they read or how they sign in, what writes their sieve,
/// locks them out or holds their mail is for super administrators, and a
/// change to the table that loosens one of these fails here.
#[test]
fn what_reaches_other_peoples_mail_stays_with_super_administrators() {
    const SUPER_PREFIXES: &[&str] = &[
        "/api/admin/audit",
        "/api/admin/auth-events",
        "/api/admin/export",
        "/api/admin/outbound",
        "/api/admin/password-clear",
        "/api/admin/queues",
        "/api/admin/spam-feedback-stats",
        "/api/admin/suppressions",
    ];
    for (template, need) in ADMIN_ROUTES {
        let kept =
            SUPER_PREFIXES.iter().any(|p| template.starts_with(p)) || template.ends_with("/sieve");
        if kept {
            assert_eq!(
                *need,
                Need::Super,
                "{template} is open to more than super administrators"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_admin_route_lets_in_only_whom_it_names() {
    // SAFETY: the only async test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    let permissions: BTreeSet<&str> = ADMIN_ROUTES
        .iter()
        .filter_map(|(_, need)| match need {
            Need::Permission(p) => Some(*p),
            _ => None,
        })
        .collect();
    let mut tokens: BTreeMap<String, String> = BTreeMap::new();
    let mut everyone: Vec<String> = vec![SUPER.into(), NOBODY.into()];
    everyone.extend(permissions.iter().map(|p| holder(p)));
    for who in &everyone {
        live.add_account(who, "pw").await;
    }
    live.make_super(SUPER);
    for p in &permissions {
        live.grant(&holder(p), p).await;
    }
    for who in &everyone {
        let token = live.login(who, "pw").await;
        live.confirm(&token, "pw").await;
        tokens.insert(who.clone(), token);
    }

    let routes = admin_routes();
    let mut wrong = Vec::new();
    for (template, need) in ADMIN_ROUTES {
        let path = concrete(template);
        for method in routes.get(*template).into_iter().flatten() {
            let method = Method::from_bytes(method.to_uppercase().as_bytes()).unwrap();
            let body = (method != Method::GET).then(|| serde_json::json!({}));
            for who in &everyone {
                let allowed = match need {
                    Need::Super => who == SUPER,
                    Need::AnyAdmin => who != NOBODY,
                    Need::Permission(p) => who == SUPER || *who == holder(p),
                };
                let (status, got) = live
                    .call(method.clone(), &path, &tokens[who], body.clone())
                    .await;
                // the gate refuses with a bare 403; a 403 that names its
                // reason came from the handler, past the gate
                let refused = status == 403 && got.is_null();
                if refused == allowed {
                    wrong.push(format!("{method} {template} as {who}: {status} {got}"));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong answers:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
