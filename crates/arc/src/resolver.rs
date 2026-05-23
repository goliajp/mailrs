//! DNS resolver trait for ARC public-key lookups.
//!
//! ARC public keys live at the same DNS shape as DKIM keys:
//! `<selector>._domainkey.<domain>` returns a TXT record carrying the
//! `v=DKIM1; k=rsa; p=<base64>` body. We delegate to the existing
//! [`mailrs_dkim::DkimResolver`] trait so a single `HickoryResolver`
//! instance feeds both verifiers.

pub use mailrs_dkim::DkimResolver as ArcResolver;

#[cfg(feature = "hickory")]
pub use mailrs_dkim::resolver::hickory::HickoryDkimResolver as HickoryArcResolver;
