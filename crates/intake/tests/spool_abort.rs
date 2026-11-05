//! A spool file that kills the process every time it is delivered.
//!
//! Tests run with `panic = "unwind"`, but production aborts, so the death
//! is a real `abort()` in a child process: the test starts its own binary
//! again to run one drain tick, with a core whose store write aborts on a
//! marked message. The spool, the maildirs and the outbound queue (a real
//! kevy-server) outlive each child, as they outlive a restart.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_sidestate::families::email_groups::Group;
use mailrs_core_sidestate::families::outbound::PENDING_IDX;
use mailrs_intake::Core;
use mailrs_intake::route::Directory;
use mailrs_intake::spool::{SpoolDirs, tick};

/// Set for the child: the test root it drains.
const CHILD_ROOT: &str = "MAILRS_TEST_SPOOL_ABORT_ROOT";
const BAD: &str = "1790000000.M1P1.bad";
const GOOD: &str = "1790000001.M2P1.good";

struct TestCore {
    root: PathBuf,
    maildir: String,
}

impl Directory for TestCore {
    fn maildir_root(&self) -> &str {
        &self.maildir
    }
    fn is_account(&self, addr: &str) -> std::io::Result<bool> {
        Ok(matches!(
            addr,
            "alice@golia.jp" | "bob@golia.jp" | "postmaster@golia.jp"
        ))
    }
    fn alias(&self, _: &str) -> std::io::Result<Option<String>> {
        Ok(None)
    }
    fn group(&self, _: &str) -> std::io::Result<Option<Group>> {
        Ok(None)
    }
    fn local_domains(&self) -> Arc<Vec<String>> {
        Arc::new(vec!["golia.jp".into()])
    }
    fn mask(&self, _: &str) -> std::io::Result<Option<Option<String>>> {
        Ok(None)
    }
}

impl Core for TestCore {
    fn net_conn(&self) -> Option<kevy_client::Connection> {
        kevy_client::Connection::connect(&std::env::var("MAILRS_KEVY_URL").ok()?).ok()
    }
    /// Aborts on bob's copy of a marked message while `abort` exists;
    /// otherwise records the call, which stands for everything a core does
    /// for an arrival (the index, the unread count, the push).
    fn ingest(&self, addr: &str, blob_ref: &str, body: &[u8], _: &str, _: Option<&str>) -> bool {
        let marked = String::from_utf8_lossy(body).contains("X-Test-Abort: yes");
        if marked && addr == "bob@golia.jp" && self.root.join("abort").exists() {
            std::process::abort();
        }
        use std::io::Write as _;
        let mut log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("ingested.log"))
            .unwrap();
        writeln!(log, "{addr} {blob_ref}").unwrap();
        true
    }
    fn tag_shared(&self, _: &str, _: &str, _: &str, _: &str) -> Option<String> {
        None
    }
    fn may_send_as(&self, _: &str, _: &str) -> bool {
        false
    }
    fn file_submitted(&self, _: &str, _: &[u8], _: &[String]) -> std::io::Result<()> {
        Ok(())
    }
}

/// The child's half: one drain tick, when started by the test below.
#[test]
fn child_drains_once() {
    let Some(root) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    let core = TestCore {
        maildir: root.join("maildir").to_string_lossy().into_owned(),
        root: root.clone(),
    };
    tick(&SpoolDirs::under(&root.join("spool")), &core);
}

/// Run one drain tick in a child process. True when it exited normally.
fn drain_in_child(root: &Path, kevy: &str) -> bool {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_drains_once", "--test-threads=1"])
        .env(CHILD_ROOT, root)
        .env("MAILRS_KEVY_URL", kevy)
        .status()
        .unwrap();
    status.success()
}

fn spool(root: &Path, name: &str, to: &[&str], body: &str) {
    let env = SpoolEnvelope {
        reverse_path: "dave@outside.example".into(),
        forward_paths: to.iter().map(|s| s.to_string()).collect(),
        is_authenticated: true,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1_790_000_000,
        fraud_verdict: None,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    };
    let new = root.join("spool/incoming/new");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::write(new.join(name), encode_spool_blob(&env, body.as_bytes())).unwrap();
}

