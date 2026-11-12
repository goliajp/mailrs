use super::{LinkAddress, assess, encode};

const ME: &str = "lihao@golia.jp";

fn me() -> Vec<String> {
    vec![ME.into()]
}

fn named(hosts: &[&str]) -> Vec<String> {
    hosts.iter().map(|h| h.to_string()).collect()
}

#[test]
fn base64_is_standard_without_padding() {
    assert_eq!(encode(b"lihao@golia.jp"), "bGloYW9AZ29saWEuanA");
}

/// The two production had: a forwarder on one unrelated host, the page on
/// another.
#[test]
fn a_base64_address_forwarded_to_an_unnamed_host_is_found() {
    for link in [
        "https://socialmanager.crockadoodle.com/away?url=https://The.smartechla.com/JjYrw7bGloYW9AZ29saWEuanA=HxAtxH01",
        "https://www.satmagazine.com/r.php?u=https%3A%2F%2Fsmartechla.com%2Fx%2FbGloYW9AZ29saWEuanA",
    ] {
        let got = assess(&[link], &[], &named(&["schustermanagement.com"]), &me());
        assert_eq!(
            got.base64_via_redirect.as_deref(),
            Some("smartechla.com"),
            "{link}"
        );
    }
}

/// Anthropic's sign-in link: base64 in the `#` part, straight to
/// `claude.ai`, which the message also links to plainly.
#[test]
fn a_direct_sign_in_link_is_not_a_redirect() {
    let link = "https://claude.ai/magic-link#abc:bGloYW9AZ29saWEuanA";
    let got = assess(
        &[link, "https://claude.ai/"],
        &[],
        &named(&["mail.anthropic.com"]),
        &me(),
    );
    assert_eq!(got, LinkAddress::default());
    // even if the plain link were missing, a direct link is no redirect
    let alone = assess(&[link], &[], &named(&["mail.anthropic.com"]), &me());
    assert_eq!(alone.base64_via_redirect, None);
}

/// A tracker on the sender's own domain forwarding to its own site.
#[test]
fn a_forwarder_the_message_names_is_not_unrelated() {
    let link = "https://click.habbymail.com/c?u=https://habby.com/p/bGloYW9AZ29saWEuanA";
    let got = assess(&[link], &[], &named(&["habbymail.com"]), &me());
    assert_eq!(got.base64_via_redirect, None);
}

#[test]
fn an_address_in_the_fragment_to_an_unnamed_host_is_found() {
    for (link, to) in [
        (
            "https://nearped.com/network/index.html#lihao@golia.jp",
            "nearped.com",
        ),
        (
            "https://cc.naver.com/cc?a=pst.link&u=https%3A%2F%2FGolia.equltynj.com/c03e%2F#lihao@golia.jp",
            "equltynj.com",
        ),
        // someone else's address, as the fake invoice carried
        (
            "https://mdbgo.io/x/#johnsipp@fenchruchadvisory.com",
            "mdbgo.io",
        ),
        // percent-encoded `@`
        ("https://x.example/p#lihao%40golia.jp", "x.example"),
    ] {
        let got = assess(&[link], &[], &named(&["stellarbbs.com"]), &me());
        assert_eq!(got.fragment_address.as_deref(), Some(to), "{link}");
    }
}

/// Two copies of the same button are one witness, not two.
#[test]
fn the_same_button_twice_does_not_name_its_host() {
    let link = "https://nearped.com/network/index.html#lihao@golia.jp";
    let got = assess(&[link, link], &[], &named(&["stellarbbs.com"]), &me());
    assert_eq!(got.fragment_address.as_deref(), Some("nearped.com"));
}

#[test]
fn a_host_the_message_names_elsewhere_is_not_unnamed() {
    let link = "https://app.example/verify#lihao@golia.jp";
    // a plain link to the same site
    let got = assess(
        &[link, "https://app.example/help"],
        &[],
        &named(&["mail.example"]),
        &me(),
    );
    assert_eq!(got.fragment_address, None);
    // its logo
    let got = assess(
        &[link],
        &named(&["cdn.app.example"]),
        &named(&["mail.example"]),
        &me(),
    );
    assert_eq!(got.fragment_address, None);
    // the sender's own domain
    let got = assess(&[link], &[], &named(&["app.example"]), &me());
    assert_eq!(got.fragment_address, None);
}

/// An address in the path or query is everywhere in genuine mail.
#[test]
fn an_address_outside_the_fragment_is_not_read() {
    let link = "https://chatgpt.com/invite?email=lihao@golia.jp";
    let got = assess(&[link], &[], &named(&["tm.openai.com"]), &me());
    assert_eq!(got, LinkAddress::default());
}

/// Not recognised today, written down so nobody takes it as covered: a
/// destination hidden in base64 is not followed, so the forwarder's own
/// host stands for it.
#[test]
fn a_base64_destination_is_not_followed() {
    let inner = encode(b"https://phish.example/login");
    let link = format!("https://r.example/go/{inner}?e=bGloYW9AZ29saWEuanA");
    let got = assess(&[&link], &[], &named(&["sender.example"]), &me());
    // the link's own host is taken as the destination, so it is no redirect
    assert_eq!(got.base64_via_redirect, None);
}

/// The genuine shape the corpus does not have, kept for the 2026-12-11
/// review: a new service's verification mail, the reader's address in
/// the `#` part, straight to its app on another domain named nowhere else
/// in the message. The fact is set; the rule reading it only scores.
#[test]
fn a_new_services_verification_link_sets_the_fact() {
    let link = "https://app.newservice.example/verify#lihao@golia.jp";
    let got = assess(&[link], &[], &named(&["mail.newservice.io"]), &me());
    assert_eq!(got.fragment_address.as_deref(), Some("newservice.example"));
}
