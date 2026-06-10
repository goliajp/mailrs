//! Daily DMARC report orchestrator — mailrs business glue around
//! `mailrs_dmarc`. The protocol/storage/format primitives live in the
//! published crate; this file just wires them to the resolver and
//! outbound queue.

use std::collections::HashMap;
use std::sync::Arc;

// Re-export the published crate's types so existing call sites
// `crate::dmarc_report::{DmarcReportStore, DmarcResultRecord}` resolve.
pub use mailrs_dmarc::{
    DmarcResultRecord, DmarcStore, PgDmarcStore as DmarcReportStore, extract_rua_from_dmarc_record,
    format_report_email, generate_dmarc_report_xml,
};

/// spawn daily DMARC report generation task
pub fn spawn_daily_report_task(
    store: Arc<DmarcReportStore>,
    org_name: String,
    report_email: String,
    hostname: String,
    resolver: Arc<hickory_resolver::TokioResolver>,
    outbound_queue: Option<crate::pg::BackendPool>,
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

                            let mut by_domain: HashMap<String, Vec<DmarcResultRecord>> = HashMap::new();
                            for r in results {
                                by_domain.entry(r.from_domain.clone()).or_default().push(r);
                            }

                            for (domain, domain_results) in &by_domain {
                                let dmarc_name = format!("_dmarc.{domain}");
                                let rua_addr = match resolver.txt_lookup(&dmarc_name).await {
                                    Ok(lookup) => lookup.answers().iter().find_map(|r| match &r.data {
                                        hickory_resolver::proto::rr::RData::TXT(txt) => {
                                            extract_rua_from_dmarc_record(&txt.to_string())
                                        }
                                        _ => None,
                                    }),
                                    Err(_) => None,
                                };

                                let Some(rua_addr) = rua_addr else {
                                    tracing::debug!(event = "dmarc_report_skip", domain = domain.as_str(), "no rua");
                                    continue;
                                };

                                let begin = chrono::NaiveDate::parse_from_str(&yesterday, "%Y-%m-%d")
                                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc().timestamp())
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
                                    );
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => tracing::error!("DMARC report error: {e:?}"),
                    }
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
