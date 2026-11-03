//! DANE for our own port 25: the `3 1 1` records the served certificate
//! (and the key due to replace it) need, and what DNS publishes.
//!
//! The receiver only reports. DNS is published by whoever runs the zone;
//! a certificate that matches nothing published is still served, because
//! the alternative is serving one that is about to expire.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use hickory_resolver::proto::op::{Edns, Message, Query};
use hickory_resolver::proto::rr::{Name, RData, RecordType};
use mailrs_core_sidestate::smtp_monitor::{DaneSource, DaneStatus};
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, SubjectPublicKeyInfoDer};
use sha2::{Digest, Sha256};

/// How often the served certificate and the next key are re-read.
const FILES_EVERY: Duration = Duration::from_secs(60);
/// How often DNS is asked: three small queries an hour, against a TTL of
/// 3600 seconds, so a publication is seen within about two TTLs.
const DNS_EVERY: Duration = Duration::from_secs(3600);
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// `3 1 1 <hex>`: DANE-EE, the SubjectPublicKeyInfo, SHA-256.
pub fn record_for_spki(spki_der: &[u8]) -> String {
    let digest = Sha256::digest(spki_der);
    let mut out = String::from("3 1 1 ");
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The record for the first (leaf) certificate of a PEM chain.
pub fn record_for_cert_pem(pem: &[u8]) -> Option<String> {
    let leaf = CertificateDer::pem_slice_iter(pem).next()?.ok()?;
    let (_, cert) = x509_parser::parse_x509_certificate(&leaf).ok()?;
    Some(record_for_spki(cert.tbs_certificate.subject_pki.raw))
}

/// The record for a PEM `PUBLIC KEY`, which is already a SubjectPublicKeyInfo.
pub fn record_for_pubkey_pem(pem: &[u8]) -> Option<String> {
    let spki = SubjectPublicKeyInfoDer::from_pem_slice(pem).ok()?;
    Some(record_for_spki(&spki))
}

/// Where the check reads and whom it asks.
pub struct DaneConfig {
    pub hostname: String,
    pub cert: PathBuf,
    pub next_key: Option<PathBuf>,
    pub resolvers: Vec<String>,
    pub authoritative: Vec<String>,
}

/// The latest status, shared with the stats heartbeat.
#[derive(Default)]
pub struct DaneCell(Mutex<Option<DaneStatus>>);

impl DaneCell {
    pub fn get(&self) -> Option<DaneStatus> {
        self.0.lock().ok().and_then(|g| g.clone())
    }

    fn update(&self, f: impl FnOnce(&mut DaneStatus)) {
        if let Ok(mut g) = self.0.lock()
            && let Some(s) = g.as_mut()
        {
            f(s);
        }
    }

    fn set(&self, s: DaneStatus) {
        if let Ok(mut g) = self.0.lock() {
            *g = Some(s);
        }
    }
}

/// Re-read the files; when the served certificate's record changes, say
/// so, and count it when it matches none of the records last published.
pub fn refresh_files(cfg: &DaneConfig, cell: &DaneCell) {
    let current = std::fs::read(&cfg.cert)
        .ok()
        .and_then(|p| record_for_cert_pem(&p));
    let next = cfg
        .next_key
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|p| record_for_pubkey_pem(&p));
    let Some(before) = cell.get() else {
        cell.set(DaneStatus {
            name: format!("_25._tcp.{}", cfg.hostname),
            current,
            next,
            checked_at: None,
            sources: Vec::new(),
            unmatched_swaps: 0,
        });
        return;
    };
    let changed = before.current.is_some() && current.is_some() && before.current != current;
    let unmatched = changed && !before.published().is_empty() && !before.matches(current.as_ref());
    if unmatched {
        tracing::warn!(
            record = current.as_deref().unwrap_or(""),
            "dane: the certificate on disk changed to a key no published TLSA record names"
        );
    } else if changed {
        tracing::info!(
            record = current.as_deref().unwrap_or(""),
            "dane: the certificate on disk changed"
        );
    }
    cell.update(|s| {
        s.current = current;
        s.next = next;
        if unmatched {
            s.unmatched_swaps += 1;
        }
    });
}

