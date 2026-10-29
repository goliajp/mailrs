//! Admin directory writes against a real in-process kevy, through the API as
//! an administrator who has confirmed a password: what a create stores is
//! what the list reports, and a delete leaves nothing behind.

mod live;

use mailrs_core_sidestate::families::permission_groups::{GROUPS_KEY, members_key};
use reqwest::Method;

const KEVY: &str = "mem://webapi-admin-directory";
const ADMIN: &str = "admin@example.test";
const PASSWORD: &str = "an admin passphrase";

async fn confirmed_admin(live: &live::Live) -> String {
    live.add_account(ADMIN, PASSWORD).await;
    live.grant(ADMIN, "admin.accounts").await;
    let token = live.login(ADMIN, PASSWORD).await;
    let (status, _) = live
        .call(
            Method::POST,
            "/api/auth/confirm",
            &token,
            Some(serde_json::json!({ "password": PASSWORD })),
        )
        .await;
    assert_eq!(status, 204, "confirm");
    token
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn groups_store_what_they_are_given_and_deletes_leave_nothing() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    let admin = confirmed_admin(&live).await;
    let call = |method: Method, path: String, body: Option<serde_json::Value>| {
        let (live, admin) = (&live, admin.clone());
        async move { live.call(method, &path, &admin, body).await }
    };
    let members = |id: i64| {
        call(
            Method::GET,
            format!("/api/admin/email-groups/{id}/members"),
            None,
        )
    };

    // an email group's initial members land in the member set
    let (status, g) = call(
        Method::POST,
        "/api/admin/email-groups".into(),
        Some(serde_json::json!({
            "address": "team@example.test", "domain": "example.test", "name": "Team",
            "members": ["a@example.test"],
        })),
    )
    .await;
    assert_eq!(status, 200, "{g}");
    let id = g["id"].as_i64().expect("id");
    assert!(
        g.get("members").is_none(),
        "members are not copied into the row"
    );
    assert_eq!(
        members(id).await.1["items"],
        serde_json::json!(["a@example.test"])
    );

    let (status, _) = call(
        Method::POST,
        format!("/api/admin/email-groups/{id}/members"),
        Some(serde_json::json!({ "address": "b@example.test" })),
    )
    .await;
    assert!((200..300).contains(&status), "add member: {status}");
    let mut now: Vec<String> =
        serde_json::from_value(members(id).await.1["items"].clone()).expect("strings");
    now.sort();
    assert_eq!(now, ["a@example.test", "b@example.test"]);

    // deleting the group deletes its members, so a later group with the
    // same id cannot inherit them
    let (status, _) = call(
        Method::DELETE,
        format!("/api/admin/email-groups/{id}"),
        None,
    )
    .await;
    assert!((200..300).contains(&status), "delete: {status}");
    assert_eq!(members(id).await.1["items"], serde_json::json!([]));

    // a permission group reports itself not builtin and claims no domain:
    // nothing limits its permissions to one
    let (status, created) = call(
        Method::POST,
        "/api/admin/groups".into(),
        Some(serde_json::json!({ "name": "ops" })),
    )
    .await;
    assert_eq!(status, 200, "{created}");
    let (_, list) = call(Method::GET, "/api/admin/groups".into(), None).await;
    let row = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == created["id"])
        .cloned()
        .expect("listed");
    assert!(row["domain"].is_null());
    assert_eq!(row["is_builtin"], false);

    // a builtin group refuses deletion and keeps its members; an ordinary
    // one goes
    let mut c = kevy_client::Connection::connect(KEVY).unwrap();
    let builtin = serde_json::json!({ "id": 900, "name": "super", "is_builtin": true });
    c.hset(
        GROUPS_KEY.as_bytes(),
        &[(b"900".as_slice(), builtin.to_string().as_bytes())],
    )
    .unwrap();
    c.sadd(
        members_key(900).as_bytes(),
        &[b"root@example.test".as_slice()],
    )
    .unwrap();
    assert_eq!(
        call(Method::DELETE, "/api/admin/groups/900".into(), None)
            .await
            .0,
        409
    );
    assert!(c.hget(GROUPS_KEY.as_bytes(), b"900").unwrap().is_some());
    assert_eq!(c.smembers(members_key(900).as_bytes()).unwrap().len(), 1);

    let ordinary = created["id"].as_i64().unwrap();
    assert_eq!(
        call(
            Method::DELETE,
            format!("/api/admin/groups/{ordinary}"),
            None
        )
        .await
        .0,
        204
    );
    let (_, list) = call(Method::GET, "/api/admin/groups".into(), None).await;
    assert!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|g| g["id"] != ordinary)
    );

    // a weak password is refused before the core is asked
    let (status, _) = call(
        Method::POST,
        "/api/admin/accounts/qa%40example.test/password".into(),
        Some(serde_json::json!({ "password": "" })),
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = call(
        Method::POST,
        "/api/admin/accounts".into(),
        Some(serde_json::json!({
            "address": "qa@example.test", "display_name": "QA",
            "password": "an-example-passphrase",
        })),
    )
    .await;
    assert_eq!(status, 400, "contains the account name");
}
