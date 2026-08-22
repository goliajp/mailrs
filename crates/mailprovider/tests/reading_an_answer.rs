//! Turning what DNS said into settings.
//!
//! The lookups belong to the caller; reading their answers does not —
//! that is where the mistakes are, and they are testable without a
//! network.

use mailrs_mailprovider::{Protocol, Tls, from_srv};

#[test]
fn an_srv_answer_becomes_an_endpoint() {
    let e = from_srv("_imaps._tcp.example.com", "imap.example.com", 993).expect("an endpoint");
    assert_eq!(e.host, "imap.example.com");
    assert_eq!(e.port, 993);
    assert_eq!(e.protocol, Protocol::Imap);
    assert_eq!(e.tls, Tls::Implicit);
}

/// The record name carries the protection, and getting it backwards
/// either sends the password in the clear or fails every handshake.
#[test]
fn the_record_name_decides_the_protection() {
    assert_eq!(
        from_srv("_imap._tcp.example.com", "h", 143).unwrap().tls,
        Tls::StartTls
    );
    assert_eq!(
        from_srv("_submissions._tcp.example.com", "h", 465)
            .unwrap()
            .tls,
        Tls::Implicit
    );
    assert_eq!(
        from_srv("_submission._tcp.example.com", "h", 587)
            .unwrap()
            .tls,
        Tls::StartTls
    );
}

/// RFC 6186 §3.1: a target of `.` means the service is **not
/// offered**. Treating it as a hostname produces an account that
/// cannot connect and a person who is told their password is wrong.
#[test]
fn a_dot_target_means_the_service_is_not_offered() {
    assert!(from_srv("_imaps._tcp.example.com", ".", 993).is_none());
}

#[test]
fn a_record_we_do_not_understand_is_ignored() {
    assert!(from_srv("_sip._tcp.example.com", "h", 5060).is_none());
    assert!(from_srv("", "h", 993).is_none());
}

/// Resolvers hand back absolute names. A trailing dot in a hostname
/// makes TLS verification fail against a certificate that names the
/// domain without one.
#[test]
fn a_trailing_dot_is_trimmed_from_the_host() {
    assert_eq!(
        from_srv("_imaps._tcp.example.com", "imap.example.com.", 993)
            .unwrap()
            .host,
        "imap.example.com"
    );
}

#[test]
fn port_zero_is_not_a_port() {
    assert!(from_srv("_imaps._tcp.example.com", "h", 0).is_none());
}
