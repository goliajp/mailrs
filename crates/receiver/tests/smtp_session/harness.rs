//! A receiver on a local port with a stand-in core, and a line-at-a-time
//! SMTP client.

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mailrs_auth_guard::login::MemoryLoginGuard;
use mailrs_core::event_bus::EventBus;
use mailrs_core::users::UserStore;
use mailrs_core_api::method::admin::EffectivePermissionsResponse;
use mailrs_receiver::inbound::rate_limit::{InMemoryRateLimitStore, TokenBucketConfig};
use mailrs_receiver::kevy_net::KevyNetClient;
use mailrs_receiver::smtp_session::spool_sink::MaildirSpoolSink;
use mailrs_receiver::smtp_session::{ConnectionContext, handle_plain_connection};
use mailrs_receiver::{AccountStoreError, CredentialStore, RecipientCheck};
use mailrs_smtp_codec::SmuggleProtection;
use mailrs_spf::{SpfError, SpfResolver};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

pub const ME: &str = "me@golia.test";
pub const MASK: &str = "amber.otter.000042@golia.test";
/// The one local address the stand-in core knows.
pub const REAL: &str = "real.word.123456@golia.test";
/// A domain whose SPF allows 127.0.0.1.
pub const SPF_DOMAIN: &str = "shop.example";

/// The two counters `/api/health` reports; the rest are not looked at.
#[derive(Default)]
pub struct Counts {
    pub tripped: AtomicU64,
    pub log_failed: AtomicU64,
}

impl Counts {
    pub fn tripped(&self) -> u64 {
        self.tripped.load(Ordering::Relaxed)
    }
    pub fn log_failed(&self) -> u64 {
        self.log_failed.load(Ordering::Relaxed)
    }
}

struct Metrics(Arc<Counts>);

impl mailrs_receiver::ConnectionMetrics for Metrics {
    fn on_connect(&self) {}
    fn on_disconnect(&self) {}
    fn on_message_delivered(&self) {}
    fn inbound_accept(&self) {}
    fn inbound_reject(&self) {}
    fn inbound_defer(&self) {}
    fn inbound_junk(&self) {}
    fn probe_guard_tripped(&self) {
        self.0.tripped.fetch_add(1, Ordering::Relaxed);
    }
    fn mask_submit_log_failed(&self) {
        self.0.log_failed.fetch_add(1, Ordering::Relaxed);
    }
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

/// DNS where only [`SPF_DOMAIN`] publishes SPF, allowing 127.0.0.1.
struct Dns;

#[async_trait::async_trait]
impl SpfResolver for Dns {
    async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, SpfError> {
        match domain.trim_end_matches('.') == SPF_DOMAIN {
            true => Ok(vec!["v=spf1 ip4:127.0.0.1 -all".into()]),
            false => Ok(Vec::new()),
        }
    }
    async fn lookup_a(&self, _: &str) -> Result<Vec<IpAddr>, SpfError> {
        Ok(Vec::new())
    }
    async fn lookup_aaaa(&self, _: &str) -> Result<Vec<IpAddr>, SpfError> {
        Ok(Vec::new())
    }
    async fn lookup_mx(&self, _: &str) -> Result<Vec<(u16, String)>, SpfError> {
        Ok(Vec::new())
    }
}

/// A core that knows exactly one address, [`REAL`].
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

pub struct Receiver {
    pub port: u16,
    pub counts: Arc<Counts>,
    pub spool: std::path::PathBuf,
    pub ctx: Arc<ConnectionContext>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.spool);
    }
}

/// A receiver for `golia.test` asking the stand-in core about local
/// recipients, with the network kevy at `kevy` (none when `None`).
pub async fn receiver(name: &str, kevy: Option<&str>) -> Receiver {
    receiver_with(name, kevy, false).await
}

/// [`receiver`], with the inbound scan (`run_antispam`) on or off.
pub async fn receiver_with(name: &str, kevy: Option<&str>, antispam: bool) -> Receiver {
    build(name, kevy, antispam, Arc::new(OneMask)).await
}

/// [`receiver`], asking `credentials` for password hashes.
pub async fn receiver_asking(name: &str, credentials: Arc<dyn CredentialStore>) -> Receiver {
    build(name, None, false, credentials).await
}

async fn build(
    name: &str,
    kevy: Option<&str>,
    antispam: bool,
    credentials: Arc<dyn CredentialStore>,
) -> Receiver {
    let spool = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&spool).unwrap();
    let counts = Arc::new(Counts::default());
    let ctx = Arc::new(ConnectionContext {
        hostname: "mx.golia.test".into(),
        tls_state: None,
        users: Arc::new(UserStore::from_plain_passwords(vec![(
            ME.into(),
            "pw".into(),
        )])),
        event_bus: EventBus::new(64),
        metrics: Arc::new(Metrics(counts.clone())),
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
        antispam_enabled: antispam,
        smuggle_protection: SmuggleProtection::Off,
        submission_from_check: mailrs_receiver::config::SubmissionFromCheck::Enforce,
        auth_guard: Arc::new(MemoryLoginGuard::default()),
        account_store: None,
        credential_store: Some(credentials),
        recipient_check: Some(RecipientCheck::new(spawn_core().await, "secret".into())),
        ldap_config: None,
        inbound_pipeline: mailrs_inbound::Pipeline::builder().build(),
        spam_lists_client: kevy.map(|url| Arc::new(KevyNetClient::new(url))),
        site_visits: Arc::new(mailrs_receiver::site_visit::SiteVisits::new(4, 2)),
        spool_sink: Arc::new(MaildirSpoolSink::new(spool.to_str().unwrap())),
        signins: Default::default(),
        unknown_rcpt: Default::default(),
        rcpt_spf: Some(Arc::new(Dns)),
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let serving = ctx.clone();
    tokio::spawn(async move {
        loop {
            let (stream, peer) = listener.accept().await.unwrap();
            tokio::spawn(handle_plain_connection(stream, peer, serving.clone()));
        }
    });
    Receiver {
        port,
        counts,
        spool,
        ctx,
    }
}

pub struct Client {
    r: BufReader<tokio::net::tcp::OwnedReadHalf>,
    w: tokio::net::tcp::OwnedWriteHalf,
}

impl Client {
    /// Connected and past EHLO.
    pub async fn connect(port: u16) -> Self {
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

    /// Signed in as [`ME`].
    pub async fn signed_in(port: u16) -> Self {
        use base64::Engine as _;
        let mut c = Client::connect(port).await;
        let creds = base64::engine::general_purpose::STANDARD.encode(format!("\0{ME}\0pw"));
        let r = c.say(&format!("AUTH PLAIN {creds}")).await;
        assert!(r.starts_with("235"), "{r}");
        c
    }

    /// The last line of the reply; empty once the server has hung up.
    pub async fn reply(&mut self) -> String {
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

    pub async fn say(&mut self, cmd: &str) -> String {
        self.send(cmd).await;
        self.reply().await
    }

    /// Send `cmd` without waiting for the reply.
    pub async fn send(&mut self, cmd: &str) {
        self.w
            .write_all(format!("{cmd}\r\n").as_bytes())
            .await
            .unwrap();
    }
}

/// Every file under `dir`, read whole.
pub fn files_under(dir: &std::path::Path) -> Vec<Vec<u8>> {
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
