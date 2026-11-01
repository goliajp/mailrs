//! Whole SMTP conversations against `handle_plain_connection`.
//!
//! Guessing local addresses at RCPT: after ten unknown recipients an IP
//! gets `421` for every local address, existing or not, and is
//! disconnected. Submitting from a masked address: the message names
//! neither the client's IP nor its EHLO name, and the audit log records
//! both the account and the IP.

use std::sync::Arc;

use base64::Engine as _;
use mailrs_auth_guard::login::MemoryLoginGuard;
use mailrs_core::event_bus::EventBus;
use mailrs_core::users::UserStore;
use mailrs_core_api::method::admin::EffectivePermissionsResponse;
use mailrs_core_sidestate::families::audit::AuditFact;
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;
use mailrs_receiver::inbound::rate_limit::{InMemoryRateLimitStore, TokenBucketConfig};
use mailrs_receiver::kevy_net::KevyNetClient;
use mailrs_receiver::smtp_session::probe_guard::LIMIT;
use mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink;
use mailrs_receiver::smtp_session::{ConnectionContext, handle_plain_connection};
use mailrs_receiver::{AccountStoreError, CredentialStore, RecipientCheck};
use mailrs_smtp_codec::SmuggleProtection;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const ME: &str = "me@golia.test";
const MASK: &str = "amber.otter.000042@golia.test";

struct NoMetrics;

impl mailrs_receiver::ConnectionMetrics for NoMetrics {
    fn on_connect(&self) {}
    fn on_disconnect(&self) {}
    fn on_message_delivered(&self) {}
    fn inbound_accept(&self) {}
    fn inbound_reject(&self) {}
    fn inbound_defer(&self) {}
    fn inbound_junk(&self) {}
}

/// The core's answer for [`ME`]: an ordinary account with one mask.
struct OneMask;

#[async_trait::async_trait]
impl CredentialStore for OneMask {
    async fn password_hash(&self, _: &str) -> Result<Option<String>, AccountStoreError> {
        Ok(None)
    }
    async fn effective_permissions(
        &self,
        address: &str,
    ) -> Result<Option<EffectivePermissionsResponse>, AccountStoreError> {
        Ok((address == ME).then(|| EffectivePermissionsResponse {
            address: ME.into(),
            permissions: Vec::new(),
            groups: Vec::new(),
            is_super: false,
            send_as: Vec::new(),
            masks: vec![MASK.into()],
        }))
    }
    async fn password_cleared(&self, _: &str) -> Result<bool, AccountStoreError> {
        Ok(false)
    }
}

/// A core that knows exactly one address, `real.word.123456@golia.test`.
async fn spawn_core() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                let head = String::from_utf8_lossy(&buf[..n]);
                let deliverable = head.contains("real.word.123456");
                let body = format!("{{\"deliverable\":{deliverable}}}");
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                s.write_all(resp.as_bytes()).await.unwrap();
            });
        }
    });
    format!("http://{addr}")
}

struct Setup {
    recipient_check: Option<RecipientCheck>,
    kevy: Option<Arc<KevyNetClient>>,
}

async fn spawn_receiver(setup: Setup, spool: &std::path::Path) -> u16 {
    let ctx = Arc::new(ConnectionContext {
        hostname: "mx.golia.test".into(),
        tls_state: None,
        users: Arc::new(UserStore::from_plain_passwords(vec![(
            ME.into(),
            "pw".into(),
        )])),
        event_bus: EventBus::new(64),
        metrics: Arc::new(NoMetrics),
        rate_limiter: Arc::new(InMemoryRateLimitStore::new(TokenBucketConfig {
            capacity: 100_000,
            refill_rate: 100_000.0,
        })),
        runtime: vec!["golia.test".to_string()].into(),
        org_names: Vec::new(),
        org_name_allowed_domains: Vec::new(),
        trusted_relays: Vec::new(),
        resolver: None,
        dnsbl_zones: Vec::new(),
        dnsbl_enabled: false,
        antispam_enabled: false,
        smuggle_protection: SmuggleProtection::Off,
        submission_from_check: mailrs_receiver::config::SubmissionFromCheck::Enforce,
        auth_guard: Arc::new(MemoryLoginGuard::default()),
        account_store: None,
        credential_store: Some(Arc::new(OneMask)),
        recipient_check: setup.recipient_check,
        ldap_config: None,
        inbound_pipeline: mailrs_inbound::Pipeline::builder().build(),
        spam_lists_client: setup.kevy,
        spool_sink: Arc::new(MaildirSpoolSink::new(spool.to_str().unwrap())),
        signins: Default::default(),
        unknown_rcpt: Default::default(),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (stream, peer) = listener.accept().await.unwrap();
            tokio::spawn(handle_plain_connection(stream, peer, ctx.clone()));
        }
    });
    port
}

struct Client {
    r: BufReader<tokio::net::tcp::OwnedReadHalf>,
    w: tokio::net::tcp::OwnedWriteHalf,
}

impl Client {
    async fn connect(port: u16) -> Self {
        let (r, w) = TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap()
            .into_split();
        let mut c = Client {
            r: BufReader::new(r),
            w,
        };
        c.reply().await;
        c.say("EHLO probe-host.example").await;
        c
    }

