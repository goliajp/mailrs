//! Sender configuration, read once at boot from the environment.

use std::sync::Arc;

use mailrs_core_sidestate::families::dkim_signers::DkimSigners;

use super::*;

#[derive(Clone)]
pub(super) struct Cfg {
    pub(super) kevy_url: String,
    /// EHLO name announced on outbound sessions. Must match the PTR of
    /// the sending IP — receivers check forward-confirmed reverse DNS.
    pub(super) helo: String,
    /// Domain used in `MAILER-DAEMON@…` on DSNs we originate. Distinct
    /// from `helo`: this one has to survive DMARC at the far end, so it
    /// must be a domain with an aligned DKIM key, not the MTA hostname.
    /// See `bounce::compose_dsn`.
    pub(super) dsn_from_domain: String,
    pub(super) max_attempts: u32,
    pub(super) poll_ms: u64,
    pub(super) retry_min_secs: i64,
    /// How long a message may stay in the queue before it bounces —
    /// Postfix's `maximal_queue_lifetime`, and the rule RFC 5321
    /// §4.5.4.1 states. `max_attempts` is only a backstop beside it.
    pub(super) max_queue_lifetime_secs: i64,
    /// DKIM signing enabled when `MAILRS_DKIM_DOMAIN`,
    /// `MAILRS_DKIM_SELECTOR`, and `MAILRS_DKIM_PRIVATE_KEY_PEM_FILE`
    /// are all set. Public MX (Gmail / Outlook / etc.) drop unsigned
    /// mail from mailrs-hosted domains into spam.
    pub(super) dkim: Option<Arc<DkimSignConfig>>,
    /// Signing key for ARC seals on forwarded mail. Same key and
    /// selector as DKIM — ARC verifiers look the public key up under
    /// `<selector>._domainkey.<domain>`, exactly where DKIM's already
    /// is, so sealing needs no new DNS.
    pub(super) arc_key: Option<Arc<mailrs_dkim::RsaSigningKey>>,
}

/// Parse the DKIM private key once for ARC sealing.
///
/// Separate from `DkimSignConfig`'s lazily-parsed copy because that one
/// is private to the signer. Returns `None` when no key is configured,
/// which simply means forwards go out unsealed.
pub(super) fn load_arc_key() -> Option<Arc<mailrs_dkim::RsaSigningKey>> {
    let path = std::env::var("MAILRS_DKIM_PRIVATE_KEY").ok()?;
    let pem = std::fs::read_to_string(&path).ok()?;
    match mailrs_dkim::RsaSigningKey::from_pkcs8_pem(&pem) {
        Ok(k) => Some(Arc::new(k)),
        Err(e) => {
            tracing::warn!(error = %e, "ARC: key unparseable; forwards will not be sealed");
            None
        }
    }
}

pub(super) fn load_dkim_from_env() -> Option<Arc<DkimSignConfig>> {
    let domain = std::env::var("MAILRS_DKIM_DOMAIN").ok()?;
    let selector = std::env::var("MAILRS_DKIM_SELECTOR").ok()?;
    // Accept either the monolith's env-var convention
    // (MAILRS_DKIM_PRIVATE_KEY = file path) or inline PEM
    // (MAILRS_DKIM_PRIVATE_KEY_PEM). The file path takes precedence.
    let pem = if let Ok(path) = std::env::var("MAILRS_DKIM_PRIVATE_KEY") {
        match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(%path, err = %e, "MAILRS_DKIM_PRIVATE_KEY unreadable");
                return None;
            }
        }
    } else if let Ok(path) = std::env::var("MAILRS_DKIM_PRIVATE_KEY_PEM_FILE") {
        match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(%path, err = %e, "MAILRS_DKIM_PRIVATE_KEY_PEM_FILE unreadable");
                return None;
            }
        }
    } else if let Ok(pem) = std::env::var("MAILRS_DKIM_PRIVATE_KEY_PEM") {
        pem
    } else {
        return None;
    };
    // Per-domain overrides from MAILRS_DKIM_KEYS. Without these every
    // outbound message signs with the default `d=`, which only aligns
    // for the default domain — every other hosted domain fails DMARC
    // the moment SPF stops covering it (i.e. on any forward).
    let extra_keys = mailrs_outbound_queue::dkim_env::extra_keys_from_env();
    if extra_keys.is_empty() {
        tracing::info!("DKIM: single-domain mode (MAILRS_DKIM_KEYS unset)");
    } else {
        let mut domains: Vec<&str> = extra_keys.keys().map(String::as_str).collect();
        domains.sort_unstable();
        tracing::info!(
            count = extra_keys.len(),
            domains = %domains.join(","),
            "DKIM: per-domain signing keys loaded"
        );
    }
    Some(Arc::new(DkimSignConfig {
        selector,
        domain,
        private_key_pem: pem,
        parsed_key: Arc::new(std::sync::OnceLock::new()),
        extra_keys,
    }))
}

