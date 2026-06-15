//! Adapter: server uses `mailrs-srs` (1.0.0) for SPF-aware envelope
//! sender rewriting. This file is a thin re-export so the existing
//! `super::srs::srs_rewrite` import in events/data.rs resolves
//! unchanged.

/// Forward-rewrite an envelope sender via mailrs-srs.
pub(super) fn srs_rewrite(sender: &str, local_domain: &str, secret: &str) -> String {
    mailrs_srs::rewrite(sender, local_domain, secret)
}
