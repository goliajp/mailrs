use std::collections::BTreeMap;

use kevy_client::Connection;

use super::registry::{OWNED, Store};
use super::shape::Shape;
use super::*;
use crate::families::{identity_link, masked_email, mock, uploads, webhook_outbox, webhooks};
use crate::keys::{credentials, session};

const ALICE: &str = "alice@x.test";
const BOB: &str = "bob@x.test";

/// A key with its type and content, sorted, so two snapshots compare.
type Snapshot = BTreeMap<Vec<u8>, String>;

fn snapshot(c: &mut Connection) -> Snapshot {
    let mut out = Snapshot::new();
    for key in scan_all(c, "*").unwrap() {
        let kind = c.type_of(&key).unwrap();
        let mut items: Vec<Vec<u8>> = match kind.as_str() {
            "string" => vec![c.get(&key).unwrap().unwrap_or_default()],
            "hash" => {
                let flat = c.hgetall(&key).unwrap();
                flat.chunks(2).map(|p| p.concat()).collect()
            }
            "set" => c.smembers(&key).unwrap(),
            "zset" => c.zrange(&key, 0, -1).unwrap(),
            "list" => c.lrange(&key, 0, -1).unwrap(),
            other => vec![other.as_bytes().to_vec()],
        };
        items.sort();
        out.insert(key, format!("{kind}:{items:?}"));
    }
    out
}

