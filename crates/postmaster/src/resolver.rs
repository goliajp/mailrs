//! Resolver trait — decouple per-check modules from `hickory_resolver`.
//!
//! Per-check modules (`bimi`, `dane`, `dkim`, `dmarc`, `mta_sts`, `mx`,
//! `ptr`, `spf`, `tlsrpt`) used to call `hickory_resolver::TokioResolver`
//! directly. That made unit-testing the check logic impossible without
//! live DNS, so lib coverage on every per-check submodule sat at 0%.
//!
//! This trait normalises the 5 DNS verbs the checks actually need into
//! plain `Vec<Strings>` / `Vec<MxRecord>` / `Vec<IpAddr>` returns. The
//! caller passes any `impl PostmasterResolver` — in prod, the supplied
//! [`HickoryPostmasterResolver`] wrapper; in tests, a [`MockResolver`]
//! that returns canned answers per qname.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use async_trait::async_trait;

/// One MX record: preference (lower = more preferred) + target FQDN
/// (trailing-dot stripped by the resolver impl).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MxRecord {
    /// `PREF` from `prefname IN MX prefnum prefname` (RFC 5321 §5.1).
    pub preference: u16,
    /// Target hostname, FQDN with trailing dot stripped.
    pub exchange: String,
}

/// Errors returned by [`PostmasterResolver`] methods. Per-check modules
/// translate these into `CheckResult { status: Skip | Fail, .. }`.
#[derive(Debug, thiserror::Error, Clone)]
pub enum ResolverError {
    /// DNS query found no records (NXDOMAIN, NODATA, empty answer set).
    #[error("not found: {0}")]
    NotFound(String),
    /// Other DNS failure (SERVFAIL, timeout, malformed answer).
    #[error("dns: {0}")]
    Dns(String),
}

/// 5-verb DNS facade for postmaster's domain-health checks.
#[async_trait]
pub trait PostmasterResolver: Send + Sync {
    /// Resolve TXT records at `qname` → list of joined-chunks strings
    /// (one per TXT RR). Multi-segment TXTs are joined by the impl.
    async fn txt_lookup(&self, qname: &str) -> Result<Vec<String>, ResolverError>;

    /// Resolve MX records at `qname`. Returned vec is **unsorted** —
    /// callers sort by `preference` themselves.
    async fn mx_lookup(&self, qname: &str) -> Result<Vec<MxRecord>, ResolverError>;

    /// Resolve A + AAAA records at `qname` to IPs.
    async fn ip_lookup(&self, qname: &str) -> Result<Vec<IpAddr>, ResolverError>;

    /// Reverse-resolve `ip` → PTR target names (trailing dot kept by
    /// the impl; callers normalise as they need).
    async fn reverse_lookup(&self, ip: IpAddr) -> Result<Vec<String>, ResolverError>;

    /// Resolve TLSA records at `qname` → list of TLSA RDATA in their
    /// text representation (e.g. `"3 1 1 abc...def"`). Used by the
    /// DANE check.
    async fn tlsa_lookup(&self, qname: &str) -> Result<Vec<String>, ResolverError>;
}

/// [`PostmasterResolver`] impl backed by `hickory_resolver::TokioResolver`.
#[cfg(feature = "hickory")]
pub use hickory::HickoryPostmasterResolver;

#[cfg(feature = "hickory")]
mod hickory {
    use super::*;
    use hickory_resolver::TokioResolver;
    use hickory_resolver::proto::rr::{RData, RecordType};

    /// Thin wrapper around [`hickory_resolver::TokioResolver`] satisfying
    /// the [`PostmasterResolver`] trait. Construct once at server
    /// startup and share via `Arc<dyn PostmasterResolver>` to all
    /// `check_domain` call sites.
    pub struct HickoryPostmasterResolver(pub TokioResolver);

    impl HickoryPostmasterResolver {
        /// Wrap the supplied `TokioResolver`.
        pub fn new(resolver: TokioResolver) -> Self {
            Self(resolver)
        }
    }

    fn map_err(e: impl std::fmt::Display, qname: &str) -> ResolverError {
        let msg = e.to_string();
        if msg.contains("no record found") || msg.contains("NoRecords") {
            ResolverError::NotFound(qname.to_string())
        } else {
            ResolverError::Dns(msg)
        }
    }

    #[async_trait]
    impl PostmasterResolver for HickoryPostmasterResolver {
        async fn txt_lookup(&self, qname: &str) -> Result<Vec<String>, ResolverError> {
            match self.0.txt_lookup(qname).await {
                Ok(records) => Ok(records
                    .answers()
                    .iter()
                    .filter_map(|r| match &r.data {
                        RData::TXT(txt) => Some(txt.to_string()),
                        _ => None,
                    })
                    .collect()),
                Err(e) => Err(map_err(e, qname)),
            }
        }

        async fn mx_lookup(&self, qname: &str) -> Result<Vec<MxRecord>, ResolverError> {
            match self.0.mx_lookup(qname).await {
                Ok(records) => Ok(records
                    .answers()
                    .iter()
                    .filter_map(|r| match &r.data {
                        RData::MX(mx) => Some(MxRecord {
                            preference: mx.preference,
                            exchange: mx.exchange.to_string().trim_end_matches('.').to_string(),
                        }),
                        _ => None,
                    })
                    .collect()),
                Err(e) => Err(map_err(e, qname)),
            }
        }

