#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::fmt;
use std::net::IpAddr;

use async_trait::async_trait;

/// Errors from a DNS lookup.
///
/// Implementors map NXDOMAIN to `Ok(Vec::new())` (not error) — "no
/// record" is a normal answer in email-auth flows. Reserve
/// [`DnsError::Temp`] for transient failures (timeout, SERVFAIL) and
/// [`DnsError::Perm`] for actual protocol/decode failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsError {
    /// Transient: timeout, SERVFAIL, network glitch. Caller should retry.
    Temp(String),
    /// Permanent: malformed response, refused, configuration error.
    Perm(String),
}

impl fmt::Display for DnsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DnsError::Temp(s) => write!(f, "dns temp error: {s}"),
            DnsError::Perm(s) => write!(f, "dns perm error: {s}"),
        }
    }
}

impl std::error::Error for DnsError {}

/// The five DNS query types email infrastructure actually uses.
///
/// All async fns return `Ok(Vec::new())` for NXDOMAIN (consistent
/// across implementors). `Err(_)` is reserved for actual lookup
/// failures.
#[async_trait]
pub trait DnsResolver: Send + Sync {
    /// TXT records (DKIM public keys, SPF policies, DMARC, …).
    async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DnsError>;
    /// IPv4 A records.
    async fn lookup_a(&self, domain: &str) -> Result<Vec<IpAddr>, DnsError>;
    /// IPv6 AAAA records.
    async fn lookup_aaaa(&self, domain: &str) -> Result<Vec<IpAddr>, DnsError>;
    /// MX records, returned as `(preference, exchange-hostname)` pairs.
    async fn lookup_mx(&self, domain: &str) -> Result<Vec<(u16, String)>, DnsError>;
    /// PTR records (reverse DNS).
    async fn lookup_ptr(&self, ip: IpAddr) -> Result<Vec<String>, DnsError>;
}

/// Ready-made [`DnsResolver`] over `hickory_resolver::TokioResolver`.
/// Enabled by the default `hickory` feature.
#[cfg(feature = "hickory")]
pub mod hickory {
    use super::*;
    use hickory_resolver::proto::rr::RData;
    use hickory_resolver::TokioResolver;

    /// Wrap a `TokioResolver` for use as a [`DnsResolver`].
    pub struct HickoryResolver {
        inner: TokioResolver,
    }

    impl HickoryResolver {
        /// Construct from an existing `TokioResolver`.
        pub fn new(resolver: TokioResolver) -> Self {
            Self { inner: resolver }
        }
    }

    /// hickory signals "no records found" via its error message;
    /// the safest cross-version check is the error text. We treat
    /// no-records as `Ok(empty)` rather than an error.
    fn is_no_records<E: std::fmt::Display>(e: &E) -> bool {
        let s = e.to_string();
        s.contains("no record")
            || s.contains("NXDOMAIN")
            || s.contains("no records found")
            || s.contains("NoRecordsFound")
    }

    #[async_trait]
    impl DnsResolver for HickoryResolver {
        async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DnsError> {
            match self.inner.txt_lookup(domain).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::TXT(txt) = &record.data {
                            out.push(txt.to_string());
                        }
                    }
                    Ok(out)
                }
                Err(e) if is_no_records(&e) => Ok(Vec::new()),
                Err(e) => Err(DnsError::Temp(e.to_string())),
            }
        }

        async fn lookup_a(&self, domain: &str) -> Result<Vec<IpAddr>, DnsError> {
            match self.inner.ipv4_lookup(domain).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::A(a) = &record.data {
                            out.push(IpAddr::V4(a.0));
                        }
                    }
                    Ok(out)
                }
                Err(e) if is_no_records(&e) => Ok(Vec::new()),
                Err(e) => Err(DnsError::Temp(e.to_string())),
            }
        }

        async fn lookup_aaaa(&self, domain: &str) -> Result<Vec<IpAddr>, DnsError> {
            match self.inner.ipv6_lookup(domain).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::AAAA(a) = &record.data {
                            out.push(IpAddr::V6(a.0));
                        }
                    }
                    Ok(out)
                }
                Err(e) if is_no_records(&e) => Ok(Vec::new()),
                Err(e) => Err(DnsError::Temp(e.to_string())),
            }
        }

        async fn lookup_mx(&self, domain: &str) -> Result<Vec<(u16, String)>, DnsError> {
            match self.inner.mx_lookup(domain).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::MX(mx) = &record.data {
                            out.push((mx.preference, mx.exchange.to_utf8()));
                        }
                    }
                    Ok(out)
                }
                Err(e) if is_no_records(&e) => Ok(Vec::new()),
                Err(e) => Err(DnsError::Temp(e.to_string())),
            }
        }

        async fn lookup_ptr(&self, ip: IpAddr) -> Result<Vec<String>, DnsError> {
            match self.inner.reverse_lookup(ip).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::PTR(ptr) = &record.data {
                            out.push(ptr.to_utf8());
                        }
                    }
                    Ok(out)
                }
                Err(e) if is_no_records(&e) => Ok(Vec::new()),
                Err(e) => Err(DnsError::Temp(e.to_string())),
            }
        }
    }
}

#[cfg(feature = "hickory")]
pub use crate::hickory::HickoryResolver;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_error_display_includes_context() {
        let e = DnsError::Temp("connection refused".into());
        let s = format!("{e}");
        assert!(s.contains("connection refused"));
        assert!(s.contains("temp"));
    }

    #[test]
    fn dns_error_eq_works() {
        assert_eq!(
            DnsError::Perm("nxdomain".into()),
            DnsError::Perm("nxdomain".into())
        );
        assert_ne!(
            DnsError::Temp("x".into()),
            DnsError::Perm("x".into())
        );
    }
}
