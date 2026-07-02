//! Network-kevy-backed `DmarcReportSink` for the split-topology receiver.
//!
//! Every per-message DMARC verdict lands in a per-domain zset:
//!   `mailrs:dmarc:results:<from_domain>` — score = unix seconds,
//!                                          member = JSON blob
//! and the domain is also added to `mailrs:dmarc:index` (SET) so admin
//! tooling can enumerate every reporting domain without SCAN.
//!
//! Errors are swallowed per the trait contract — a lost aggregate row
//! must never block delivery.

use async_trait::async_trait;

use kevy_client::Connection;
use mailrs_dmarc::DmarcResultRecord;

use crate::inbound::stages::mail_auth::DmarcReportSink;

/// Zset key + set-index key pair. Kept as constants so admin readers
/// and this writer stay in sync.
const RESULTS_KEY_PREFIX: &str = "mailrs:dmarc:results:";
const INDEX_KEY: &[u8] = b"mailrs:dmarc:index";

/// Records DMARC verification outcomes into a shared network kevy so
/// admin tooling (and a future aggregate reporter) can read them.
pub struct KevyDmarcSink {
    kevy_url: String,
}

impl KevyDmarcSink {
    /// Build a new sink with the `kevy://` URL of the shared network kevy.
    pub fn new(kevy_url: impl Into<String>) -> Self {
        Self {
            kevy_url: kevy_url.into(),
        }
    }
}

#[async_trait]
impl DmarcReportSink for KevyDmarcSink {
    async fn record_result(&self, record: &DmarcResultRecord) {
        let url = self.kevy_url.clone();
        let record = clone_record(record);
        // Blocking kevy client — run on the blocking pool so we never
        // stall the accept loop. record_result is fire-and-forget from
        // the pipeline's PoV so the JoinError is best-effort ignored.
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = write_result(&url, &record) {
                tracing::warn!(error = %e, "dmarc: kevy write failed");
            }
        })
        .await;
    }
}

/// Push the record to `mailrs:dmarc:results:<domain>` + add domain to
/// the index set. Idempotent zadd (member is JSON with the timestamp
/// baked in, so re-issuing a lookup after fastcore consumes it is safe).
fn write_result(kevy_url: &str, record: &DmarcResultRecord) -> std::io::Result<()> {
    let mut conn = Connection::open(kevy_url).map_err(std::io::Error::other)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let domain = if record.from_domain.is_empty() {
        "unknown"
    } else {
        &record.from_domain
    };
    let key = format!("{RESULTS_KEY_PREFIX}{domain}");
    let blob = serde_json::json!({
        "recorded_at": now,
        "source_ip": record.source_ip,
        "from_domain": record.from_domain,
        "spf_result": record.spf_result,
        "dkim_result": record.dkim_result,
        "dmarc_result": record.dmarc_result,
        "disposition": record.disposition,
    })
    .to_string();
    conn.zadd(key.as_bytes(), &[(now as f64, blob.as_bytes())])
        .map_err(std::io::Error::other)?;
    conn.sadd(INDEX_KEY, &[domain.as_bytes()])
        .map_err(std::io::Error::other)?;
    Ok(())
}

/// `DmarcResultRecord` is `Clone` but doesn't derive `Send + 'static`
/// bounds via #[derive] in a way rust-analyzer recognises across trait
/// boundaries; explicit shallow clone is a defensive helper.
fn clone_record(r: &DmarcResultRecord) -> DmarcResultRecord {
    DmarcResultRecord {
        source_ip: r.source_ip.clone(),
        from_domain: r.from_domain.clone(),
        spf_result: r.spf_result.clone(),
        dkim_result: r.dkim_result.clone(),
        dmarc_result: r.dmarc_result.clone(),
        disposition: r.disposition.clone(),
    }
}