/// Every file in `addr`'s maildir, `new/` and `cur/`, whose name starts
/// with `prefix`.
fn copies(root: &Path, addr: &str, prefix: &str) -> usize {
    let (local, domain) = addr.split_once('@').unwrap();
    ["new", "cur"]
        .iter()
        .flat_map(|leaf| {
            std::fs::read_dir(root.join("maildir").join(domain).join(local).join(leaf))
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
        .count()
}

fn ingested(root: &Path, addr: &str, file: &str) -> usize {
    std::fs::read_to_string(root.join("ingested.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| *l == format!("{addr} {file}"))
        .count()
}

fn attempts(root: &Path, file: &str) -> Option<u64> {
    let raw = std::fs::read(root.join("spool/attempts").join(file)).ok()?;
    serde_json::from_slice::<serde_json::Value>(&raw).unwrap()["attempts"].as_u64()
}

fn jobs(kevy: &str) -> usize {
    let mut c = kevy_client::Connection::connect(kevy).unwrap();
    c.lrange(PENDING_IDX, 0, -1).unwrap().len()
}

#[tokio::test]
async fn a_message_that_kills_the_drain_is_quarantined_and_delivered_once_when_put_back() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("abort"), b"").unwrap();
    // carol is relayed out, alice is served, and then bob's copy kills
    // the process
    spool(
        root,
        BAD,
        &["carol@outside.example", "alice@golia.jp", "bob@golia.jp"],
        "From: dave@outside.example\r\nSubject: the secret subject\r\n\
         X-Test-Abort: yes\r\n\r\nthe secret body\r\n",
    );

    for run in 1..=3u64 {
        assert!(!drain_in_child(root, &kevy), "run {run} did not die");
        assert_eq!(attempts(root, BAD), Some(run), "run {run}");
        assert_eq!(copies(root, "alice@golia.jp", BAD), 1, "run {run}");
        assert_eq!(ingested(root, "alice@golia.jp", BAD), 1, "run {run}");
        assert_eq!(jobs(&kevy), 1, "run {run}: carol's relay queued again");
    }

    // other mail arrives while the bad file waits
    spool(
        root,
        GOOD,
        &["alice@golia.jp"],
        "From: dave@outside.example\r\nSubject: fine\r\n\r\nfine\r\n",
    );
    assert!(drain_in_child(root, &kevy), "the fourth run died too");
    let quarantine = root.join("spool/quarantine");
    assert!(quarantine.join(BAD).is_file(), "not quarantined");
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(quarantine.join(format!("{BAD}.json"))).unwrap())
            .unwrap();
    assert_eq!(record["record"]["attempts"], 3);
    assert_eq!(record["record"]["stage"], "rcpt bob@golia.jp");
    assert!(!root.join("spool/incoming/new").join(BAD).exists());
    assert_eq!(attempts(root, BAD), None, "the attempt record stayed");
    assert_eq!(
        ingested(root, "alice@golia.jp", GOOD),
        1,
        "the next mail waited"
    );
    assert_eq!(
        attempts(root, GOOD),
        None,
        "a file delivered cleanly left a record"
    );

    // the postmaster is told, from the envelope alone
    drain_in_child(root, &kevy);
    let pm = root.join("maildir/golia.jp/postmaster/new");
    let notices: Vec<String> = std::fs::read_dir(&pm)
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].contains(&format!("Spool file: {BAD}")));
    assert!(notices[0].contains("RCPT TO: bob@golia.jp"));
    assert!(
        !notices[0].contains("secret"),
        "the notice quotes the message"
    );

    // fixed, and put back by hand; bob's client has filed the copy the
    // dead attempts wrote
    std::fs::remove_file(root.join("abort")).unwrap();
    let bob = root.join("maildir/golia.jp/bob");
    std::fs::rename(
        bob.join("new").join(BAD),
        bob.join("cur").join(format!("{BAD}:2,S")),
    )
    .unwrap();
    std::fs::rename(
        quarantine.join(BAD),
        root.join("spool/incoming/new").join(BAD),
    )
    .unwrap();
    assert!(drain_in_child(root, &kevy));
    assert!(!root.join("spool/incoming/new").join(BAD).exists());
    assert_eq!(ingested(root, "bob@golia.jp", BAD), 1);
    assert_eq!(copies(root, "bob@golia.jp", BAD), 1, "bob has two copies");
    assert_eq!(
        ingested(root, "alice@golia.jp", BAD),
        1,
        "alice served twice"
    );
    assert_eq!(copies(root, "alice@golia.jp", BAD), 1);
    assert_eq!(jobs(&kevy), 1, "carol's relay queued twice");
    assert!(!quarantine.join(format!("{BAD}.json")).exists());
    assert_eq!(attempts(root, BAD), None);
}