        async fn ip_lookup(&self, qname: &str) -> Result<Vec<IpAddr>, ResolverError> {
            match self.0.lookup_ip(qname).await {
                Ok(ips) => Ok(ips.iter().collect()),
                Err(e) => Err(map_err(e, qname)),
            }
        }

        async fn reverse_lookup(&self, ip: IpAddr) -> Result<Vec<String>, ResolverError> {
            match self.0.reverse_lookup(ip).await {
                Ok(names) => Ok(names
                    .answers()
                    .iter()
                    .filter_map(|r| match &r.data {
                        RData::PTR(name) => Some(name.to_string()),
                        _ => None,
                    })
                    .collect()),
                Err(e) => Err(map_err(e, &ip.to_string())),
            }
        }

        async fn tlsa_lookup(&self, qname: &str) -> Result<Vec<String>, ResolverError> {
            match self.0.lookup(qname, RecordType::TLSA).await {
                Ok(records) => Ok(records.answers().iter().map(|r| format!("{}", r.data)).collect()),
                Err(e) => Err(map_err(e, qname)),
            }
        }
    }
}

/// In-memory [`PostmasterResolver`] for unit tests — returns canned
/// answers per (verb, qname). Use [`MockResolver::with_*`] to seed
/// responses; default response is `NotFound`.
#[derive(Default)]
pub struct MockResolver {
    state: Mutex<MockState>,
}

#[derive(Default)]
struct MockState {
    txt: HashMap<String, Result<Vec<String>, ResolverError>>,
    mx: HashMap<String, Result<Vec<MxRecord>, ResolverError>>,
    ip: HashMap<String, Result<Vec<IpAddr>, ResolverError>>,
    reverse: HashMap<IpAddr, Result<Vec<String>, ResolverError>>,
    tlsa: HashMap<String, Result<Vec<String>, ResolverError>>,
}

impl MockResolver {
    /// New empty mock — every lookup returns `NotFound` until seeded.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed a TXT response.
    pub fn with_txt(self, qname: impl Into<String>, records: Vec<String>) -> Self {
        self.state.lock().unwrap().txt.insert(qname.into(), Ok(records));
        self
    }

    /// Seed a TXT error.
    pub fn with_txt_err(self, qname: impl Into<String>, err: ResolverError) -> Self {
        self.state.lock().unwrap().txt.insert(qname.into(), Err(err));
        self
    }

    /// Seed an MX response.
    pub fn with_mx(self, qname: impl Into<String>, records: Vec<MxRecord>) -> Self {
        self.state.lock().unwrap().mx.insert(qname.into(), Ok(records));
        self
    }

    /// Seed an MX error.
    pub fn with_mx_err(self, qname: impl Into<String>, err: ResolverError) -> Self {
        self.state.lock().unwrap().mx.insert(qname.into(), Err(err));
        self
    }

    /// Seed an IP-resolution response.
    pub fn with_ip(self, qname: impl Into<String>, ips: Vec<IpAddr>) -> Self {
        self.state.lock().unwrap().ip.insert(qname.into(), Ok(ips));
        self
    }

    /// Seed an IP-resolution error.
    pub fn with_ip_err(self, qname: impl Into<String>, err: ResolverError) -> Self {
        self.state.lock().unwrap().ip.insert(qname.into(), Err(err));
        self
    }

    /// Seed a reverse-DNS response.
    pub fn with_reverse(self, ip: IpAddr, names: Vec<String>) -> Self {
        self.state.lock().unwrap().reverse.insert(ip, Ok(names));
        self
    }

    /// Seed a TLSA response.
    pub fn with_tlsa(self, qname: impl Into<String>, records: Vec<String>) -> Self {
        self.state.lock().unwrap().tlsa.insert(qname.into(), Ok(records));
        self
    }
}

#[async_trait]
impl PostmasterResolver for MockResolver {
    async fn txt_lookup(&self, qname: &str) -> Result<Vec<String>, ResolverError> {
        self.state
            .lock()
            .unwrap()
            .txt
            .get(qname)
            .cloned()
            .unwrap_or_else(|| Err(ResolverError::NotFound(qname.to_string())))
    }

    async fn mx_lookup(&self, qname: &str) -> Result<Vec<MxRecord>, ResolverError> {
        self.state
            .lock()
            .unwrap()
            .mx
            .get(qname)
            .cloned()
            .unwrap_or_else(|| Err(ResolverError::NotFound(qname.to_string())))
    }

    async fn ip_lookup(&self, qname: &str) -> Result<Vec<IpAddr>, ResolverError> {
        self.state
            .lock()
            .unwrap()
            .ip
            .get(qname)
            .cloned()
            .unwrap_or_else(|| Err(ResolverError::NotFound(qname.to_string())))
    }

    async fn reverse_lookup(&self, ip: IpAddr) -> Result<Vec<String>, ResolverError> {
        self.state
            .lock()
            .unwrap()
            .reverse
            .get(&ip)
            .cloned()
            .unwrap_or_else(|| Err(ResolverError::NotFound(ip.to_string())))
    }

    async fn tlsa_lookup(&self, qname: &str) -> Result<Vec<String>, ResolverError> {
        self.state
            .lock()
            .unwrap()
            .tlsa
            .get(qname)
            .cloned()
            .unwrap_or_else(|| Err(ResolverError::NotFound(qname.to_string())))
    }
}
