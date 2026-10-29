//! A session is stored under the hash of its token, so the key names a
//! SCAN prints cannot sign anyone in. Sessions issued before that are
//! under the token itself; they keep working, move to the hash on first
//! use, and are ended by logout and by revocation wherever they are.

mod live;

use kevy_client::Connection;
use mailrs_core_sidestate::keys::session::{
    SESSION_PREFIX, by_addr_key, legacy_session_key, session_key, session_ref,
};
use reqwest::Method;

const KEVY: &str = "mem://webapi-sessions-are-stored-by-hash";
const ADMIN: &str = "admin@example.test";
const ALICE: &str = "alice@example.test";

fn exists(kv: &mut Connection, key: &str) -> bool {
    kv.exists(&[key.as_bytes()]).unwrap() == 1
}

fn indexed(kv: &mut Connection) -> Vec<String> {
    let mut members: Vec<String> = kv
        .smembers(by_addr_key(ALICE).as_bytes())
        .unwrap()
        .into_iter()
        .map(|m| String::from_utf8(m).unwrap())
        .collect();
    members.sort();
    members
}

/// Put a session back the way a release before hashing stored it.
fn make_legacy(kv: &mut Connection, token: &str) {
    let blob = kv.get(session_key(token).as_bytes()).unwrap().unwrap();
    kv.del(&[session_key(token).as_bytes()]).unwrap();
    kv.set_with_ttl(
        legacy_session_key(token).as_bytes(),
        &blob,
        std::time::Duration::from_secs(3600),
    )
    .unwrap();
    let idx = by_addr_key(ALICE);
    kv.srem(idx.as_bytes(), &[session_ref(token).as_bytes()])
        .unwrap();
    kv.sadd(idx.as_bytes(), &[token.as_bytes()]).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sessions_live_under_the_hash_of_their_token() {
    // SAFETY: the only test in this binary, before anything reads kevy
    unsafe { std::env::set_var("MAILRS_KEVY_URL", KEVY) };
    let live = live::start(KEVY).await;
    for who in [ADMIN, ALICE] {
        live.add_account(who, "pw").await;
    }
    live.grant(ADMIN, "admin.accounts").await;
    let admin = live.login(ADMIN, "pw").await;
    let me = |token: String| {
        let live = &live;
        async move { live.call(Method::GET, "/api/auth/me", &token, None).await.0 }
    };
    let mut kv = Connection::connect(KEVY).unwrap();

    let first = live.login(ALICE, "pw").await;
    let names: Vec<String> = kv
        .keys(format!("{SESSION_PREFIX}*").as_bytes())
        .unwrap()
        .into_iter()
        .map(|k| String::from_utf8(k).unwrap())
        .collect();
    assert!(names.iter().any(|k| k == &session_key(&first)));
    assert!(
        names.iter().all(|k| !k.contains(&first)),
        "a key name holds the token"
    );
    assert_eq!(indexed(&mut kv), [session_ref(&first)]);

    // an old session is honoured and moved on first use
    make_legacy(&mut kv, &first);
    assert_eq!(me(first.clone()).await, 200);
    assert!(!exists(&mut kv, &legacy_session_key(&first)));
    assert!(exists(&mut kv, &session_key(&first)));
    assert_eq!(indexed(&mut kv), [session_ref(&first)]);
    assert_eq!(me(first.clone()).await, 200);

    let (status, _) = live
        .call(Method::POST, "/api/auth/logout", &first, None)
        .await;
    assert_eq!(status, 204);
    assert!(!exists(&mut kv, &session_key(&first)));
    assert!(indexed(&mut kv).is_empty());
    assert_eq!(me(first).await, 401);

    // an old session never used since is ended by logout
    let unused = live.login(ALICE, "pw").await;
    make_legacy(&mut kv, &unused);
    let (status, _) = live
        .call(Method::POST, "/api/auth/logout", &unused, None)
        .await;
    assert_eq!(status, 204);
    assert!(!exists(&mut kv, &legacy_session_key(&unused)));
    assert!(indexed(&mut kv).is_empty());

    // and by a password reset, both before and after it moved
    let waiting = live.login(ALICE, "pw").await;
    make_legacy(&mut kv, &waiting);
    let moved = live.login(ALICE, "pw").await;
    make_legacy(&mut kv, &moved);
    assert_eq!(me(moved.clone()).await, 200);
    let (status, _) = live
        .call(
            Method::POST,
            "/api/admin/accounts/alice%40example.test/password",
            &admin,
            Some(serde_json::json!({ "password": "a much longer new passphrase" })),
        )
        .await;
    assert_eq!(status, 204);
    assert!(!exists(&mut kv, &legacy_session_key(&waiting)));
    assert!(!exists(&mut kv, &session_key(&moved)));
    assert_eq!(me(waiting).await, 401);
    assert_eq!(me(moved).await, 401);
}