/// One entry in every network family the registry lists for `who`: by the
/// writers that own the families with a reverse index or a value to read,
/// and by the registry's own key builders for the rest.
fn seed(c: &mut Connection, who: &str, tag: &str, now: i64) {
    let token = format!("token-{tag}");
    c.set(session::session_key(&token).as_bytes(), who.as_bytes())
        .unwrap();
    session::index_session(c, who, &token).unwrap();
    c.set(
        format!("agent:key:secret:h:{tag}").as_bytes(),
        serde_json::json!({ "user": who, "id": 1 })
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    c.set(
        credentials::pwreset_by_addr_key(who).as_bytes(),
        token.as_bytes(),
    )
    .unwrap();
    c.hset(
        credentials::pwreset_key(&token).as_bytes(),
        &[(b"address".as_slice(), who.as_bytes())],
    )
    .unwrap();
    identity_link::link(c, "https://idp.test", &format!("sub-{tag}"), who).unwrap();
    for kind in ["token", "refresh", "code"] {
        c.hset(
            format!("oidc:{kind}:{tag}").as_bytes(),
            &[(b"user".as_slice(), who.as_bytes())],
        )
        .unwrap();
    }
    c.hset(
        crate::families::inline_images::key(tag).as_bytes(),
        &[(b"owner".as_slice(), who.as_bytes())],
    )
    .unwrap();
    let sub = webhooks::create(
        c,
        webhooks::NewWebhook {
            account_address: who.into(),
            url: "https://hook.test".into(),
            event_type: "message.received".into(),
            filter_sender: None,
            filter_thread_id: None,
        },
    )
    .unwrap();
    webhook_outbox::enqueue(c, sub.id, who, "{}", now).unwrap();
    masked_email::create_at(
        c,
        who,
        &format!("mask-{tag}@y.test"),
        &masked_email::NewMask {
            for_domain: "shop.test".into(),
            description: String::new(),
            url: None,
            created_by: "test".into(),
            state: None,
        },
        now,
    )
    .unwrap();
    mock::create_namespace(c, who, &format!("ns-{tag}")).unwrap();
    uploads::put(
        &uploads::root(),
        who,
        tag.as_bytes(),
        std::time::SystemTime::now(),
    )
    .unwrap();
    c.hset(
        mailrs_alias_store::keys::alias_v2_key(&format!("alias-{tag}@y.test")).as_bytes(),
        &[(b"target".as_slice(), who.as_bytes())],
    )
    .unwrap();
    c.hset(
        b"admin:aliases",
        &[(
            tag.as_bytes(),
            serde_json::json!({ "target_address": who })
                .to_string()
                .as_bytes(),
        )],
    )
    .unwrap();
    for owned in OWNED.iter().filter(|o| o.store == Store::Network) {
        let keys = match owned.shape {
            Shape::Key(f) => vec![f(who)],
            Shape::Globs(f) => f(who).iter().map(|g| g.replace('*', "7")).collect(),
            Shape::Members { keys, sep } => {
                let member = match sep {
                    Some(s) => format!("{who}{s}1"),
                    None => who.to_string(),
                };
                let k = keys().replace('*', "7");
                c.sadd(k.as_bytes(), &[member.as_bytes()]).unwrap();
                continue;
            }
            Shape::Custom(_) => continue,
        };
        for k in keys {
            if c.exists(&[k.as_bytes()]).unwrap() == 0 {
                c.set(k.as_bytes(), b"1").unwrap();
            }
        }
    }
}

fn network_names() -> Vec<String> {
    let mut names: Vec<String> = OWNED
        .iter()
        .filter(|o| o.store == Store::Network)
        .map(|o| o.name.to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

fn names(counts: &[mailrs_core_api::method::admin::PurgeCount]) -> Vec<String> {
    let mut n: Vec<String> = counts.iter().map(|p| p.name.clone()).collect();
    n.sort();
    n
}

/// Counters every writer bumps and the purge's own records: what may
/// differ from before the purged account was seeded.
fn bookkeeping(key: &[u8]) -> bool {
    [
        b"admin:webhooks:counter".as_slice(),
        b"webhook:outbox:counter",
        crate::keys::admin::AUDIT_LOG,
        crate::keys::admin::AUDIT_LOG_COUNTER,
        pending::PENDING_KEY.as_bytes(),
    ]
    .contains(&key)
}

#[tokio::test]
async fn a_purge_leaves_nothing_of_the_account_and_all_of_the_others() {
    let uploads_root = tempfile::tempdir().unwrap();
    // SAFETY: no other test in this crate reads the variable
    unsafe { std::env::set_var("MAILRS_JMAP_UPLOAD_DIR", uploads_root.path()) };
    let (_server, url) = mailrs_test_docker::kevy::kevy_server().await;
    let mut c = Connection::connect(&url).unwrap();
    let now = 1_800_000_000;
    seed(&mut c, BOB, "bob", now);
    let bob_only = snapshot(&mut c);
    seed(&mut c, ALICE, "alice", now);
    pending::mark(&mut c, ALICE, now - 31 * 86_400).unwrap();
    let before = snapshot(&mut c);

    let counts = count_store(&mut c, Store::Network, ALICE).unwrap();
    assert_eq!(
        names(&counts),
        network_names(),
        "a registered family the seed did not reach, or one the purge could not count"
    );
    let unaccounted = unregistered(&mut c, ALICE).unwrap();
    assert!(unaccounted.is_empty(), "{unaccounted:?}");
    assert_eq!(snapshot(&mut c), before, "counting wrote");

    remove_store(&mut c, Store::Network, ALICE).unwrap();
    assert!(!uploads::account_dir(uploads_root.path(), ALICE).exists());
    assert!(uploads::account_dir(uploads_root.path(), BOB).exists());
    let left = unregistered(&mut c, ALICE).unwrap();
    assert!(left.is_empty(), "keys naming the account remain: {left:?}");
    let after = count_store(&mut c, Store::Network, ALICE).unwrap();
    assert!(after.is_empty(), "{after:?}");
    let row = mailrs_core_api::method::admin::AccountPurgeRow {
        address: ALICE.into(),
        deleted_at: now - 31 * 86_400,
        maildir_files: 0,
        maildir_bytes: 0,
        mail_store: vec![],
        network: counts,
        unregistered: vec![],
    };
    finish(&mut c, &row, now).unwrap();

    let end = snapshot(&mut c);
    let differs: Vec<String> = end
        .keys()
        .chain(bob_only.keys())
        .filter(|k| !bookkeeping(k) && end.get(*k) != bob_only.get(*k))
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect();
    assert!(
        differs.is_empty(),
        "the store is not what it was before the purged account came: {differs:?}"
    );
    let audit = c.lrange(crate::keys::admin::AUDIT_LOG, 0, -1).unwrap();
    let rows: Vec<crate::families::audit::AuditFact> = audit
        .iter()
        .map(|r| serde_json::from_slice(r).unwrap())
        .filter(|f: &crate::families::audit::AuditFact| f.action == PURGED)
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target, ALICE);
    assert!(
        rows[0].detail.contains(&crate::sieve_key("{user}")),
        "{}",
        rows[0].detail
    );
    assert!(pending::all(&mut c).unwrap().is_empty());
}

#[test]
fn retention_decides_what_is_due() {
    let mut c = Connection::connect("mem://purge-due").unwrap();
    let now = 1_800_000_000;
    pending::mark(&mut c, "old@x.test", now - 31 * 86_400).unwrap();
    pending::mark(&mut c, "new@x.test", now - 86_400).unwrap();
    let thirty = due(&mut c, now, 30, None).unwrap();
    assert_eq!(thirty.pending, 2);
    assert_eq!(thirty.due, [("old@x.test".to_string(), now - 31 * 86_400)]);
    let zero = due(&mut c, now, 0, None).unwrap();
    assert_eq!(zero.due.len(), 2);
    let one = due(&mut c, now, 0, Some("new@x.test")).unwrap();
    assert_eq!(one.due, [("new@x.test".to_string(), now - 86_400)]);
    assert!(
        due(&mut c, now, 30, Some("new@x.test"))
            .unwrap()
            .due
            .is_empty()
    );
}

#[test]
fn kept_globs_match_whole_names() {
    assert!(glob_match("zz:*a@x.test*", "zz:1.2.3.4|s@y|a@x.test"));
    assert!(!glob_match("zz:*a@x.test*", "zz:1.2.3.4|s@y|b@x.test"));
    assert!(glob_match("zz:*:a\\*b", "zz:1:2:presence:a*b"));
    assert!(!glob_match("zz:*:a\\*b", "zz:1:2:presence:aXb"));
    assert!(glob_match("x", "x"));
    assert!(!glob_match("x", "xy"));
}
