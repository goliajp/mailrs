//! Mail to an email group's address reaches the group's members: local
//! members in their maildirs, outside members on the outbound queue.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kevy_client::Connection;
use mailrs_core_sidestate::families::email_groups::{GROUPS_KEY, members_key};

const DOMAIN: &str = "golia.example.test";

fn init_env() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: set once, before any test in this binary reads it
    ONCE.call_once(|| unsafe { std::env::set_var("MAILRS_LOCAL_DOMAINS", DOMAIN) });
}

fn put_group(kv: &mut Connection, id: i64, address: &str, members: &[&str]) {
    let row = serde_json::to_vec(&serde_json::json!({ "id": id, "address": address })).unwrap();
    kv.hset(
        GROUPS_KEY.as_bytes(),
        &[(id.to_string().as_bytes(), row.as_slice())],
    )
    .unwrap();
    let refs: Vec<&[u8]> = members.iter().map(|m| m.as_bytes()).collect();
    kv.sadd(members_key(id).as_bytes(), &refs).unwrap();
}

struct World {
    _tmp: tempfile::TempDir,
    maildir: PathBuf,
    incoming: PathBuf,
    state: Arc<mailrs_fastcore::FastcoreState>,
}

impl World {
    fn new(net_url: &str, mailboxes: &[&str]) -> Self {
        init_env();
        let tmp = tempfile::tempdir().unwrap();
        let maildir = tmp.path().join("maildir");
        let incoming = tmp.path().join("spool/incoming/new");
        std::fs::create_dir_all(&incoming).unwrap();
        for local in mailboxes {
            for sub in ["cur", "new", "tmp"] {
                std::fs::create_dir_all(maildir.join(DOMAIN).join(local).join(sub)).unwrap();
            }
        }
        let mailbox = mailrs_mailbox_kevy::KevyMailboxStore::new(Arc::new(
            kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
        ));
        let mut state = mailrs_fastcore::FastcoreState::new(mailbox);
        state.net_url = Some(net_url.to_string());
        World {
            _tmp: tmp,
            maildir,
            incoming,
            state: Arc::new(state),
        }
    }

    fn spool(&self, to: &str) {
        let env = mailrs_core::spool::SpoolEnvelope {
            reverse_path: "sender@example.org".into(),
            forward_paths: vec![to.into()],
            is_authenticated: false,
            auth_user: None,
            conn_id: 1,
            target_folder: "INBOX".into(),
            received_at: 1,
            fraud_verdict: None,
            sandbox_ns: None,
            schema_version: mailrs_core::spool::SPOOL_SCHEMA_VERSION,
        };
        let raw = b"Subject: to the group\r\nMessage-ID: <g1@example.org>\r\n\r\nhello";
        std::fs::write(
            self.incoming.join("1700000000.M1P1.group"),
            mailrs_core::spool::encode_spool_blob(&env, raw),
        )
        .unwrap();
    }

    fn drain(&self) -> usize {
        mailrs_fastcore::spool_drain::drain_once(
            &self.incoming,
            self.maildir.to_str().unwrap(),
            &self.state,
        )
        .0
    }

    fn inbox(&self, local: &str) -> usize {
        count(&self.maildir.join(DOMAIN).join(local).join("new"))
    }

    fn spooled(&self) -> usize {
        count(&self.incoming)
    }
}

fn count(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

#[test]
fn every_local_member_gets_the_message() {
    let url = "mem://email-group-local-members";
    let w = World::new(url, &["alice", "bob", "carol"]);
    let mut kv = Connection::connect(url).unwrap();
    put_group(
        &mut kv,
        1,
        "team@golia.example.test",
        &["alice@golia.example.test", "bob@golia.example.test"],
    );
    w.spool("team@golia.example.test");
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("alice"), 1);
    assert_eq!(w.inbox("bob"), 1);
    assert_eq!(w.inbox("carol"), 0);
    assert_eq!(w.spooled(), 0);
}

