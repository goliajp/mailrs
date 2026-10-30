//! An account's local forwards: the message lands in its own mailbox and a
//! copy in each target's, one hop only, nothing on the outbound queue.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kevy_client::Connection;
use mailrs_core_sidestate::families::local_forward::set_targets;

const DOMAIN: &str = "golia.example.test";

fn init_env() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    // SAFETY: set once, before any test in this binary reads it
    ONCE.call_once(|| unsafe { std::env::set_var("MAILRS_LOCAL_DOMAINS", DOMAIN) });
}

fn forward(kv: &mut Connection, from: &str, to: &[&str]) {
    let to: Vec<String> = to.iter().map(|t| format!("{t}@{DOMAIN}")).collect();
    set_targets(kv, &format!("{from}@{DOMAIN}"), &to).unwrap();
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
        let raw = b"Subject: a reply\r\nMessage-ID: <f1@example.org>\r\n\r\nhello";
        std::fs::write(
            self.incoming.join("1700000000.M1P1.forward"),
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

    fn has_mailbox(&self, local: &str) -> bool {
        self.maildir.join(DOMAIN).join(local).exists()
    }
}

fn count(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

#[test]
fn the_forward_target_gets_a_copy_and_the_account_keeps_its_own() {
    let url = "mem://local-forward-copy";
    let w = World::new(url, &["sentori", "lihao", "carol"]);
    let mut kv = Connection::connect(url).unwrap();
    forward(&mut kv, "sentori", &["lihao"]);
    w.spool(&format!("sentori@{DOMAIN}"));
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("sentori"), 1);
    assert_eq!(w.inbox("lihao"), 1);
    assert_eq!(w.inbox("carol"), 0);
    assert_eq!(w.spooled(), 0);
}

#[test]
fn a_forwarded_copy_is_not_forwarded_again() {
    let url = "mem://local-forward-one-hop";
    let w = World::new(url, &["a", "b", "c"]);
    let mut kv = Connection::connect(url).unwrap();
    forward(&mut kv, "a", &["b"]);
    forward(&mut kv, "b", &["a", "c"]);
    w.spool(&format!("a@{DOMAIN}"));
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("a"), 1);
    assert_eq!(w.inbox("b"), 1);
    assert_eq!(w.inbox("c"), 0);
}

#[test]
fn a_target_that_is_not_a_mailbox_here_gets_nothing() {
    let url = "mem://local-forward-unknown";
    let w = World::new(url, &["a"]);
    let mut kv = Connection::connect(url).unwrap();
    forward(&mut kv, "a", &["a", "ghost"]);
    w.spool(&format!("a@{DOMAIN}"));
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("a"), 1);
    assert!(!w.has_mailbox("ghost"));
}

#[test]
fn without_forwards_nothing_changes() {
    let url = "mem://local-forward-none";
    let w = World::new(url, &["a", "b"]);
    w.spool(&format!("a@{DOMAIN}"));
    assert_eq!(w.drain(), 1);
    assert_eq!(w.inbox("a"), 1);
    assert_eq!(w.inbox("b"), 0);
}
