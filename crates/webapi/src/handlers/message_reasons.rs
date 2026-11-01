//! The reasons shown beside a message as it is read: what each sender
//! check said, and which of its links point somewhere other than they
//! claim or at a host listed for fraud.
//!
//! Read from the stored file, so every core serves the same answer: the
//! `Authentication-Results:` header the receiver wrote on top, and the
//! links in the HTML. Structural facts only: what a link's text shows
//! against where it goes, and whether its host is listed. Nothing here
//! reads the wording of the mail.

use mailrs_inbound::Reason;
use mailrs_inbound::reason::{LINK_HOST_LISTED, LINK_SHOWS_ANOTHER_HOST, SENDER_BIDI};

/// One host the message links to, and why a reader should look twice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LinkWarning {
    /// The host the link goes to, lower-case. A client marks every link
    /// in the message whose target is on this host.
    pub host: String,
    /// What the text of the link shows, when that is another host. A
    /// client marks only the links whose text shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<String>,
    /// Why, one code per fact.
    pub reasons: Vec<Reason>,
}

/// What SPF, DKIM and DMARC each said about the sender, and whether the
/// name or subject renders reordered. Empty for mail that carries no
/// verdict of ours: the user's own, and mail stored before the receiver
/// wrote one.
pub fn sender_checks(raw: &[u8]) -> Vec<Reason> {
    let (spf, dkim, dmarc) = mailrs_inbound::identity::auth_results_tokens(raw);
    if spf.is_empty() && dkim.is_empty() && dmarc.is_empty() {
        return Vec::new();
    }
    let mut out = mailrs_inbound::verdict::auth_checks(&spf, &dkim, &dmarc);
    if mailrs_inbound::deception_in_identity(raw).bidi_override {
        out.push(Reason::new(SENDER_BIDI));
    }
    out
}

/// The links worth a warning. `listed` answers whether a host is on the
/// reputation list; it is the process's current set in the handler.
pub fn link_warnings(raw: &[u8], listed: impl Fn(&str) -> bool) -> Vec<LinkWarning> {
    let parsed = mailrs_mime::parse(raw);
    let mut out: Vec<LinkWarning> = Vec::new();
    for pair in mailrs_inbound::body::shown_link_pairs(&parsed) {
        let mut reasons = vec![
            Reason::new(LINK_SHOWS_ANOTHER_HOST)
                .with("shown", pair.shown.clone())
                .with("host", pair.target.clone()),
        ];
        if listed(&pair.target) {
            reasons.push(Reason::new(LINK_HOST_LISTED).with("host", pair.target.clone()));
        }
        out.push(LinkWarning {
            host: pair.target,
            shown: Some(pair.shown),
            reasons,
        });
    }
    for host in mailrs_inbound::scan_input::link_hosts(&parsed) {
        let marked = out.iter().any(|w| w.host == host && w.shown.is_none());
        if !marked && listed(&host) {
            out.push(LinkWarning {
                reasons: vec![Reason::new(LINK_HOST_LISTED).with("host", host.clone())],
                host,
                shown: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: &[u8] = b"Authentication-Results: mx.golia.ai; spf=pass smtp.mailfrom=x.example; dkim=fail; dmarc=fail\r\n\
From: IT <doc@x.example>\r\n\
Content-Type: text/html\r\n\
\r\n\
<p>Visit <a href=\"https://webmail-oauth.wzldq.com/auth\">www.golia.jp/lihao/check</a>\
 or <a href=\"https://bad.example/x\">here</a>\
 or <a href=\"https://fine.example/\">fine</a></p>\r\n";

    fn codes(w: &LinkWarning) -> Vec<&str> {
        w.reasons.iter().map(|r| r.code.as_str()).collect()
    }

    #[test]
    fn each_check_is_named_with_what_it_said() {
        let checks = sender_checks(HTML);
        let said: Vec<(&str, &str)> = checks
            .iter()
            .map(|r| (r.params["check"].as_str(), r.params["result"].as_str()))
            .collect();
        assert_eq!(said, [("SPF", "pass"), ("DKIM", "fail"), ("DMARC", "fail")]);
    }

    #[test]
    fn mail_without_our_header_has_no_checks_to_show() {
        assert!(sender_checks(b"From: me@golia.jp\r\n\r\nhi\r\n").is_empty());
    }

    #[test]
    fn a_link_showing_another_host_and_a_listed_host_are_both_marked() {
        let w = link_warnings(HTML, |h| h == "bad.example");
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(w[0].host, "webmail-oauth.wzldq.com");
        assert_eq!(w[0].shown.as_deref(), Some("www.golia.jp"));
        assert_eq!(codes(&w[0]), ["link-shows-another-host"]);
        assert_eq!(w[1].host, "bad.example");
        assert_eq!(codes(&w[1]), ["link-host-listed"]);
    }

    /// Nothing listed and nothing misleading: no warnings, so a test that
    /// passed because every link was marked would fail here.
    #[test]
    fn plain_links_carry_no_warning() {
        let plain = b"Content-Type: text/html\r\n\r\n<a href=\"https://fine.example/\">fine.example/docs</a>\r\n";
        assert!(link_warnings(plain, |_| false).is_empty());
    }
}