#[test]
fn a_domain_catch_all_does_not_take_a_group_s_mail() {
    let url = "mem://email-group-catch-all";
    let w = World::new(url, &["alice", "bob", "carol"]);
    w.state
        .alias_store
        .upsert("@golia.example.test", "carol@golia.example.test")
        .unwrap();
    let mut kv = Connection::connect(url).unwrap();
    put_group(
        &mut kv,
        1,
        "team@golia.example.test",
        &["alice@golia.example.test", "bob@golia.example.test"],
    );
    w.spool("team@golia.example.test");
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("alice"), 1);
    assert_eq!(w.inbox("bob"), 1);
    assert_eq!(w.inbox("carol"), 0, "the catch-all took the group's mail");

    // an address that is no group still falls through to the catch-all
    w.spool("someone@golia.example.test");
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("carol"), 1);
}

#[test]
fn a_nested_group_is_expanded_once_and_nobody_gets_two_copies() {
    let url = "mem://email-group-nested";
    let w = World::new(url, &["alice", "bob", "carol"]);
    let mut kv = Connection::connect(url).unwrap();
    put_group(
        &mut kv,
        1,
        "all@golia.example.test",
        &[
            "alice@golia.example.test",
            "ALICE@golia.example.test",
            "ops@golia.example.test",
        ],
    );
    put_group(
        &mut kv,
        2,
        "ops@golia.example.test",
        &[
            "alice@golia.example.test",
            "bob@golia.example.test",
            "deep@golia.example.test",
        ],
    );
    // a third level that must not be reached
    put_group(
        &mut kv,
        3,
        "deep@golia.example.test",
        &["carol@golia.example.test"],
    );
    w.spool("all@golia.example.test");
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("alice"), 1, "a duplicate member got a second copy");
    assert_eq!(w.inbox("bob"), 1, "the nested group was not expanded");
    assert_eq!(w.inbox("carol"), 0, "a group two levels down was expanded");
}

#[test]
fn an_address_that_is_nothing_stays_in_the_spool() {
    let url = "mem://email-group-none";
    let w = World::new(url, &["alice"]);
    let mut kv = Connection::connect(url).unwrap();
    put_group(
        &mut kv,
        1,
        "team@golia.example.test",
        &["alice@golia.example.test"],
    );
    w.spool("nobody@golia.example.test");
    assert_eq!(w.drain(), 0);
    assert_eq!(w.spooled(), 1);
    assert_eq!(w.inbox("alice"), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_outside_member_is_forwarded_through_the_outbound_queue() {
    use mailrs_core_sidestate::families::outbound::{PENDING_IDX, job_key};
    use testcontainers::core::{IntoContainerPort, WaitFor};
    use testcontainers::runners::AsyncRunner;
    use testcontainers::{GenericImage, ImageExt};

    let startup = mailrs_test_docker::startup_lock().await;
    let container = GenericImage::new("ghcr.io/goliajp/kevy", "6.4.0")
        .with_wait_for(WaitFor::message_on_stderr("starting:"))
        .with_exposed_port(6379.tcp())
        .with_startup_timeout(mailrs_test_docker::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start kevy container");
    let host = container.get_host().await.unwrap().to_string();
    let url = format!(
        "kevy://{}:{}",
        mailrs_test_docker::ipv4_loopback(&host),
        container.get_host_port_ipv4(6379).await.unwrap()
    );
    drop(startup);
    tokio::task::spawn_blocking(move || {
        // SAFETY: the only test in this binary that reads or writes it
        unsafe { std::env::set_var("MAILRS_KEVY_URL", &url) };
        let mut kv = (0..50)
            .find_map(|_| {
                let c = Connection::connect(&url).ok();
                if c.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                c
            })
            .expect("kevy answers");
        let w = World::new(&url, &["alice"]);
        put_group(
            &mut kv,
            1,
            "team@golia.example.test",
            &["alice@golia.example.test", "friend@outside.example.org"],
        );
        w.spool("team@golia.example.test");
        assert_eq!(w.drain(), 1);
        assert_eq!(w.inbox("alice"), 1);
        let ids = kv.lrange(PENDING_IDX, 0, -1).unwrap();
        assert_eq!(ids.len(), 1, "the outside member was not queued");
        let id = String::from_utf8(ids[0].clone()).unwrap();
        let blob = kv.hget(job_key(id).as_bytes(), b"blob").unwrap().unwrap();
        let job: serde_json::Value = serde_json::from_slice(&blob).unwrap();
        assert_eq!(job["recipient"], "friend@outside.example.org");
    })
    .await
    .unwrap();
}
