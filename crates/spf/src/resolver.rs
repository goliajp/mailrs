//! DNS resolver trait that the SPF evaluator uses to fetch records.
//!
//! The crate is **resolver-agnostic** so callers can plug in whichever
//! DNS implementation they're already using (hickory-resolver,
//! trust-dns, custom). Enable the `hickory` feature (default on) to
//! get a ready-made impl over `hickory_resolver::TokioResolver`.

use std::net::IpAddr;

use async_trait::async_trait;

use crate::error::SpfError;

/// Minimal DNS interface the SPF evaluator needs.
///
/// Implementors should map `NXDOMAIN` to **empty Vec** (not error) —
/// "no record" is normal in SPF. Reserve `Err(SpfError::DnsTempError)`
/// for actual lookup failures (timeout, SERVFAIL) and
/// `Err(SpfError::DnsPermError)` for malformed responses.
#[async_trait]
pub trait SpfResolver: Send + Sync {
    /// TXT records for `domain`. SPF lives in TXT; one domain can have
    /// multiple TXT records (only one should be `v=spf1` per RFC 7208
    /// §4.5; we detect multi-v=spf1 in the evaluator).
    async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, SpfError>;
    /// IPv4 A records for `domain`.
    async fn lookup_a(&self, domain: &str) -> Result<Vec<IpAddr>, SpfError>;
    /// IPv6 AAAA records for `domain`.
    async fn lookup_aaaa(&self, domain: &str) -> Result<Vec<IpAddr>, SpfError>;
    /// MX records. Returns `(preference, exchange-hostname)` pairs.
    async fn lookup_mx(&self, domain: &str) -> Result<Vec<(u16, String)>, SpfError>;
}

/// Ready-made [`SpfResolver`] impl over `hickory_resolver::TokioResolver`.
///
/// Enabled by the default `hickory` feature. If you have your own
/// resolver, set `default-features = false` in your `Cargo.toml` and
/// implement [`SpfResolver`] directly.
#[cfg(feature = "hickory")]
pub mod hickory {
    use super::*;
    use hickory_resolver::TokioResolver;

    /// Wrap a `TokioResolver` for use as an [`SpfResolver`].
    pub struct HickoryResolver {
        inner: TokioResolver,
    }

    impl HickoryResolver {
        /// Construct from an existing resolver. The caller owns the
        /// resolver lifecycle; we hold a reference-counted clone.
        pub fn new(resolver: TokioResolver) -> Self {
            Self { inner: resolver }
        }
    }

    /// hickory's error type signals "no records found" via its message;
    /// the safest cross-version check is the error text. We treat
    /// no-records as Ok(empty) rather than an error.
    fn is_no_records<E: std::fmt::Display>(e: &E) -> bool {
        let s = e.to_string();
        s.contains("no record")
            || s.contains("NXDOMAIN")
            || s.contains("no records found")
            || s.contains("NoRecordsFound")
    }

    #[async_trait]
    impl SpfResolver for HickoryResolver {
        async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, SpfError> {
            // hickory 0.26: each `*_lookup` returns a `Lookup`. Iterate
            // `.answers()` to get `&[Record]`; `record.data` is the
            // typed `RData` enum.
            use hickory_resolver::proto::rr::RData;
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
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }

        async fn lookup_a(&self, domain: &str) -> Result<Vec<IpAddr>, SpfError> {
            use hickory_resolver::proto::rr::RData;
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
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }

        async fn lookup_aaaa(&self, domain: &str) -> Result<Vec<IpAddr>, SpfError> {
            use hickory_resolver::proto::rr::RData;
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
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }

        async fn lookup_mx(&self, domain: &str) -> Result<Vec<(u16, String)>, SpfError> {
            use hickory_resolver::proto::rr::RData;
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
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }
    }
}
