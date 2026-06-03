#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! DMARC (RFC 7489) — policy parsing, identifier alignment, message
//! evaluation, and aggregate-report generation.
//!
//! ## 1.1 — full mail-auth replacement
//!
//! As of 1.1 this crate **fully replaces** the DMARC half of
//! `stalwart/mail-auth`:
//!
//! - [`policy::DmarcPolicy::parse`] — TXT record parser (RFC 7489 §6.3).
//! - [`align::check`] — identifier alignment, strict + relaxed (RFC 7489 §3.1).
//! - [`eval::evaluate`] — pure-function DMARC outcome from SPF + DKIM
//!   verdicts + policy (RFC 7489 §6.6).
//!
//! The original 1.0 surface (aggregate reporting, store trait, XML
//! builder, mailto extraction) is unchanged:
//!
//! - **Result recording** — [`DmarcStore`] trait + a Postgres reference
//!   impl ([`PgDmarcStore`]) behind the default `pg-store` feature.
//! - **Aggregate XML report generation** —
//!   [`generate_dmarc_report_xml`] produces the canonical
//!   `<feedback>` document a `rua` mailbox expects.
//! - **Report-mail formatting** — [`format_report_email`] wraps the
//!   gzipped XML into the multipart/mixed envelope per §7.2.1.
//! - **`rua` extraction** — [`extract_rua_from_dmarc_record`] pulls a
//!   `mailto:` URI out of a `_dmarc.<domain>` TXT record.
//!
//! The crate is store-agnostic by default: implement [`DmarcStore`] over
//! whatever you have (SQLite, Redis, S3 + flat files), feed verified
//! results in via [`DmarcStore::record_result`], and at report time
//! pull a day's results with [`DmarcStore::get_results_for_date`] and
//! pass them to [`generate_dmarc_report_xml`].
//!
//! ## Example
//!
//! ```no_run
//! use mailrs_dmarc::{
//!     DmarcResultRecord, format_report_email, generate_dmarc_report_xml,
//! };
//!
//! let results = vec![DmarcResultRecord {
//!     source_ip: "192.0.2.1".into(),
//!     from_domain: "example.com".into(),
//!     spf_result: "pass".into(),
//!     dkim_result: "pass".into(),
//!     dmarc_result: "pass".into(),
//!     disposition: "none".into(),
//! }];
//! let xml = generate_dmarc_report_xml(
//!     "Example Inc.", "postmaster@reporter.example",
//!     "example.com!2026-05-20", "example.com",
//!     1715990400, 1716076800, &results,
//! );
//! let email = format_report_email(
//!     "postmaster@reporter.example", "rua@example.com",
//!     "example.com", "example.com!2026-05-20",
//!     "2026-05-20", &xml,
//! );
//! ```

use std::collections::HashMap;
use std::io::Write;

use async_trait::async_trait;

pub mod align;
pub mod eval;
pub mod policy;

pub use align::{check as align_check, organizational_domain};
pub use eval::{DkimSignatureResult, DmarcInput, DmarcOutcome, SpfResult, evaluate};
pub use policy::{Alignment, DmarcParseError, DmarcPolicy, PolicyAction};

/// One verified DMARC result, recorded per inbound message.
///
/// Two equivalent ways to construct one:
///
/// ```
/// use mailrs_dmarc::DmarcResultRecord;
/// // Fluent constructor — preferred for the canonical 6-field record.
/// let r = DmarcResultRecord::new(
///     "192.0.2.1", "example.com", "pass", "pass", "pass", "none",
/// );
/// # let _ = r;
///
/// // Struct literal — still supported, useful with partial values.
/// let r = DmarcResultRecord {
///     source_ip: "192.0.2.1".into(),
///     ..Default::default()
/// };
/// # let _ = r;
/// ```
///
/// **Forward-compatibility note:** this struct is not `#[non_exhaustive]`
/// in 1.x to keep struct-literal call sites working. Future fields that
/// don't fit the 6-field shape will arrive in a 2.0 release alongside
/// `#[non_exhaustive]`.
#[derive(Debug, Clone, Default)]
pub struct DmarcResultRecord {
    /// Client-IP that delivered the message.
    pub source_ip: String,
    /// Domain in the `From:` header.
    pub from_domain: String,
    /// SPF verification result (`pass` / `fail` / `softfail` / `neutral` / `none`).
    pub spf_result: String,
    /// DKIM verification result (`pass` / `fail` / `neutral` / `none`).
    pub dkim_result: String,
    /// DMARC alignment + policy result (`pass` / `fail`).
    pub dmarc_result: String,
    /// Action taken (`none` / `quarantine` / `reject`).
    pub disposition: String,
}

