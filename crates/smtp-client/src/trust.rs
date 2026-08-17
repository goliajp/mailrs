//! Which certificate authorities an SMTP peer is verified against.
//!
//! Split out of `connection.rs` at the 500-line limit, and it belongs
//! apart anyway: the connection state machine is about SMTP, and this is
//! about whom we believe.

use rustls::ClientConfig;

/// Default PKIX client config used by [`SmtpConnection::try_starttls`]:
/// `webpki-roots` trust store, no client auth, empty ALPN.
pub fn default_pkix_client_config() -> ClientConfig {
    let mut config = ClientConfig::builder()
        .with_root_certificates(pkix_root_store())
        .with_no_client_auth();
    config.alpn_protocols = vec![];
    config
}

/// The anchors an SMTP peer is verified against: the platform's store
/// **and** the compiled-in Mozilla set, never one instead of the other.
///
/// `webpki-roots` alone is the wrong store for mail. It tracks Mozilla's
/// **browser** program, and on 2026-08-17 that difference stopped mail
/// to Microsoft: `mail.protection.outlook.com` chains through `DigiCert
/// SHA2 Secure Server CA` to `DigiCert Global Root CA`, a root valid
/// until 2031 that Mozilla no longer ships. `webpki-roots` 1.0.8 carries
/// DigiCert G2, G3, G4, Assured ID G2/G3 and both G5 roots — not that
/// one. Under MTA-STS enforce the handshake failed and, correctly,
/// refused to downgrade, so every message to every Microsoft 365 tenant
/// died with `UnknownIssuer`. Google, Yahoo Japan and iCloud were
/// unaffected; `openssl s_client` on the same host succeeded, because
/// Debian's `ca-certificates` has the root.
///
/// The platform store is what every other MTA verifies against, and it
/// is the operator's choice rather than a browser vendor's. So it is
/// loaded first — and the compiled-in set is **added**, not replaced, so
/// an image without `ca-certificates` still has anchors rather than
/// silently trusting nothing.
pub fn pkix_root_store() -> rustls::RootCertStore {
    let mut store = rustls::RootCertStore::empty();
    match rustls_native_certs::load_native_certs() {
        result if !result.certs.is_empty() => {
            let (added, ignored) = store.add_parsable_certificates(result.certs);
            if !result.errors.is_empty() {
                tracing_ignored(added, ignored, result.errors.len());
            }
        }
        result => tracing_ignored(0, 0, result.errors.len()),
    }
    // Always, regardless of what the platform gave us. The two sets
    // overlap heavily; `add_trust_anchors` de-duplicates by subject, and
    // a duplicate anchor costs a comparison, while a missing one costs
    // an operator's mail.
    store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    store
}

/// Say what the platform store gave us when it gave us less than all of
/// it. Silence here is how a shrinking trust store looks like a working
/// one.
fn tracing_ignored(added: usize, ignored: usize, errors: usize) {
    if ignored > 0 || errors > 0 {
        eprintln!(
            "smtp-client: platform trust store: {added} anchors added, \
             {ignored} unparsable, {errors} read errors"
        );
    }
}

#[cfg(test)]
mod trust_store_tests {
    use super::*;

    /// The subject of an anchor, as printable bytes. A trust anchor's
    /// subject is a DER RDN sequence; the CN survives as a run of ASCII
    /// inside it, which is all this needs to identify one.
    fn subjects(store: &rustls::RootCertStore) -> Vec<String> {
        store
            .roots
            .iter()
            .map(|r| {
                String::from_utf8_lossy(r.subject.as_ref())
                    .chars()
                    .map(|c| {
                        if c.is_ascii_graphic() || c == ' ' {
                            c
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// **Microsoft's mail anchors at a root Mozilla no longer ships.**
    ///
    /// 2026-08-17: a message to `hotmail.com` bounced with
    /// `mta-sts enforce: hotmail-com.olc.protection.outlook.com TLS
    /// handshake failed`. The reason, once it was extracted from the
    /// arm that was throwing it away, was
    /// `CertificateNotTrusted("UnknownIssuer")`.
    ///
    /// The chain is `mail.protection.outlook.com` → `DigiCert SHA2
    /// Secure Server CA` → **`DigiCert Global Root CA`**, and that root
    /// — valid until 2031, present in Debian's `ca-certificates`, which
    /// is why `openssl s_client` succeeded from the same host — is not
    /// among the 121 anchors in `webpki-roots` 1.0.8. It carries
    /// DigiCert G2, G3, G4, Assured ID G2/G3 and both G5 roots, and not
    /// the original.
    ///
    /// `webpki-roots` tracks Mozilla's **browser** program. An SMTP peer
    /// is not a browser, and Microsoft is one of the two largest mail
    /// operators on the internet: measured the same day, every
    /// `*.olc.protection.outlook.com` and `*.mail.protection.outlook.com`
    /// host failed while Google, Yahoo Japan and iCloud succeeded. Under
    /// MTA-STS enforce a failed handshake correctly refuses to
    /// downgrade, so the mail died — for **every Microsoft 365 tenant**,
    /// not only hotmail.
    #[test]
    fn the_root_microsofts_mail_anchors_at_is_trusted() {
        let s = subjects(&pkix_root_store());
        assert!(
            s.iter().any(|s| s.contains("DigiCert Global Root CA")),
            "the anchor every Microsoft-hosted domain chains to is missing, \
             so mail to Microsoft cannot be delivered under MTA-STS enforce; \
             {} anchors present",
            s.len()
        );
    }

    /// The platform store **adds**; it never replaces.
    ///
    /// A container without `ca-certificates`, or a platform load that
    /// fails, must leave the compiled-in set intact rather than
    /// silently shrinking trust to nothing — which would turn one
    /// operator's mail failing into all of it failing, and would look
    /// exactly like this bug wearing a different hostname.
    #[test]
    fn the_compiled_in_anchors_are_never_lost() {
        let n = pkix_root_store().roots.len();
        let baseline = webpki_roots::TLS_SERVER_ROOTS.len();
        assert!(
            n >= baseline,
            "the trust store shrank below the compiled-in set: {n} < {baseline}"
        );
    }
}
