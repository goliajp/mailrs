//! What a set-up screen needs from this, stated as tests.

use mailrs_mailprovider::{AuthKind, Autodiscover, Protocol, Tls, preset_for, preset_for_domain};

#[test]
fn the_big_four_are_known_by_address() {
    for addr in [
        "someone@gmail.com",
        "someone@googlemail.com",
        "someone@outlook.com",
        "someone@hotmail.co.jp",
        "someone@qq.com",
        "someone@163.com",
    ] {
        assert!(preset_for(addr).is_some(), "{addr} was not recognised");
    }
}

#[test]
fn gmail_is_oauth_and_never_a_password() {
    let p = preset_for("someone@gmail.com").expect("known");
    assert_eq!(p.auth, AuthKind::OAuth2);
    assert_eq!(p.imap.host, "imap.gmail.com");
    assert_eq!(p.imap.port, 993);
    assert_eq!(p.imap.tls, Tls::Implicit);
    assert_eq!(p.smtp.port, 587);
    assert_eq!(p.smtp.tls, Tls::StartTls);
}

/// The one that generates support mail: a QQ login password is refused
/// and the message does not explain why. The screen has to say where
/// the authorisation code comes from, so the preset has to carry it.
#[test]
fn qq_asks_for_an_authorisation_code_and_says_where() {
    let p = preset_for("someone@qq.com").expect("known");
    assert_eq!(p.auth, AuthKind::AppPassword);
    let help = p.secret_help.expect("no instructions for an app password");
    assert!(help.url.starts_with("https://"), "{}", help.url);
    assert!(!help.what.is_empty());
}

/// Every preset that cannot take a login password must say where to get
/// what it does take. Checked over the whole table, so a preset added
/// later cannot quietly ship without instructions.
#[test]
fn no_preset_demands_a_secret_without_saying_where_to_get_it() {
    for p in mailrs_mailprovider::ALL {
        if p.auth == AuthKind::AppPassword {
            assert!(
                p.secret_help.is_some(),
                "{} wants an app password and does not say where from",
                p.domains[0]
            );
        }
    }
}

/// Gmail's All Mail holds a copy of every message; syncing it as a
/// folder downloads the mailbox twice.
#[test]
fn gmail_hides_the_folder_that_would_double_the_mailbox() {
    let p = preset_for("someone@gmail.com").expect("known");
    assert!(p.skip_folders.iter().any(|f| f.contains("All Mail")));
}

#[test]
fn an_unknown_domain_has_no_preset() {
    assert!(preset_for("someone@a-university.example").is_none());
    assert!(preset_for("not an address").is_none());
}

#[test]
fn the_domain_is_matched_case_insensitively() {
    assert!(preset_for("Someone@GMail.COM").is_some());
    assert!(preset_for_domain("GMAIL.com").is_some());
}

/// Autodiscovery is ordered: the provider's own SRV records first
/// because they are authoritative, then the community database, then a
/// guess. A guess first would ship wrong settings that appear to work
/// until they do not.
#[test]
fn autodiscovery_asks_the_authoritative_source_first() {
    let steps = Autodiscover::for_domain("a-university.example");
    assert!(matches!(steps.first(), Some(Autodiscover::Srv { .. })));
    assert!(matches!(steps.last(), Some(Autodiscover::Guess { .. })));
    assert!(
        steps
            .iter()
            .any(|s| matches!(s, Autodiscover::Ispdb { .. }))
    );
}

#[test]
fn the_srv_names_are_the_ones_rfc_6186_registered() {
    let steps = Autodiscover::for_domain("example.com");
    let names: Vec<String> = steps
        .iter()
        .filter_map(|s| match s {
            Autodiscover::Srv { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"_imaps._tcp.example.com".to_string()));
    assert!(names.contains(&"_submission._tcp.example.com".to_string()));
}

#[test]
fn a_guess_is_the_conventional_hostname() {
    let steps = Autodiscover::for_domain("example.com");
    let Some(Autodiscover::Guess { imap, smtp }) = steps.last() else {
        panic!("no guess");
    };
    assert_eq!(imap.host, "imap.example.com");
    assert_eq!(imap.protocol, Protocol::Imap);
    assert_eq!(smtp.host, "smtp.example.com");
}