/// Ask every configured server for the TLSA name.
pub async fn check_dns(cfg: &DaneConfig, cell: &DaneCell) {
    let name = format!("_25._tcp.{}.", cfg.hostname.trim_end_matches('.'));
    let mut sources = Vec::new();
    for r in &cfg.resolvers {
        let answer = match format!("{r}:53").parse::<SocketAddr>() {
            Ok(addr) => ask(addr, &name, true).await,
            Err(e) => Err(e.to_string()),
        };
        sources.push(source(r, true, answer));
    }
    for ns in &cfg.authoritative {
        let answer = match tokio::net::lookup_host((ns.as_str(), 53)).await {
            Ok(mut addrs) => match addrs.find(SocketAddr::is_ipv4) {
                Some(addr) => ask(addr, &name, false).await,
                None => Err("no IPv4 address".into()),
            },
            Err(e) => Err(e.to_string()),
        };
        sources.push(source(ns, false, answer));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok();
    cell.update(|s| {
        s.sources = sources;
        s.checked_at = now;
    });
}

fn source(
    server: &str,
    validating: bool,
    answer: Result<(bool, Vec<String>), String>,
) -> DaneSource {
    match answer {
        Ok((ad, records)) => DaneSource {
            server: server.to_string(),
            authenticated: validating.then_some(ad),
            records,
            error: None,
        },
        Err(e) => DaneSource {
            server: server.to_string(),
            authenticated: validating.then_some(false),
            records: Vec::new(),
            error: Some(e),
        },
    }
}

/// One UDP query with DO set; `recursive` asks a resolver, otherwise an
/// authoritative server. Returns the AD bit and the TLSA answers.
async fn ask(
    server: SocketAddr,
    name: &str,
    recursive: bool,
) -> Result<(bool, Vec<String>), String> {
    let qname = Name::from_ascii(name).map_err(|e| e.to_string())?;
    let mut msg = Message::query();
    msg.metadata.id = rand_id();
    msg.metadata.recursion_desired = recursive;
    msg.metadata.authentic_data = recursive;
    msg.add_query(Query::query(qname, RecordType::TLSA));
    let mut edns = Edns::new();
    edns.set_dnssec_ok(true).set_max_payload(1232);
    msg.set_edns(edns);
    let wire = msg.to_vec().map_err(|e| e.to_string())?;

    let sock = tokio::net::UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| e.to_string())?;
    sock.connect(server).await.map_err(|e| e.to_string())?;
    sock.send(&wire).await.map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(QUERY_TIMEOUT, sock.recv(&mut buf))
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    parse_answer(&buf[..n], msg.metadata.id)
}

fn parse_answer(wire: &[u8], id: u16) -> Result<(bool, Vec<String>), String> {
    use hickory_resolver::proto::op::ResponseCode;
    let reply = Message::from_vec(wire).map_err(|e| e.to_string())?;
    if reply.metadata.id != id {
        return Err("reply to another query".into());
    }
    if reply.metadata.truncation {
        return Err("truncated".into());
    }
    match reply.metadata.response_code {
        ResponseCode::NoError | ResponseCode::NXDomain => {}
        other => return Err(format!("{other}")),
    }
    let mut records: Vec<String> = reply
        .answers
        .iter()
        .filter_map(|r| match &r.data {
            RData::TLSA(t) => {
                let mut s = format!(
                    "{} {} {} ",
                    u8::from(t.cert_usage),
                    u8::from(t.selector),
                    u8::from(t.matching)
                );
                for b in &t.cert_data {
                    s.push_str(&format!("{b:02x}"));
                }
                Some(s)
            }
            _ => None,
        })
        .collect();
    records.sort();
    Ok((reply.metadata.authentic_data, records))
}

fn rand_id() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos ^ (nanos >> 16)) as u16
}

/// Files every minute, DNS at start and every hour.
pub fn spawn(cfg: DaneConfig, cell: std::sync::Arc<DaneCell>) {
    tokio::spawn(async move {
        refresh_files(&cfg, &cell);
        check_dns(&cfg, &cell).await;
        let mut files = tokio::time::interval(FILES_EVERY);
        let mut dns = tokio::time::interval(DNS_EVERY);
        files.tick().await;
        dns.tick().await;
        loop {
            tokio::select! {
                _ = files.tick() => refresh_files(&cfg, &cell),
                _ = dns.tick() => check_dns(&cfg, &cell).await,
            }
        }
    });
}

#[cfg(test)]
#[path = "dane_tests.rs"]
mod tests;
