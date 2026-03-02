use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use sqlx::PgPool;

/// DMARC result record for aggregate reporting
#[derive(Debug, Clone)]
pub struct DmarcResultRecord {
    pub source_ip: String,
    pub from_domain: String,
    pub spf_result: String,
    pub dkim_result: String,
    pub dmarc_result: String,
    pub disposition: String,
}

/// DMARC report store backed by Postgres
pub struct DmarcReportStore {
    pool: PgPool,
}

impl DmarcReportStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// record a DMARC verification result
    pub async fn record_result(&self, record: &DmarcResultRecord) -> Result<(), sqlx::Error> {
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

    /// get results for a specific date (for report generation)
    pub async fn get_results_for_date(&self, date: &str) -> Result<Vec<DmarcResultRecord>, sqlx::Error> {
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT source_ip, from_domain, spf_result, dkim_result, dmarc_result, disposition
             FROM dmarc_results WHERE report_date = $1::date",
        )
        .bind(date)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| DmarcResultRecord {
            source_ip: r.0,
            from_domain: r.1,
            spf_result: r.2,
            dkim_result: r.3,
            dmarc_result: r.4,
            disposition: r.5,
        }).collect())
    }

    /// cleanup old results (older than days)
    pub async fn cleanup_old(&self, days: i64) -> Result<u64, sqlx::Error> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
        let cutoff_date = cutoff.format("%Y-%m-%d").to_string();
        let result = sqlx::query("DELETE FROM dmarc_results WHERE report_date < $1::date")
            .bind(cutoff_date)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

/// aggregated row for report generation
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct AggKey {
    source_ip: String,
    from_domain: String,
    disposition: String,
    dkim_result: String,
    spf_result: String,
}