impl DmarcResultRecord {
    /// Construct a [`DmarcResultRecord`] from the 6 canonical fields.
    ///
    /// Use this constructor in code outside the `mailrs-dmarc` crate
    /// because [`DmarcResultRecord`] is `#[non_exhaustive]` and cannot
    /// be built with struct-literal syntax from other crates.
    pub fn new(
        source_ip: impl Into<String>,
        from_domain: impl Into<String>,
        spf_result: impl Into<String>,
        dkim_result: impl Into<String>,
        dmarc_result: impl Into<String>,
        disposition: impl Into<String>,
    ) -> Self {
        Self {
            source_ip: source_ip.into(),
            from_domain: from_domain.into(),
            spf_result: spf_result.into(),
            dkim_result: dkim_result.into(),
            dmarc_result: dmarc_result.into(),
            disposition: disposition.into(),
        }
    }
}

/// Pluggable storage for DMARC verification results.
///
/// Implementations should be cheap to clone (`Arc<Self>` style) since
/// callers typically share a single instance across the inbound
/// pipeline and the daily report task.
#[async_trait]
pub trait DmarcStore: Send + Sync {
    /// Backend-specific error type returned by every trait method.
    type Error: std::fmt::Debug + Send;

    /// Append a verified result. Called per-message during inbound.
    async fn record_result(&self, record: &DmarcResultRecord) -> Result<(), Self::Error>;

    /// Fetch all results recorded on `date` (YYYY-MM-DD). Called once
    /// per day by the report generator.
    async fn get_results_for_date(&self, date: &str)
    -> Result<Vec<DmarcResultRecord>, Self::Error>;

    /// Prune results older than `days`. Called periodically.
    async fn cleanup_old(&self, days: i64) -> Result<u64, Self::Error>;
}

#[cfg(feature = "pg-store")]
pub use pg::PgDmarcStore;

#[cfg(feature = "pg-store")]
mod pg {
    use async_trait::async_trait;
    use sqlx::PgPool;

    use super::{DmarcResultRecord, DmarcStore};

    /// Postgres-backed [`DmarcStore`]. Expects a table:
    ///
    /// ```sql
    /// CREATE TABLE dmarc_results (
    ///   source_ip   text NOT NULL,
    ///   from_domain text NOT NULL,
    ///   spf_result  text NOT NULL,
    ///   dkim_result text NOT NULL,
    ///   dmarc_result text NOT NULL,
    ///   disposition text NOT NULL,
    ///   report_date date NOT NULL DEFAULT CURRENT_DATE
    /// );
    /// CREATE INDEX dmarc_results_by_date ON dmarc_results(report_date);
    /// ```
    pub struct PgDmarcStore {
        pool: PgPool,
    }

    impl PgDmarcStore {
        /// Construct a [`PgDmarcStore`] from an existing pool. The caller
        /// owns the pool lifecycle.
        pub fn new(pool: PgPool) -> Self {
            Self { pool }
        }
    }

    #[async_trait]
    impl DmarcStore for PgDmarcStore {
        type Error = sqlx::Error;

        async fn record_result(&self, record: &DmarcResultRecord) -> Result<(), sqlx::Error> {
            sqlx::query(
                "INSERT INTO dmarc_results (source_ip, from_domain, spf_result, dkim_result, dmarc_result, disposition)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&record.source_ip)
            .bind(&record.from_domain)
            .bind(&record.spf_result)
            .bind(&record.dkim_result)
            .bind(&record.dmarc_result)
            .bind(&record.disposition)
            .execute(&self.pool)
            .await?;
            Ok(())
        }

        async fn get_results_for_date(
            &self,
            date: &str,
        ) -> Result<Vec<DmarcResultRecord>, sqlx::Error> {
            let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
                "SELECT source_ip, from_domain, spf_result, dkim_result, dmarc_result, disposition
                 FROM dmarc_results WHERE report_date = $1::date",
            )
            .bind(date)
            .fetch_all(&self.pool)
            .await?;
            Ok(rows
                .into_iter()
                .map(|r| DmarcResultRecord {
                    source_ip: r.0,
                    from_domain: r.1,
                    spf_result: r.2,
                    dkim_result: r.3,
                    dmarc_result: r.4,
                    disposition: r.5,
                })
                .collect())
        }

        async fn cleanup_old(&self, days: i64) -> Result<u64, sqlx::Error> {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
            let cutoff_date = cutoff.format("%Y-%m-%d").to_string();
            let result = sqlx::query("DELETE FROM dmarc_results WHERE report_date < $1::date")
                .bind(cutoff_date)
                .execute(&self.pool)
                .await?;
            Ok(result.rows_affected())
        }
    }
}

/// Aggregated row for report generation.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AggKey {
    source_ip: String,
    from_domain: String,
    disposition: String,
    dkim_result: String,
    spf_result: String,
}