    /// The last line of the reply; empty once the server has hung up.
    async fn reply(&mut self) -> String {
        loop {
            let mut line = String::new();
            if self.r.read_line(&mut line).await.unwrap() == 0 {
                return String::new();
            }
            if line.as_bytes().get(3) != Some(&b'-') {
                return line;
            }
        }
    }

    async fn say(&mut self, cmd: &str) -> String {
        self.w
            .write_all(format!("{cmd}\r\n").as_bytes())
            .await
            .unwrap();
        self.reply().await
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every file under `dir`, read whole.
fn files_under(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(std::fs::read(&path).unwrap());
        }
    }
    out
}

#[tokio::test]
async fn guessing_local_addresses_is_cut_off_after_ten_misses() {
    let spool = temp_dir("rcpt-probing");
    let setup = Setup {
        recipient_check: Some(RecipientCheck::new(spawn_core().await, "secret".into())),
        kevy: None,
    };
    let port = spawn_receiver(setup, &spool).await;

    let mut c = Client::connect(port).await;
    c.say("MAIL FROM:<x@probe.example>").await;
    assert!(
        c.say("RCPT TO:<real.word.123456@golia.test>")
            .await
            .starts_with("250"),
        "an existing address is accepted"
    );
    for i in 0..LIMIT {
        let r = c.say(&format!("RCPT TO:<guess.{i}@golia.test>")).await;
        assert!(r.starts_with("550 5.1.1"), "miss {i}: {r}");
    }
    let r = c.say("RCPT TO:<real.word.123456@golia.test>").await;
    assert!(
        r.starts_with("421 4.7.0"),
        "once blocked, an existing address gets the same answer: {r}"
    );
    assert_eq!(c.reply().await, "", "the connection is closed");

    let mut again = Client::connect(port).await;
    again.say("MAIL FROM:<x@probe.example>").await;
    let r = again.say("RCPT TO:<someone@remote.example>").await;
    assert!(
        !r.is_empty() && !r.starts_with("421"),
        "a remote recipient is not guarded: {r}"
    );
    let r = again.say("RCPT TO:<guess.x@golia.test>").await;
    assert!(
        r.starts_with("421"),
        "a new connection from the same IP: {r}"
    );
    std::fs::remove_dir_all(spool).unwrap();
}

#[tokio::test]
async fn a_submission_from_a_mask_is_audited_and_names_no_client() {
    let spool = temp_dir("mask-submit");
    let url = format!("mem://mask-submit-{}", std::process::id());
    // held for the whole test: the in-process store goes with its last
    // connection
    let mut kv = kevy_client::Connection::connect(&url).unwrap();
    let setup = Setup {
        recipient_check: None,
        kevy: Some(Arc::new(KevyNetClient::new(url.clone()))),
    };
    let port = spawn_receiver(setup, &spool).await;

    let mut c = Client::connect(port).await;
    let creds = base64::engine::general_purpose::STANDARD.encode(format!("\0{ME}\0pw"));
    let r = c.say(&format!("AUTH PLAIN {creds}")).await;
    assert!(r.starts_with("235"), "{r}");
    assert!(c.say(&format!("MAIL FROM:<{ME}>")).await.starts_with("250"));
    assert!(
        c.say("RCPT TO:<shop@remote.example>")
            .await
            .starts_with("250")
    );
    assert!(c.say("DATA").await.starts_with("354"));
    let r = c
        .say(&format!(
            "From: Shopper <{MASK}>\r\nTo: shop@remote.example\r\nSubject: order\r\n\r\nwhere is it\r\n."
        ))
        .await;
    assert!(r.starts_with("250"), "{r}");

    let spooled = files_under(&spool);
    assert_eq!(spooled.len(), 1, "one spooled message");
    let (envelope, body) = mailrs_core::spool::decode_spool_blob(&spooled[0]).unwrap();
    assert_eq!(envelope.reverse_path, MASK, "queued under the mask");
    let text = String::from_utf8_lossy(body);
    let head = text.split("\r\n\r\n").next().unwrap();
    assert!(head.contains("Received: by mx.golia.test"), "{head}");
    assert!(!head.contains("127.0.0.1"), "the client IP leaked:\n{head}");
    assert!(
        !head.contains("probe-host"),
        "the EHLO name leaked:\n{head}"
    );
    assert!(!head.contains(ME), "the account leaked:\n{head}");

    let rows = kv.lrange(AUDIT_LOG, 0, -1).unwrap();
    let facts: Vec<AuditFact> = rows
        .iter()
        .map(|r| serde_json::from_slice(r).unwrap())
        .collect();
    assert_eq!(facts.len(), 1, "{facts:?}");
    let f = &facts[0];
    assert_eq!(f.actor, ME);
    assert_eq!(f.action, "mask.submit");
    assert_eq!(f.target, "amber.ot");
    assert_eq!(f.detail, "ip=127.0.0.1");
    assert!(f.occurred_at > 0);
    std::fs::remove_dir_all(spool).unwrap();
}
