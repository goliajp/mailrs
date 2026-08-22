//! The REST surface for mailboxes somewhere else.
//!
//! The store calls need a network kevy, so what is asserted here is
//! what can be asserted without one — and it is the part that matters
//! most: the shapes a set-up screen reads, and the refusal that keeps a
//! credential from ever being written in the clear.

use mailrs_core_sidestate::families::external_accounts as ext;
use mailrs_mailprovider::{AuthKind, preset_for};

/// A phone posts an address and a password; everything else is filled
/// in. If this stops being true the set-up screen grows six fields.
#[test]
fn a_known_provider_needs_only_an_address() {
    let p = preset_for("someone@qq.com").expect("qq is known");
    assert_eq!(p.imap.host, "imap.qq.com");
    assert_eq!(p.smtp.host, "smtp.qq.com");
    assert_eq!(p.auth, AuthKind::AppPassword);
    assert!(p.secret_help.is_some(), "no instructions for the 授权码");
}

/// The row that reaches the store must be usable, or the account sits
/// in the list failing forever with a message about a host that is
/// empty because nobody filled it in.
#[test]
fn a_row_built_from_a_preset_validates() {
    let row = ext::AccountRow {
        id: "ext_1".into(),
        email: "someone@qq.com".into(),
        display_name: "QQ".into(),
        provider: "qq".into(),
        incoming: ext::Endpoint {
            protocol: "imap".into(),
            host: "imap.qq.com".into(),
            port: 993,
            tls: ext::Tls::Implicit,
        },
        outgoing: ext::Endpoint {
            protocol: "smtp".into(),
            host: "smtp.qq.com".into(),
            port: 465,
            tls: ext::Tls::Implicit,
        },
        auth: ext::AuthKind::AppPassword,
        ..ext::AccountRow::default()
    };
    assert_eq!(ext::validate(&row), Ok(()));
}

/// An address nobody has a preset for still produces something to try,
/// in an order that puts the authoritative answer first.
#[test]
fn an_unknown_domain_still_gets_a_starting_point() {
    use mailrs_mailprovider::Autodiscover;
    let steps = Autodiscover::for_domain("a-university.example");
    assert!(matches!(steps.first(), Some(Autodiscover::Srv { .. })));
    assert!(matches!(steps.last(), Some(Autodiscover::Guess { .. })));
}

/// The whole point of the module, asserted where it cannot rot: what
/// is stored is not the secret.
#[test]
fn what_is_stored_for_a_secret_is_not_the_secret() {
    let key = mailrs_secretbox::Key::from_passphrase("a deployment key");
    let sealed = mailrs_secretbox::seal(&key, b"hunter2-authorisation-code").expect("seal");
    assert!(!sealed.contains("hunter2"), "{sealed}");
    assert_eq!(
        mailrs_secretbox::open(&key, &sealed).expect("open"),
        b"hunter2-authorisation-code"
    );
}

/// And a deployment without a key refuses rather than falling back.
///
/// The failure mode this forecloses is the quiet one: a missing key
/// treated as "store it plainly for now", which nobody notices because
/// everything works.
#[test]
fn a_missing_deployment_key_has_no_plaintext_fallback() {
    let src = include_str!("../src/handlers/external_accounts.rs");
    assert!(
        src.contains("SERVICE_UNAVAILABLE"),
        "a missing MAILRS_ACCOUNT_KEY must refuse the request"
    );
    for fallback in ["unwrap_or_default()", "unwrap_or(String::new())"] {
        let near_key = src
            .split("fn sealing_key")
            .nth(1)
            .and_then(|s| s.split("\n}").next())
            .unwrap_or_default();
        assert!(
            !near_key.contains(fallback),
            "sealing_key falls back with {fallback}"
        );
    }
}