/// Generate a DMARC aggregate report XML body (RFC 7489 §12.4).
pub fn generate_dmarc_report_xml(
    org_name: &str,
    email: &str,
    report_id: &str,
    domain: &str,
    begin_ts: i64,
    end_ts: i64,
    results: &[DmarcResultRecord],
) -> String {
    let mut agg: HashMap<AggKey, u32> = HashMap::new();
    for r in results {
        let key = AggKey {
            source_ip: r.source_ip.clone(),
            from_domain: r.from_domain.clone(),
            disposition: r.disposition.clone(),
            dkim_result: r.dkim_result.clone(),
            spf_result: r.spf_result.clone(),
        };
        *agg.entry(key).or_insert(0) += 1;
    }

    // Pre-size the output: each record is ~600 bytes of XML; the
    // fixed header is ~400 bytes. Avoids `String` growth-doubling
    // re-allocations on the per-record loop.
    use std::fmt::Write as _;
    let mut xml = String::with_capacity(512 + agg.len() * 600);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n");
    xml.push_str("<feedback>\n");

    xml.push_str("  <report_metadata>\n");
    let _ = writeln!(xml, "    <org_name>{}</org_name>", XmlEscape(org_name));
    let _ = writeln!(xml, "    <email>{}</email>", XmlEscape(email));
    let _ = writeln!(xml, "    <report_id>{report_id}</report_id>");
    xml.push_str("    <date_range>\n");
    let _ = writeln!(xml, "      <begin>{begin_ts}</begin>");
    let _ = writeln!(xml, "      <end>{end_ts}</end>");
    xml.push_str("    </date_range>\n");
    xml.push_str("  </report_metadata>\n");

    xml.push_str("  <policy_published>\n");
    let _ = writeln!(xml, "    <domain>{}</domain>", XmlEscape(domain));
    xml.push_str("    <adkim>r</adkim>\n");
    xml.push_str("    <aspf>r</aspf>\n");
    xml.push_str("    <p>none</p>\n");
    xml.push_str("    <sp>none</sp>\n");
    xml.push_str("    <pct>100</pct>\n");
    xml.push_str("  </policy_published>\n");

    let mut keys: Vec<_> = agg.keys().collect();
    keys.sort_by(|a, b| (&a.source_ip, &a.from_domain).cmp(&(&b.source_ip, &b.from_domain)));

    for key in keys {
        let count = agg[key];
        xml.push_str("  <record>\n");
        xml.push_str("    <row>\n");
        let _ = writeln!(xml, "      <source_ip>{}</source_ip>", key.source_ip);
        let _ = writeln!(xml, "      <count>{count}</count>");
        xml.push_str("      <policy_evaluated>\n");
        let _ = writeln!(
            xml,
            "        <disposition>{}</disposition>",
            key.disposition
        );
        let _ = writeln!(xml, "        <dkim>{}</dkim>", key.dkim_result);
        let _ = writeln!(xml, "        <spf>{}</spf>", key.spf_result);
        xml.push_str("      </policy_evaluated>\n");
        xml.push_str("    </row>\n");
        xml.push_str("    <identifiers>\n");
        let _ = writeln!(
            xml,
            "      <header_from>{}</header_from>",
            XmlEscape(&key.from_domain)
        );
        xml.push_str("    </identifiers>\n");
        xml.push_str("    <auth_results>\n");
        xml.push_str("      <spf>\n");
        let _ = writeln!(
            xml,
            "        <domain>{}</domain>",
            XmlEscape(&key.from_domain)
        );
        let _ = writeln!(xml, "        <result>{}</result>", key.spf_result);
        xml.push_str("      </spf>\n");
        xml.push_str("      <dkim>\n");
        let _ = writeln!(
            xml,
            "        <domain>{}</domain>",
            XmlEscape(&key.from_domain)
        );
        let _ = writeln!(xml, "        <result>{}</result>", key.dkim_result);
        xml.push_str("      </dkim>\n");
        xml.push_str("    </auth_results>\n");
        xml.push_str("  </record>\n");
    }

    xml.push_str("</feedback>\n");
    xml
}

/// Newtype wrapper that escapes XML special chars (`& < > "`) when
/// rendered via `{}`. Used inside `write!()` calls in
/// [`generate_dmarc_report_xml`] so the escape runs directly into
/// the destination `String` instead of allocating an intermediate
/// per call site.
struct XmlEscape<'a>(&'a str);

