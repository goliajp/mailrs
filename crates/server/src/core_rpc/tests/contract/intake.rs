//! Mail in through the receiver's spool, out through the core-api, on both
//! cores: the same spooled messages, drained by each core, must read alike.

use std::path::Path;
use std::sync::Arc;

use mailrs_core::spool::{SPOOL_SCHEMA_VERSION, SpoolEnvelope, encode_spool_blob};
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::{ConversationFilter, ConversationSummaryWire};
use mailrs_receiver::smtp_session::SpoolSink;
use mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink;

use super::harness::{self, Cores, Diffs};
use super::{ALICE, accounts};

/// Ids and counters each store keeps for itself.
const MINTED: &[&str] = &["id", "mailbox_id", "created_at", "modseq"];

pub(super) fn envelope(verdict: Option<String>) -> SpoolEnvelope {
    SpoolEnvelope {
        reverse_path: "carol@outside.example".into(),
        forward_paths: vec![ALICE.into()],
        is_authenticated: false,
        auth_user: None,
        conn_id: 1,
        target_folder: "INBOX".into(),
        received_at: 1_790_000_000,
        fraud_verdict: verdict,
        sandbox_ns: None,
        schema_version: SPOOL_SCHEMA_VERSION,
    }
}

const REPORT: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Quarterly report\r\n\
Message-ID: <q1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 09:00:00 +0900\r\n\
\r\n\
Please review the attached figures before Friday.\r\n";

const REPLY: &[u8] = b"From: Carol <carol@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Re: Quarterly report\r\n\
Message-ID: <q2@outside.example>\r\n\
In-Reply-To: <q1@outside.example>\r\n\
References: <q1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 10:00:00 +0900\r\n\
\r\n\
One more figure.\r\n";

const OWN: &[u8] = b"From: Alice <alice@example.test>\r\n\
To: carol@outside.example\r\n\
Subject: my own note\r\n\
Message-ID: <own1@example.test>\r\n\
Date: Tue, 5 Aug 2025 11:00:00 +0900\r\n\
\r\n\
sent from my phone\r\n";

const PHISH: &[u8] = b"From: Bank <alerts@outside.example>\r\n\
To: alice@example.test\r\n\
Subject: Your account is locked\r\n\
Message-ID: <p1@outside.example>\r\n\
Date: Tue, 5 Aug 2025 12:00:00 +0900\r\n\
\r\n\
Confirm your password here.\r\n";

fn held() -> Option<String> {
    Some(
        serde_json::json!({
            "rules_version": "2026-08-29.1",
            "score": 9.5,
            "quarantined": true,
            "layers": [],
        })
        .to_string(),
    )
}

/// Both cores' spools, and how each is drained. Each drains into the
/// maildir root its core serves from, so mail spooled here is the mail the
/// core's other calls see on disk.
pub(super) struct Spools {
    fastcore: tempfile::TempDir,
    pg_core: tempfile::TempDir,
    pg: Arc<crate::intake::PgCore>,
}

impl Spools {
    pub(super) fn new(c: &Cores) -> Self {
        let s = &c.pg_core_state;
        let pg = Arc::new(crate::intake::PgCore {
            dir: crate::intake::PgDirectory::new(
                s.domain.clone(),
                s.maildir_root.clone(),
                s.net_url.clone(),
            ),
            mailbox: s.mailbox.clone(),
            event_bus: crate::event_bus::EventBus::new(64),
        });
        Self {
            fastcore: tempfile::tempdir().expect("spool"),
            pg_core: tempfile::tempdir().expect("spool"),
            pg,
        }
    }

    /// Spool `body` as the receiver does, once for each core, and drain
    /// both. One message at a time, so each finds the one before it.
    pub(super) async fn arrive(&self, c: &Cores, env: &SpoolEnvelope, body: &[u8]) {
        let blob = encode_spool_blob(env, body);
        for root in [self.fastcore.path(), self.pg_core.path()] {
            MaildirSpoolSink::new(&root.to_string_lossy())
                .write(&blob)
                .await
                .expect("spool write");
        }
        let state = c.fastcore_state.clone();
        let dir = incoming(self.fastcore.path());
        let maildir = c.fastcore_state.maildir_root.clone();
        let fc = tokio::task::spawn_blocking(move || {
            mailrs_fastcore::spool_drain::drain_once(&dir, &maildir, &state).0
        });
        let (core, dirs) = (
            self.pg.clone(),
            mailrs_intake::spool::SpoolDirs::under(self.pg_core.path()),
        );
        let pg = tokio::task::spawn_blocking(move || mailrs_intake::spool::tick(&dirs, &*core));
        assert_eq!(
            fc.await.expect("fastcore drain"),
            1,
            "fastcore took nothing"
        );
        assert_eq!(pg.await.expect("pg drain"), 1, "pg-core took nothing");
    }
}

fn incoming(root: &Path) -> std::path::PathBuf {
    root.join("incoming").join("new")
}

async fn list(
    c: &Cores,
    d: &mut Diffs,
    name: &str,
    filter: ConversationFilter,
) -> Vec<ConversationSummaryWire> {
    let req = ListConversationsRequest { filter };
    d.both(
        &format!("list_conversations [{name}] after spooled mail"),
        c.fastcore.list_conversations(ALICE, &req).await,
        c.pg_core.list_conversations(ALICE, &req).await,
        MINTED,
    )
    .map(|(_, pg)| pg.items)
    .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mail_spooled_by_the_receiver_reads_alike_from_both_cores() {
    let cores = harness::start_on(
        "mem://intake-fastcore-side-state",
        "mem://intake-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    let spools = Spools::new(&cores);

    spools.arrive(&cores, &envelope(None), REPORT).await;
    spools.arrive(&cores, &envelope(None), REPLY).await;
    spools.arrive(&cores, &envelope(None), OWN).await;
    spools.arrive(&cores, &envelope(held()), PHISH).await;

    let all = ConversationFilter {
        limit: 50,
        ..Default::default()
    };
    let rows = list(&cores, &mut d, "default", all.clone()).await;
    let report = rows
        .iter()
        .find(|r| r.thread_id == "q1@outside.example")
        .expect("the report's conversation is listed");
    assert_eq!(report.subject, "Re: Quarterly report");
    assert_eq!(report.message_count, 2, "the reply did not join its thread");
    assert_eq!(report.unread_count, 2);
    assert_eq!(report.category, "inbox");
    assert!(
        report.snippet.starts_with("One more figure"),
        "preview was {:?}",
        report.snippet
    );
    assert!(
        rows.iter().all(|r| r.thread_id != "p1@outside.example"),
        "a held conversation is listed"
    );

    let sent = list(
        &cores,
        &mut d,
        "sent",
        ConversationFilter {
            folder: Some("Sent".into()),
            ..all
        },
    )
    .await;
    let own = sent
        .iter()
        .find(|r| r.thread_id == "own1@example.test")
        .expect("the user's own mail is in Sent");
    assert_eq!(own.unread_count, 0, "the user's own mail arrived unread");

    let quarantined = d.both(
        "list_quarantined after spooled mail",
        cores.fastcore.list_quarantined(ALICE, 50, None).await,
        cores.pg_core.list_quarantined(ALICE, 50, None).await,
        MINTED,
    );
    let (_, pg_held) = quarantined.expect("both cores list what they hold");
    let json = serde_json::to_string(&pg_held).expect("json");
    assert!(json.contains("p1@outside.example"), "nothing held: {json}");

    assert!(
        d.0.is_empty(),
        "{} difference(s) between the cores:\n\n{}",
        d.0.len(),
        d.0.join("\n\n")
    );
}