/// generate DMARC aggregate report XML (RFC 7489 §12.4)
pub fn generate_dmarc_report_xml(
    org_name: &str,
    email: &str,
    report_id: &str,
    domain: &str,
    begin_ts: i64,
    end_ts: i64,
    results: &[DmarcResultRecord],
) -> String {
    // aggregate results by (source_ip, from_domain, disposition, dkim, spf)
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

    let mut xml = String::new();
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n");
    xml.push_str("<feedback>\n");

    // report_metadata
    xml.push_str("  <report_metadata>\n");
    xml.push_str(&format!("    <org_name>{}</org_name>\n", escape_xml(org_name)));
    xml.push_str(&format!("    <email>{}</email>\n", escape_xml(email)));
    xml.push_str(&format!("    <report_id>{report_id}</report_id>\n"));
    xml.push_str("    <date_range>\n");
    xml.push_str(&format!("      <begin>{begin_ts}</begin>\n"));
    xml.push_str(&format!("      <end>{end_ts}</end>\n"));
    xml.push_str("    </date_range>\n");
    xml.push_str("  </report_metadata>\n");

    // policy_published
    xml.push_str("  <policy_published>\n");
    xml.push_str(&format!("    <domain>{}</domain>\n", escape_xml(domain)));
    xml.push_str("    <adkim>r</adkim>\n");
    xml.push_str("    <aspf>r</aspf>\n");
    xml.push_str("    <p>none</p>\n");
    xml.push_str("    <sp>none</sp>\n");
    xml.push_str("    <pct>100</pct>\n");
    xml.push_str("  </policy_published>\n");

    // records
    let mut keys: Vec<_> = agg.keys().collect();
    keys.sort_by(|a, b| (&a.source_ip, &a.from_domain).cmp(&(&b.source_ip, &b.from_domain)));

    for key in keys {
        let count = agg[key];
        xml.push_str("  <record>\n");
        xml.push_str("    <row>\n");
        xml.push_str(&format!("      <source_ip>{}</source_ip>\n", key.source_ip));
        xml.push_str(&format!("      <count>{count}</count>\n"));
        xml.push_str("      <policy_evaluated>\n");
        xml.push_str(&format!("        <disposition>{}</disposition>\n", key.disposition));
        xml.push_str(&format!("        <dkim>{}</dkim>\n", key.dkim_result));
        xml.push_str(&format!("        <spf>{}</spf>\n", key.spf_result));
        xml.push_str("      </policy_evaluated>\n");
        xml.push_str("    </row>\n");
        xml.push_str("    <identifiers>\n");
        xml.push_str(&format!(
            "      <header_from>{}</header_from>\n",
            escape_xml(&key.from_domain)
        ));
        xml.push_str("    </identifiers>\n");
        xml.push_str("    <auth_results>\n");
        xml.push_str("      <spf>\n");
        xml.push_str(&format!(
            "        <domain>{}</domain>\n",
            escape_xml(&key.from_domain)
        ));
        xml.push_str(&format!("        <result>{}</result>\n", key.spf_result));
        xml.push_str("      </spf>\n");
        xml.push_str("      <dkim>\n");
        xml.push_str(&format!(
            "        <domain>{}</domain>\n",
            escape_xml(&key.from_domain)
        ));
        xml.push_str(&format!("        <result>{}</result>\n", key.dkim_result));
        xml.push_str("      </dkim>\n");
        xml.push_str("    </auth_results>\n");
        xml.push_str("  </record>\n");
    }

    xml.push_str("</feedback>\n");
    xml
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// gzip compress data
fn gzip_compress(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

/// format a DMARC aggregate report email with gzipped XML attachment
pub fn format_report_email(
    from: &str,
    to: &str,
    org_domain: &str,
    report_id: &str,
    date: &str,
    xml: &str,
) -> Vec<u8> {
    use base64::Engine;
    let gz = gzip_compress(xml.as_bytes());
    let b64 = base64::engine::general_purpose::STANDARD.encode(&gz);
    let boundary = format!("dmarc-report-{report_id}");
    let filename = format!("{org_domain}!{to}!{date}!{report_id}.xml.gz");
    let now = chrono::Utc::now().to_rfc2822();

    let mut msg = format!(
        "From: {from}\r\n\
         To: {to}\r\n\
         Subject: Report domain: {org_domain} Submitter: {from} Report-ID: <{report_id}>\r\n\
         Date: {now}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         DMARC aggregate report for {org_domain} ({date})\r\n\
         \r\n\
         --{boundary}\r\n\
         Content-Type: application/gzip\r\n\
         Content-Disposition: attachment; filename=\"{filename}\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n"
    );

    // wrap base64 at 76 chars
    for chunk in b64.as_bytes().chunks(76) {
        msg.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        msg.push_str("\r\n");
    }
    msg.push_str(&format!("--{boundary}--\r\n"));

    msg.into_bytes()
}

/// extract rua mailto address from _dmarc TXT record
pub fn extract_rua_from_dmarc_record(txt: &str) -> Option<String> {
    for part in txt.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("rua=") {
            // rua can have multiple URIs separated by commas
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

/// spawn daily DMARC report generation task
pub fn spawn_daily_report_task(
    store: Arc<DmarcReportStore>,
    org_name: String,
    report_email: String,
    hostname: String,
    resolver: Arc<hickory_resolver::TokioResolver>,
    outbound_queue: Option<sqlx::PgPool>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(86400));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let yesterday = (chrono::Utc::now() - chrono::Duration::days(1))
                        .format("%Y-%m-%d")
                        .to_string();
                    match store.get_results_for_date(&yesterday).await {
                        Ok(results) if !results.is_empty() => {
                            tracing::info!(
                                event = "dmarc_report",
                                count = results.len(),
                                date = %yesterday,
                                "generating DMARC aggregate report"
                            );

                            // group results by from_domain
                            let mut by_domain: HashMap<String, Vec<DmarcResultRecord>> = HashMap::new();
                            for r in results {
                                by_domain.entry(r.from_domain.clone()).or_default().push(r);
                            }

                            for (domain, domain_results) in &by_domain {
                                // look up _dmarc.{domain} for rua address
                                let dmarc_name = format!("_dmarc.{domain}");
                                let rua_addr = match resolver.txt_lookup(&dmarc_name).await {
                                    Ok(lookup) => {
                                        lookup.iter()
                                            .find_map(|txt| {
                                                let s = txt.to_string();
                                                extract_rua_from_dmarc_record(&s)
                                            })
                                    }
                                    Err(_) => None,
                                };

                                let Some(rua_addr) = rua_addr else {
                                    tracing::debug!(
                                        event = "dmarc_report_skip",
                                        domain = domain.as_str(),
                                        "no rua address found"
                                    );
                                    continue;
                                };

                                // generate report
                                let begin = chrono::NaiveDate::parse_from_str(&yesterday, "%Y-%m-%d")
                                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp())
                                    .unwrap_or(0);
                                let end = begin + 86400;
                                let report_id = format!("{}!{}!{}", hostname, domain, yesterday);

                                let xml = generate_dmarc_report_xml(
                                    &org_name, &report_email, &report_id,
                                    domain, begin, end, domain_results,
                                );
                                let email = format_report_email(
                                    &report_email, &rua_addr, domain,
                                    &report_id, &yesterday, &xml,
                                );

                                // enqueue report email
                                if let Some(ref pool) = outbound_queue {
                                    let rua_domain = rua_addr
                                        .rsplit_once('@')
                                        .map(|(_, d)| d)
                                        .unwrap_or("unknown");
                                    let now = chrono::Utc::now().timestamp();
                                    let _ = mailrs_outbound_queue::queue::enqueue(
                                        pool, &report_email, &rua_addr, rua_domain,
                                        &email, None, now,
                                    ).await;
                                    tracing::info!(
                                        event = "dmarc_report_queued",
                                        domain = domain.as_str(),
                                        rua = rua_addr.as_str(),
                                        "DMARC report enqueued"
                                    );
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!("DMARC report error: {e}"),
                    }
                    // cleanup results older than 90 days
                    let _ = store.cleanup_old(90).await;
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        return;
                    }
                }
            }
        }
    });
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
            "Test Org", "dmarc@test.com", "rpt-001",
            "test.com", 1000000, 1086400, &results,
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
            "O&G <Corp>", "dmarc@test.com", "rpt-002",
            "test.com", 0, 86400, &results,
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
        assert_eq!(
            extract_rua_from_dmarc_record("v=DMARC1; p=none"),
            None
        );
    }

    #[test]
    fn format_report_email_structure() {
        let xml = "<feedback><record/></feedback>";
        let email = format_report_email(
            "dmarc@host.com", "rua@example.com", "example.com",
            "rpt-001", "2026-03-01", xml,
        );
        let email_str = String::from_utf8_lossy(&email);
        assert!(email_str.contains("From: dmarc@host.com"));
        assert!(email_str.contains("To: rua@example.com"));
        assert!(email_str.contains("Report domain: example.com"));
        assert!(email_str.contains("Content-Type: multipart/mixed"));
        assert!(email_str.contains("Content-Type: application/gzip"));
        assert!(email_str.contains("Content-Transfer-Encoding: base64"));
    }
}