impl std::fmt::Display for XmlEscape<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Fast path: input has no special characters → write as-is
        // in one call. Most DMARC report fields (domains, IPs,
        // result enums) hit this path.
        if !self
            .0
            .bytes()
            .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"'))
        {
            return f.write_str(self.0);
        }
        // Slow path: byte-iter, escaping the four XML special chars.
        for c in self.0.chars() {
            match c {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                _ => std::fmt::Write::write_char(f, c)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
fn escape_xml(s: &str) -> String {
    // Test-only helper that wraps [`XmlEscape`] to produce an owned
    // `String`. Prod XML emission goes through `write!(... XmlEscape(...))`
    // directly to skip this intermediate allocation.
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() + 8);
    let _ = write!(out, "{}", XmlEscape(s));
    out
}

fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// Format a DMARC aggregate report email with a gzipped XML attachment.
pub fn format_report_email(
    from: &str,
    to: &str,
    org_domain: &str,
    report_id: &str,
    date: &str,
    xml: &str,
) -> Vec<u8> {
    use mailrs_mail_builder::{Attachment, MessageBuilder};
    let gz = gzip_compress(xml.as_bytes());
    let filename = format!("{org_domain}!{to}!{date}!{report_id}.xml.gz");
    MessageBuilder::new()
        .from(from)
        .to(to)
        .subject(format!(
            "Report domain: {org_domain} Submitter: {from} Report-ID: <{report_id}>",
        ))
        .text_body(format!(
            "DMARC aggregate report for {org_domain} ({date})\r\n"
        ))
        .attachment(Attachment::new(filename, "application/gzip", gz))
        .build()
}

/// Extract the `rua` mailbox from a `_dmarc.<domain>` TXT record.
pub fn extract_rua_from_dmarc_record(txt: &str) -> Option<String> {
    for part in txt.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("rua=") {
            for uri in value.split(',') {
                let uri = uri.trim();
                if let Some(addr) = uri.strip_prefix("mailto:") {
                    return Some(addr.to_string());
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_report_xml_basic() {
        let results = vec![
            DmarcResultRecord {
                source_ip: "1.2.3.4".into(),
                from_domain: "example.com".into(),
                spf_result: "pass".into(),
                dkim_result: "pass".into(),
                dmarc_result: "pass".into(),
                disposition: "none".into(),
            },
            DmarcResultRecord {
                source_ip: "1.2.3.4".into(),
                from_domain: "example.com".into(),
                spf_result: "pass".into(),
                dkim_result: "pass".into(),
                dmarc_result: "pass".into(),
                disposition: "none".into(),
            },
            DmarcResultRecord {
                source_ip: "5.6.7.8".into(),
                from_domain: "example.com".into(),
                spf_result: "fail".into(),
                dkim_result: "fail".into(),
                dmarc_result: "fail".into(),
                disposition: "reject".into(),
            },
        ];

        let xml = generate_dmarc_report_xml(
            "Test Org",
            "dmarc@test.com",
            "rpt-001",
            "test.com",
            1000000,
            1086400,
            &results,
        );

        assert!(xml.contains("<org_name>Test Org</org_name>"));
        assert!(xml.contains("<email>dmarc@test.com</email>"));
        assert!(xml.contains("<report_id>rpt-001</report_id>"));
        assert!(xml.contains("<begin>1000000</begin>"));
        assert!(xml.contains("<end>1086400</end>"));
        assert!(xml.contains("<count>2</count>")); // aggregated
        assert!(xml.contains("<count>1</count>"));
        assert!(xml.contains("<source_ip>1.2.3.4</source_ip>"));
        assert!(xml.contains("<source_ip>5.6.7.8</source_ip>"));
        assert!(xml.contains("<disposition>none</disposition>"));
        assert!(xml.contains("<disposition>reject</disposition>"));
    }

    #[test]
    fn generate_report_xml_escapes_special_chars() {
        let results = vec![DmarcResultRecord {
            source_ip: "1.2.3.4".into(),
            from_domain: "test&co.com".into(),
            spf_result: "pass".into(),
            dkim_result: "pass".into(),
            dmarc_result: "pass".into(),
            disposition: "none".into(),
        }];

        let xml = generate_dmarc_report_xml(
            "O&G <Corp>",
            "dmarc@test.com",
            "rpt-002",
            "test.com",
            0,
            86400,
            &results,
        );

        assert!(xml.contains("O&amp;G &lt;Corp&gt;"));
        assert!(xml.contains("test&amp;co.com"));
    }

    #[test]
    fn extract_rua_mailto() {
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; p=none; rua=mailto:dmarc@example.com"),
            Some("dmarc@example.com".into())
        );
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; p=reject; rua=mailto:a@b.com, mailto:c@d.com"),
            Some("a@b.com".into())
        );
        assert_eq!(extract_rua_from_dmarc_record("v=DMARC1; p=none"), None);
    }

    #[test]
    fn generate_report_xml_empty_results() {
        let xml = generate_dmarc_report_xml("Org", "a@b.com", "rpt-0", "b.com", 0, 86400, &[]);
        assert!(xml.contains("<feedback>"));
        assert!(xml.contains("</feedback>"));
        assert!(!xml.contains("<record>"));
    }

    #[test]
    fn escape_xml_all_special_chars() {
        assert_eq!(escape_xml("a&b<c>d\"e"), "a&amp;b&lt;c&gt;d&quot;e");
    }

    #[test]
    fn escape_xml_passthrough() {
        assert_eq!(escape_xml("hello world"), "hello world");
    }

    #[test]
    fn gzip_compress_roundtrip() {
        let data = b"hello world test data";
        let compressed = gzip_compress(data);
        assert!(!compressed.is_empty());
        assert!(compressed.len() < data.len() + 100); // gzip has overhead for small data
    }

    #[test]
    fn extract_rua_no_mailto_prefix() {
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; rua=https://example.com/dmarc"),
            None
        );
    }

    #[test]
    fn extract_rua_empty_string() {
        assert_eq!(extract_rua_from_dmarc_record(""), None);
    }

    #[test]
    fn format_report_email_structure() {
        let xml = "<feedback><record/></feedback>";
        let email = format_report_email(
            "dmarc@host.com",
            "rua@example.com",
            "example.com",
            "rpt-001",
            "2026-03-01",
            xml,
        );
        let email_str = String::from_utf8_lossy(&email);
        assert!(email_str.contains("From: dmarc@host.com"));
        assert!(email_str.contains("To: rua@example.com"));
        assert!(email_str.contains("Report domain: example.com"));
        assert!(email_str.contains("Content-Type: multipart/mixed"));
        assert!(email_str.contains("Content-Type: application/gzip"));
        assert!(email_str.contains("Content-Transfer-Encoding: base64"));
    }

    // --- additional extract_rua_from_dmarc_record tests ---

    #[test]
    fn extract_rua_with_whitespace_around_mailto() {
        // the value " mailto:report@example.com" is split by comma, then trimmed,
        // so strip_prefix("mailto:") matches after trim
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; p=none; rua= mailto:report@example.com"),
            Some("report@example.com".into()),
        );
    }

    #[test]
    fn extract_rua_multiple_uris_first_non_mailto() {
        // first uri is https, second is mailto — should return the mailto one
        assert_eq!(
            extract_rua_from_dmarc_record(
                "v=DMARC1; p=none; rua=https://report.example.com, mailto:dmarc@example.com"
            ),
            Some("dmarc@example.com".into())
        );
    }

    #[test]
    fn extract_rua_just_rua_field() {
        assert_eq!(
            extract_rua_from_dmarc_record("rua=mailto:x@y.com"),
            Some("x@y.com".into())
        );
    }

    #[test]
    fn extract_rua_ruf_not_rua() {
        // ruf is for forensic reports, not aggregate — should return None
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; p=none; ruf=mailto:forensic@example.com"),
            None
        );
    }

    #[test]
    fn extract_rua_complex_real_world_record() {
        let record = "v=DMARC1; p=quarantine; sp=reject; adkim=s; aspf=s; pct=100; rua=mailto:dmarc-agg@example.com; ruf=mailto:dmarc-forensic@example.com; fo=1";
        assert_eq!(
            extract_rua_from_dmarc_record(record),
            Some("dmarc-agg@example.com".into())
        );
    }

    #[test]
    fn extract_rua_with_size_limit() {
        // some DMARC records include size limits like mailto:rua@example.com!10m
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; p=none; rua=mailto:rua@example.com!10m"),
            Some("rua@example.com!10m".into())
        );
    }

    #[test]
    fn extract_rua_semicolon_only() {
        assert_eq!(extract_rua_from_dmarc_record(";;;"), None);
    }

    // --- additional escape_xml tests ---

    #[test]
    fn escape_xml_empty_string() {
        assert_eq!(escape_xml(""), "");
    }

    #[test]
    fn escape_xml_only_special_chars() {
        assert_eq!(escape_xml("&<>\""), "&amp;&lt;&gt;&quot;");
    }

    #[test]
    fn escape_xml_single_quote_not_escaped() {
        // xml escape in this impl does not handle single quotes
        assert_eq!(escape_xml("it's"), "it's");
    }

    #[test]
    fn escape_xml_repeated_ampersands() {
        assert_eq!(escape_xml("&&&&"), "&amp;&amp;&amp;&amp;");
    }

    #[test]
    fn escape_xml_unicode_passthrough() {
        assert_eq!(escape_xml("日本語テスト"), "日本語テスト");
    }

    #[test]
    fn escape_xml_mixed_content() {
        assert_eq!(
            escape_xml("Hello <world> & \"universe\""),
            "Hello &lt;world&gt; &amp; &quot;universe&quot;"
        );
    }

    // --- additional gzip_compress tests ---

    #[test]
    fn gzip_compress_empty_data() {
        let compressed = gzip_compress(b"");
        assert!(!compressed.is_empty()); // gzip header still present
    }

    #[test]
    fn gzip_compress_decompresses_correctly() {
        use std::io::Read;
        let original = b"The quick brown fox jumps over the lazy dog";
        let compressed = gzip_compress(original);
        let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn gzip_compress_large_repetitive_data() {
        let data: Vec<u8> = "ABCDEFGHIJ".repeat(10000).into_bytes();
        let compressed = gzip_compress(&data);
        // repetitive data should compress well
        assert!(compressed.len() < data.len() / 10);
    }

    // --- additional generate_dmarc_report_xml tests ---

    #[test]
    fn generate_report_xml_single_result() {
        let results = vec![DmarcResultRecord {
            source_ip: "10.0.0.1".into(),
            from_domain: "single.com".into(),
            spf_result: "pass".into(),
            dkim_result: "fail".into(),
            dmarc_result: "fail".into(),
            disposition: "quarantine".into(),
        }];
        let xml = generate_dmarc_report_xml(
            "Single Org",
            "s@s.com",
            "rpt-single",
            "single.com",
            0,
            86400,
            &results,
        );
        assert!(xml.contains("<count>1</count>"));
        assert!(xml.contains("<source_ip>10.0.0.1</source_ip>"));
        assert!(xml.contains("<disposition>quarantine</disposition>"));
        assert!(xml.contains("<dkim>fail</dkim>"));
        assert!(xml.contains("<spf>pass</spf>"));
        assert!(xml.contains("<header_from>single.com</header_from>"));
    }

    #[test]
    fn generate_report_xml_starts_with_xml_declaration() {
        let xml = generate_dmarc_report_xml("O", "e@e.com", "r", "d.com", 0, 1, &[]);
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>"));
    }

    #[test]
    fn generate_report_xml_policy_published_defaults() {
        let xml = generate_dmarc_report_xml("O", "e@e.com", "r", "d.com", 0, 1, &[]);
        assert!(xml.contains("<adkim>r</adkim>"));
        assert!(xml.contains("<aspf>r</aspf>"));
        assert!(xml.contains("<p>none</p>"));
        assert!(xml.contains("<sp>none</sp>"));
        assert!(xml.contains("<pct>100</pct>"));
    }

    #[test]
    fn generate_report_xml_aggregates_identical_keys() {
        // 5 identical records should aggregate to count=5
        let record = DmarcResultRecord {
            source_ip: "9.9.9.9".into(),
            from_domain: "agg.com".into(),
            spf_result: "pass".into(),
            dkim_result: "pass".into(),
            dmarc_result: "pass".into(),
            disposition: "none".into(),
        };
        let results: Vec<_> = (0..5).map(|_| record.clone()).collect();
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "agg.com", 0, 86400, &results);
        assert!(xml.contains("<count>5</count>"));
        // only one <record> block since all aggregate to same key
        assert_eq!(xml.matches("<record>").count(), 1);
    }

    #[test]
    fn generate_report_xml_different_ips_separate_records() {
        let results = vec![
            DmarcResultRecord {
                source_ip: "1.1.1.1".into(),
                from_domain: "test.com".into(),
                spf_result: "pass".into(),
                dkim_result: "pass".into(),
                dmarc_result: "pass".into(),
                disposition: "none".into(),
            },
            DmarcResultRecord {
                source_ip: "2.2.2.2".into(),
                from_domain: "test.com".into(),
                spf_result: "pass".into(),
                dkim_result: "pass".into(),
                dmarc_result: "pass".into(),
                disposition: "none".into(),
            },
        ];
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "test.com", 0, 86400, &results);
        assert_eq!(xml.matches("<record>").count(), 2);
        // records should be sorted by source_ip
        let pos1 = xml.find("<source_ip>1.1.1.1</source_ip>").unwrap();
        let pos2 = xml.find("<source_ip>2.2.2.2</source_ip>").unwrap();
        assert!(pos1 < pos2, "records should be sorted by source_ip");
    }

    #[test]
    fn generate_report_xml_different_dispositions_separate_records() {
        let results = vec![
            DmarcResultRecord {
                source_ip: "1.1.1.1".into(),
                from_domain: "test.com".into(),
                spf_result: "pass".into(),
                dkim_result: "pass".into(),
                dmarc_result: "pass".into(),
                disposition: "none".into(),
            },
            DmarcResultRecord {
                source_ip: "1.1.1.1".into(),
                from_domain: "test.com".into(),
                spf_result: "fail".into(),
                dkim_result: "fail".into(),
                dmarc_result: "fail".into(),
                disposition: "reject".into(),
            },
        ];
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "test.com", 0, 86400, &results);
        // same ip but different disposition/results = different agg keys
        assert_eq!(xml.matches("<record>").count(), 2);
    }

    #[test]
    fn generate_report_xml_domain_in_policy_published() {
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "mydomain.org", 0, 86400, &[]);
        assert!(xml.contains("<domain>mydomain.org</domain>"));
    }

    #[test]
    fn generate_report_xml_auth_results_section() {
        let results = vec![DmarcResultRecord {
            source_ip: "3.3.3.3".into(),
            from_domain: "auth.com".into(),
            spf_result: "softfail".into(),
            dkim_result: "temperror".into(),
            dmarc_result: "fail".into(),
            disposition: "none".into(),
        }];
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "auth.com", 0, 86400, &results);
        assert!(xml.contains("<auth_results>"));
        assert!(xml.contains("<result>softfail</result>"));
        assert!(xml.contains("<result>temperror</result>"));
    }

    #[test]
    fn generate_report_xml_multiple_domains() {
        let results = vec![
            DmarcResultRecord {
                source_ip: "1.1.1.1".into(),
                from_domain: "a.com".into(),
                spf_result: "pass".into(),
                dkim_result: "pass".into(),
                dmarc_result: "pass".into(),
                disposition: "none".into(),
            },
            DmarcResultRecord {
                source_ip: "1.1.1.1".into(),
                from_domain: "b.com".into(),
                spf_result: "fail".into(),
                dkim_result: "fail".into(),
                dmarc_result: "fail".into(),
                disposition: "reject".into(),
            },
        ];
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "test.com", 0, 86400, &results);
        assert!(xml.contains("<header_from>a.com</header_from>"));
        assert!(xml.contains("<header_from>b.com</header_from>"));
    }

    // --- additional format_report_email tests ---

    #[test]
    fn format_report_email_contains_boundary() {
        let xml = "<feedback/>";
        let email = format_report_email("f@f.com", "t@t.com", "d.com", "rpt-1", "2026-01-01", xml);
        let email_str = String::from_utf8_lossy(&email);
        // mail-builder generates random boundaries — verify the
        // envelope shape: a boundary parameter on the outer
        // Content-Type, opening + closing lines using that exact
        // value.
        let unfold = email_str.replace("\r\n ", " ").replace("\r\n\t", " ");
        let needle = "boundary=\"";
        let start = unfold
            .find(needle)
            .expect("no boundary= on outer Content-Type")
            + needle.len();
        let end = unfold[start..].find('"').unwrap() + start;
        let boundary = &unfold[start..end];
        assert!(!boundary.is_empty());
        let open = format!("--{boundary}");
        let close = format!("--{boundary}--");
        assert!(
            email_str.contains(&open),
            "no opening boundary line {open:?}"
        );
        assert!(
            email_str.contains(&close),
            "no closing boundary line {close:?}"
        );
    }

    #[test]
    fn format_report_email_filename_format() {
        let xml = "<feedback/>";
        let email = format_report_email(
            "f@f.com",
            "rua@target.com",
            "example.com",
            "rpt-42",
            "2026-03-01",
            xml,
        );
        let email_str = String::from_utf8_lossy(&email);
        assert!(
            email_str.contains("filename=\"example.com!rua@target.com!2026-03-01!rpt-42.xml.gz\"")
        );
    }

    #[test]
    fn format_report_email_subject_contains_domain_and_report_id() {
        let xml = "<feedback/>";
        let email = format_report_email(
            "dmarc@mx.com",
            "rua@dest.com",
            "sender.org",
            "RPT-99",
            "2026-02-28",
            xml,
        );
        let email_str = String::from_utf8_lossy(&email);
        assert!(email_str.contains("Report domain: sender.org"));
        assert!(email_str.contains("Report-ID: <RPT-99>"));
    }

    #[test]
    fn format_report_email_mime_version() {
        let email = format_report_email("f@f.com", "t@t.com", "d.com", "r", "2026-01-01", "<x/>");
        let email_str = String::from_utf8_lossy(&email);
        assert!(email_str.contains("MIME-Version: 1.0"));
    }

    #[test]
    fn format_report_email_has_date_header() {
        let email = format_report_email("f@f.com", "t@t.com", "d.com", "r", "2026-01-01", "<x/>");
        let email_str = String::from_utf8_lossy(&email);
        assert!(email_str.contains("Date: "));
    }

    #[test]
    fn format_report_email_text_body_mentions_domain_and_date() {
        let email =
            format_report_email("f@f.com", "t@t.com", "mydom.com", "r", "2026-03-05", "<x/>");
        let email_str = String::from_utf8_lossy(&email);
        assert!(email_str.contains("DMARC aggregate report for mydom.com (2026-03-05)"));
    }

    #[test]
    fn format_report_email_base64_attachment_is_valid() {
        use base64::Engine;
        let xml = "<feedback><record>test</record></feedback>";
        let email = format_report_email("f@f.com", "t@t.com", "d.com", "r", "2026-01-01", xml);
        let email_str = String::from_utf8_lossy(&email);

        // extract base64 content between the attachment header
        // block and the next boundary line. mail-builder emits
        // Content-Type, CTE, and Content-Disposition in that order,
        // so we anchor on the CTE header then walk to the
        // header-body separator and finally the boundary.
        let cte_marker = "Content-Transfer-Encoding: base64";
        let cte_at = email_str.find(cte_marker).unwrap();
        let blank = email_str[cte_at..].find("\r\n\r\n").unwrap() + cte_at + 4;
        let start = blank;
        let end = email_str[start..].find("\r\n--").unwrap() + start;
        let b64_content: String = email_str[start..end].lines().collect::<Vec<_>>().join("");
        // should be valid base64
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64_content.trim())
            .expect("base64 should be valid");
        assert!(!decoded.is_empty());

        // should decompress back to original xml
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&decoded[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert_eq!(decompressed, xml);
    }

    #[test]
    fn format_report_email_base64_line_length() {
        // base64 lines should be wrapped at 76 chars
        let xml = "<feedback>".repeat(100); // large enough to produce multi-line base64
        let email = format_report_email("f@f.com", "t@t.com", "d.com", "r", "2026-01-01", &xml);
        let email_str = String::from_utf8_lossy(&email);
        let cte_marker = "Content-Transfer-Encoding: base64";
        let cte_at = email_str.find(cte_marker).unwrap();
        let blank = email_str[cte_at..].find("\r\n\r\n").unwrap() + cte_at + 4;
        let start = blank;
        let end = email_str[start..].find("\r\n--").unwrap() + start;
        for line in email_str[start..end].split("\r\n") {
            if !line.is_empty() && !line.starts_with("--") {
                assert!(
                    line.len() <= 76,
                    "base64 line too long: {} chars",
                    line.len()
                );
            }
        }
    }

    // --- DmarcResultRecord tests ---

    #[test]
    fn dmarc_result_record_clone() {
        let record = DmarcResultRecord {
            source_ip: "1.2.3.4".into(),
            from_domain: "test.com".into(),
            spf_result: "pass".into(),
            dkim_result: "pass".into(),
            dmarc_result: "pass".into(),
            disposition: "none".into(),
        };
        let cloned = record.clone();
        assert_eq!(cloned.source_ip, "1.2.3.4");
        assert_eq!(cloned.from_domain, "test.com");
        assert_eq!(cloned.disposition, "none");
    }

    #[test]
    fn dmarc_result_record_debug() {
        let record = DmarcResultRecord {
            source_ip: "1.2.3.4".into(),
            from_domain: "test.com".into(),
            spf_result: "pass".into(),
            dkim_result: "fail".into(),
            dmarc_result: "fail".into(),
            disposition: "reject".into(),
        };
        let debug = format!("{:?}", record);
        assert!(debug.contains("DmarcResultRecord"));
        assert!(debug.contains("1.2.3.4"));
        assert!(debug.contains("reject"));
    }

    // --- AggKey tests ---

    #[test]
    fn agg_key_equality() {
        let key1 = AggKey {
            source_ip: "1.1.1.1".into(),
            from_domain: "a.com".into(),
            disposition: "none".into(),
            dkim_result: "pass".into(),
            spf_result: "pass".into(),
        };
        let key2 = key1.clone();
        assert_eq!(key1, key2);
    }

    #[test]
    fn agg_key_inequality_on_spf() {
        let key1 = AggKey {
            source_ip: "1.1.1.1".into(),
            from_domain: "a.com".into(),
            disposition: "none".into(),
            dkim_result: "pass".into(),
            spf_result: "pass".into(),
        };
        let key2 = AggKey {
            spf_result: "fail".into(),
            ..key1.clone()
        };
        assert_ne!(key1, key2);
    }

    #[test]
    fn agg_key_hash_consistency() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let key = AggKey {
            source_ip: "1.1.1.1".into(),
            from_domain: "a.com".into(),
            disposition: "none".into(),
            dkim_result: "pass".into(),
            spf_result: "pass".into(),
        };
        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();
        key.hash(&mut h1);
        key.clone().hash(&mut h2);
        assert_eq!(h1.finish(), h2.finish());
    }

    // --- edge cases for xml generation ---

    #[test]
    fn generate_report_xml_ipv6_source() {
        let results = vec![DmarcResultRecord {
            source_ip: "2001:db8::1".into(),
            from_domain: "ipv6.com".into(),
            spf_result: "pass".into(),
            dkim_result: "pass".into(),
            dmarc_result: "pass".into(),
            disposition: "none".into(),
        }];
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "ipv6.com", 0, 86400, &results);
        assert!(xml.contains("<source_ip>2001:db8::1</source_ip>"));
    }

    #[test]
    fn generate_report_xml_large_timestamps() {
        let xml =
            generate_dmarc_report_xml("Org", "e@e.com", "r", "d.com", i64::MAX - 1, i64::MAX, &[]);
        assert!(xml.contains(&format!("<begin>{}</begin>", i64::MAX - 1)));
        assert!(xml.contains(&format!("<end>{}</end>", i64::MAX)));
    }

    #[test]
    fn generate_report_xml_special_chars_in_domain() {
        let xml = generate_dmarc_report_xml("Org", "e@e.com", "r", "test&<>.com", 0, 86400, &[]);
        assert!(xml.contains("<domain>test&amp;&lt;&gt;.com</domain>"));
    }
}
