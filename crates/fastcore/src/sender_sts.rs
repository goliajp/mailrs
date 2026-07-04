//! MTA-STS policy discovery + cache for the fastcore sender (G8).
//!
//! Before delivering to a domain we look up its STS policy:
//!   1. DNS TXT `_mta-sts.<domain>` — presence (v=STSv1) signals STS
//!   2. HTTPS GET `https://mta-sts.<domain>/.well-known/mta-sts.txt`
//!   3. parse + cache in network kevy under `mailrs:mta-sts:<domain>`
//!      with the policy's own `max_age` as the TTL
//!
//! `mode: enforce` makes the caller (a) skip MX hosts not covered by
//! the policy's `mx:` patterns and (b) refuse plaintext downgrade —
//! a TLS failure becomes a transient error (retry) rather than a
//! silent plaintext send. `testing`/`none`/absent = opportunistic
//! (unchanged behaviour).

use mailrs_mta_sts::{Decision, Policy, PolicyMode, enforce, policy_url};

/// Look up + cache the STS policy for `domain`. Returns `None` when the
/// domain publishes no usable enforce/testing policy (caller stays
/// opportunistic). Fully fail-open: any DNS/HTTP/parse error → None.
pub async fn fetch_policy(kevy_url: &str, domain: &str) -> Option<Policy> {
    // cache hit?
    if let Some(p) = cache_get(kevy_url, domain) {
        return Some(p);
    }
    // DNS TXT presence check — cheap gate before the HTTPS fetch
    let resolver = mailrs_smtp_client::TokioResolver::builder_tokio()
        .ok()?
        .build()
        .ok()?;
    let txt_name = format!("_mta-sts.{domain}");
    let has_record = resolver
        .txt_lookup(txt_name)
        .await
        .ok()
        .map(|lookup| {
            lookup
                .answers()
                .iter()
                .any(|rec| rec.to_string().to_ascii_lowercase().contains("v=stsv1"))
        })
        .unwrap_or(false);
    if !has_record {
        return None;
    }
    // HTTPS fetch of the policy file
    let url = policy_url(domain);
    let body = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?
        .get(&url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    let policy = Policy::parse(&body).ok()?;
    cache_put(kevy_url, domain, &body, policy.max_age);
    Some(policy)
}

/// Enforcement verdict for one MX under a policy.
pub fn mx_decision(policy: &Policy, mx_host: &str) -> Decision {
    enforce(policy, mx_host)
}

/// True when the policy forbids plaintext / non-matching MX (i.e. a
/// TLS failure must abort delivery instead of downgrading).
pub fn is_enforce(policy: &Policy) -> bool {
    matches!(policy.mode, PolicyMode::Enforce)
}

fn cache_key(domain: &str) -> String {
    format!("mailrs:mta-sts:{}", domain.to_ascii_lowercase())
}

fn cache_get(kevy_url: &str, domain: &str) -> Option<Policy> {
    let mut conn = kevy_client::Connection::open(kevy_url).ok()?;
    let raw = conn.get(cache_key(domain).as_bytes()).ok().flatten()?;
    let body = String::from_utf8(raw).ok()?;
    Policy::parse(&body).ok()
}

fn cache_put(kevy_url: &str, domain: &str, body: &str, max_age: u64) {
    let Ok(mut conn) = kevy_client::Connection::open(kevy_url) else {
        return;
    };
    let key = cache_key(domain);
    if conn.set(key.as_bytes(), body.as_bytes()).is_ok() {
        // clamp TTL to a sane band even if the policy lies (RFC 8461 §3.2)
        let ttl = max_age.clamp(300, 31_557_600);
        let _ = conn.expire(key.as_bytes(), std::time::Duration::from_secs(ttl));
    }
}
