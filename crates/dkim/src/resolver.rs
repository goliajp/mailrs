//! DNS resolver trait for DKIM public-key TXT lookups.
//!
//! DKIM verifiers need exactly one DNS query type: TXT at
//! `<selector>._domainkey.<domain>`. We define a minimal trait so
//! callers plug in their own DNS layer.

use async_trait::async_trait;

use crate::error::DkimError;

/// Minimal DNS interface — DKIM only needs TXT lookups.
///
/// Implementors map NXDOMAIN to `Ok(vec![])` (the caller maps that
/// to [`DkimResult::PermError`] per RFC 6376 §6.1.2). Reserve
/// `Err(DkimError::DnsTempError)` for actual lookup failures.
#[async_trait]
pub trait DkimResolver: Send + Sync {
    /// TXT records for `domain`. For DKIM, the caller passes
    /// `<selector>._domainkey.<signing-domain>`.
    async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DkimError>;
}

/// Ready-made [`DkimResolver`] over `hickory_resolver::TokioResolver`.
/// Enabled by the default `hickory` feature.
#[cfg(feature = "hickory")]
pub mod hickory {
    use super::*;
    use hickory_resolver::proto::rr::RData;
    use hickory_resolver::TokioResolver;

    /// Wrap a `TokioResolver` for use as a [`DkimResolver`].
    pub struct HickoryDkimResolver {
        inner: TokioResolver,
    }

    impl HickoryDkimResolver {
        /// Construct from an existing `TokioResolver`.
        pub fn new(resolver: TokioResolver) -> Self {
            Self { inner: resolver }
        }
    }

    /// hickory signals "no records found" via its error message; the
    /// safest cross-version check is the error text.
    fn is_no_records<E: std::fmt::Display>(e: &E) -> bool {
        let s = e.to_string();
        s.contains("no record")
            || s.contains("NXDOMAIN")
            || s.contains("no records found")
            || s.contains("NoRecordsFound")
    }

    #[async_trait]
    impl DkimResolver for HickoryDkimResolver {
        async fn lookup_txt(&self, domain: &str) -> Result<Vec<String>, DkimError> {
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
                Err(e) => Err(DkimError::DnsTempError(e.to_string())),
            }
        }
    }
}
