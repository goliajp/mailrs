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
/// `Err(SpfError::DnsPermError)` for malformed responses and for a
/// `domain` that is not a domain name, which no retry will fix.
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
    use hickory_resolver::proto::rr::Name;

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

    /// The name to ask for. A target that does not parse as a domain name
    /// (a macro left unexpanded, a space, an empty label) is the record's
    /// fault, and asking again later will not change it: a permanent
    /// error, not a temporary one. A character DNS allows but a host name
    /// does not, such as a `+` that `%{l}` brought in from a local part,
    /// fails to parse too, and this resolver cannot ask for that name
    /// either.
    ///
    /// No such domain, and no record of the type, are ordinary answers in
    /// SPF; each lookup turns them into an empty list
    /// (`is_no_records_found`) and anything else into a temporary error.
    fn target(domain: &str) -> Result<Name, SpfError> {
        Name::from_utf8(domain)
            .map_err(|e| SpfError::DnsPermError(format!("{domain:?} is not a domain name: {e}")))
    }

    #[async_trait]
    impl SpfResolver for HickoryResolver {
        async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, SpfError> {
            // hickory 0.26: each `*_lookup` returns a `Lookup`. Iterate
            // `.answers()` to get `&[Record]`; `record.data` is the
            // typed `RData` enum.
            use hickory_resolver::proto::rr::RData;
            match self.inner.txt_lookup(target(domain)?).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::TXT(txt) = &record.data {
                            out.push(txt.to_string());
                        }
                    }
                    Ok(out)
                }
                Err(e) if e.is_no_records_found() => Ok(Vec::new()),
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }

        async fn lookup_a(&self, domain: &str) -> Result<Vec<IpAddr>, SpfError> {
            use hickory_resolver::proto::rr::RData;
            match self.inner.ipv4_lookup(target(domain)?).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::A(a) = &record.data {
                            out.push(IpAddr::V4(a.0));
                        }
                    }
                    Ok(out)
                }
                Err(e) if e.is_no_records_found() => Ok(Vec::new()),
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }

        async fn lookup_aaaa(&self, domain: &str) -> Result<Vec<IpAddr>, SpfError> {
            use hickory_resolver::proto::rr::RData;
            match self.inner.ipv6_lookup(target(domain)?).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::AAAA(a) = &record.data {
                            out.push(IpAddr::V6(a.0));
                        }
                    }
                    Ok(out)
                }
                Err(e) if e.is_no_records_found() => Ok(Vec::new()),
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }

        async fn lookup_mx(&self, domain: &str) -> Result<Vec<(u16, String)>, SpfError> {
            use hickory_resolver::proto::rr::RData;
            match self.inner.mx_lookup(target(domain)?).await {
                Ok(resp) => {
                    let mut out = Vec::new();
                    for record in resp.answers() {
                        if let RData::MX(mx) = &record.data {
                            out.push((mx.preference, mx.exchange.to_utf8()));
                        }
                    }
                    Ok(out)
                }
                Err(e) if e.is_no_records_found() => Ok(Vec::new()),
                Err(e) => Err(SpfError::DnsTempError(e.to_string())),
            }
        }
    }
}

#[cfg(all(test, feature = "hickory"))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use hickory_resolver::TokioResolver;
    use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig};
    use hickory_resolver::net::runtime::TokioRuntimeProvider;
    use hickory_resolver::proto::op::{Message, OpCode, ResponseCode};

    use super::SpfResolver;
    use super::hickory::HickoryResolver;
    use crate::error::SpfError;

    /// A name server on loopback that answers every query with `code` and
    /// no records, and counts the queries it was sent.
    async fn answering(code: ResponseCode) -> (HickoryResolver, Arc<AtomicUsize>) {
        let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = sock.local_addr().unwrap().port();
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                let (n, from) = sock.recv_from(&mut buf).await.unwrap();
                counter.fetch_add(1, Ordering::SeqCst);
                let query = Message::from_vec(&buf[..n]).unwrap();
                let mut reply = Message::response(query.metadata.id, OpCode::Query);
                reply.metadata.recursion_desired = query.metadata.recursion_desired;
                reply.metadata.recursion_available = true;
                reply.metadata.response_code = code;
                reply.add_queries(query.queries.clone());
                sock.send_to(&reply.to_vec().unwrap(), from).await.unwrap();
            }
        });
        let mut udp = ConnectionConfig::udp();
        udp.port = port;
        let ns = NameServerConfig::new("127.0.0.1".parse().unwrap(), true, vec![udp]);
        let config = ResolverConfig::from_parts(None, vec![], vec![ns]);
        let resolver = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
            .build()
            .unwrap();
        (HickoryResolver::new(resolver), asked)
    }

    /// A macro left unexpanded in a record's target is the record's
    /// fault, and asking again later will not fix it.
    #[tokio::test]
    async fn a_target_that_is_not_a_domain_name_is_a_permanent_error() {
        let (r, asked) = answering(ResponseCode::NXDomain).await;
        for name in ["%{i}._spf.example.com", "a b.example.com", "x..example.com"] {
            assert!(
                matches!(r.lookup_txt(name).await, Err(SpfError::DnsPermError(_))),
                "txt {name}"
            );
            assert!(
                matches!(r.lookup_a(name).await, Err(SpfError::DnsPermError(_))),
                "a {name}"
            );
            assert!(
                matches!(r.lookup_aaaa(name).await, Err(SpfError::DnsPermError(_))),
                "aaaa {name}"
            );
            assert!(
                matches!(r.lookup_mx(name).await, Err(SpfError::DnsPermError(_))),
                "mx {name}"
            );
        }
        assert_eq!(asked.load(Ordering::SeqCst), 0, "nothing was sent");
    }

    /// No such domain and no such record are both ordinary answers in
    /// SPF: an empty list, not an error.
    #[tokio::test]
    async fn no_records_is_an_empty_answer() {
        for code in [ResponseCode::NXDomain, ResponseCode::NoError] {
            let (r, asked) = answering(code).await;
            assert!(r.lookup_txt("example.com").await.unwrap().is_empty());
            assert!(r.lookup_a("example.com").await.unwrap().is_empty());
            assert!(r.lookup_mx("_spf.example.com").await.unwrap().is_empty());
            assert!(asked.load(Ordering::SeqCst) > 0, "the server was asked");
        }
    }

    /// A server failure is temporary.
    #[tokio::test]
    async fn a_server_failure_is_a_temporary_error() {
        let (r, _) = answering(ResponseCode::ServFail).await;
        assert!(matches!(
            r.lookup_txt("example.com").await,
            Err(SpfError::DnsTempError(_))
        ));
    }
}