/// What `dkim` can sign, as the domain check reads it.
fn dkim_signers(dkim: Option<&DkimSignConfig>) -> DkimSigners {
    match dkim {
        Some(d) => DkimSigners {
            default_domain: Some(d.domain.to_ascii_lowercase()),
            selectors: mailrs_outbound_queue::dkim_signers::usable_selectors(d),
        },
        None => DkimSigners::default(),
    }
}

/// Replace the published DKIM signers with what `dkim` can sign.
pub(super) fn publish_dkim_signers(
    conn: &mut kevy_client::Connection,
    dkim: Option<&DkimSignConfig>,
) -> std::io::Result<DkimSigners> {
    let signers = dkim_signers(dkim);
    mailrs_core_sidestate::families::dkim_signers::publish(conn, &signers, now_secs())?;
    Ok(signers)
}

/// Connect once at boot, exiting when that fails so a bad URL surfaces
/// now, and publish which domains this process signs for: webapi's domain
/// check cannot read the key files and asks this instead.
pub(super) fn boot_kevy(cfg: &Cfg) {
    let mut conn = match kevy(&cfg.kevy_url) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(err = %e, "kevy connect failed at boot — exiting");
            std::process::exit(2);
        }
    };
    match publish_dkim_signers(&mut conn, cfg.dkim.as_deref()) {
        Ok(s) => tracing::info!(
            domains = s.selectors.len(),
            "DKIM: published signing domains"
        ),
        Err(e) => tracing::warn!(err = %e, "DKIM: publishing signing domains failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mailrs_core_sidestate::families::dkim_signers::read;

    #[test]
    fn a_sender_without_dkim_publishes_that_it_signs_nothing() {
        let mut c = kevy_client::Connection::connect("mem://sender-dkim-off").unwrap();
        publish_dkim_signers(&mut c, None).unwrap();
        let back = read(&mut c).unwrap().expect("published");
        assert_eq!(back.signers, DkimSigners::default());
    }

    #[test]
    fn a_key_that_does_not_parse_is_not_published_as_a_signer() {
        let mut c = kevy_client::Connection::connect("mem://sender-dkim-broken").unwrap();
        let mut extra = std::collections::HashMap::new();
        extra.insert(
            "golia.ai".to_string(),
            mailrs_outbound_queue::dkim_sign::DkimDomainKey {
                selector: "s2".into(),
                private_key_pem: "not a key".into(),
                ..Default::default()
            },
        );
        let cfg = DkimSignConfig {
            selector: "mail".into(),
            domain: "Golia.JP".into(),
            private_key_pem: "not a key either".into(),
            extra_keys: extra,
            ..Default::default()
        };
        publish_dkim_signers(&mut c, Some(&cfg)).unwrap();
        let back = read(&mut c).unwrap().expect("published");
        assert_eq!(back.signers.default_domain.as_deref(), Some("golia.jp"));
        assert!(back.signers.selectors.is_empty(), "{back:?}");
    }
}
