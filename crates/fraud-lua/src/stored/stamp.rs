//! What a stored message says about how this deployment's receiver judged
//! it, read only from the `Authentication-Results:` that receiver wrote.
//!
//! The receiver puts its header on top, directly above its own trace, and
//! removes any incoming one naming it. A message imported, appended or
//! filed as a sent copy before those paths removed it too can still carry
//! a header naming us that we never wrote; it sits where no receiver put
//! it (`mailrs_rfc5322::auth_results::written_by`), and is not read.

/// The receiver's own findings on a message.
pub struct Stamp {
    /// It arrived on the inbound path, the only one the receiver stamps.
    pub unauthenticated: bool,
    /// `spf`, `dkim`, `dmarc` of the first `Authentication-Results:`;
    /// empty when that header names us but we did not write it.
    pub auth: (String, String, String),
    /// A DKIM signature by the `From:` organisation passed, from the
    /// receiver's entries per signature; `None` without them.
    pub dkim_from_org_passed: Option<bool>,
}

/// Read `raw`'s stamp. `our_stamp` is this deployment's authserv-id
/// (`MAILRS_HOSTNAME`); without it nothing counts as ours.
#[must_use]
pub fn read(raw: &[u8], our_stamp: Option<&str>, from_host: &str) -> Stamp {
    let ours = our_stamp.filter(|h| mailrs_rfc5322::auth_results::written_by(raw, h).is_some());
    let first = mailrs_inbound::identity::auth_results_authserv(raw);
    let forged = match (our_stamp, ours, first.as_deref()) {
        (Some(us), None, Some(named)) => named.eq_ignore_ascii_case(us),
        _ => false,
    };
    let auth = match forged {
        true => Default::default(),
        false => mailrs_inbound::identity::auth_results_tokens(raw),
    };
    Stamp {
        unauthenticated: ours.is_some(),
        auth,
        dkim_from_org_passed: ours
            .and_then(|us| mailrs_inbound::dkim_results::ours(raw, us))
            .map(|sigs| mailrs_fraud::org_domain::passed_for(from_host, &sigs)),
    }
}
