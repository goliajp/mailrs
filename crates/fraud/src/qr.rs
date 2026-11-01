//! Where the QR codes in a message point, against what the message says.
//!
//! A code is read off an image, after delivery, by the OCR worker. Two
//! things it can show without anyone's opinion of the sender: the image
//! prints one domain and its code goes to another, or the only way out of
//! a message with no links is a code to a host the message never names.

use crate::brand::registrable;

/// What a message's QR codes showed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QrFacts {
    /// Codes that carry a web address.
    pub codes: usize,
    /// A code's host is neither the sender's domain nor any linked host.
    pub host_not_in_message: bool,
    /// The text read off the images names a domain, and none of the
    /// domains it names is where the codes go.
    pub text_mismatch: bool,
}

/// Weigh `urls` (the codes' addresses) against the `From:` host, the hosts
/// the message links to, and the text read off its images.
#[must_use]
pub fn assess(
    urls: &[String],
    sender_host: &str,
    link_hosts: &[String],
    image_text: &str,
    never_listed: impl Fn(&str) -> bool,
) -> QrFacts {
    let code_domains: Vec<String> = urls
        .iter()
        .filter_map(|u| host_of(u))
        .map(|h| registrable(&h))
        .filter(|d| !d.is_empty())
        .collect();
    if code_domains.is_empty() {
        return QrFacts::default();
    }
    let sender = registrable(sender_host);
    let linked: Vec<String> = link_hosts.iter().map(|h| registrable(h)).collect();
    let host_not_in_message = code_domains
        .iter()
        .any(|d| *d != sender && !linked.contains(d) && !never_listed(d));
    let named = named_domains(image_text);
    let text_mismatch = !named.is_empty() && !named.iter().any(|n| code_domains.contains(n));
    QrFacts {
        codes: code_domains.len(),
        host_not_in_message,
        text_mismatch,
    }
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

// a bare `name.ext` counts only under a common tld, so `invoice.pdf` is not a domain
const COMMON_TLDS: &[&str] = &[
    "com", "net", "org", "jp", "io", "co", "info", "biz", "xyz", "top", "app", "dev", "cn", "ru",
    "uk", "de", "fr", "us", "me", "online", "site", "shop",
];

/// Registrable domains written in `text` after a scheme or `www.`, or as `name.tld` under a common tld.
fn named_domains(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || "()[]<>\"'、。，,".contains(c)) {
        let lower = word.to_ascii_lowercase();
        let marked = lower.starts_with("http://")
            || lower.starts_with("https://")
            || lower.starts_with("www.");
        let word = word
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(['.', ':', ';'])
            .to_ascii_lowercase();
        let labels: Vec<&str> = word.split('.').collect();
        let looks_like = labels.len() >= 2
            && labels.iter().all(|l| {
                !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            && labels.last().is_some_and(|tld| {
                tld.bytes().all(|b| b.is_ascii_alphabetic())
                    && (marked || COMMON_TLDS.contains(tld))
            });
        if looks_like {
            let d = registrable(&word);
            if !out.contains(&d) {
                out.push(d);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never(d: &str) -> bool {
        d == "google.com"
    }

    #[test]
    fn a_code_that_goes_somewhere_the_image_does_not_say_is_a_mismatch() {
        let urls = ["https://secure-login.evil.example/a".to_string()];
        let f = assess(
            &urls,
            "bank.example",
            &[],
            "ログイン www.bank.example でご確認ください",
            never,
        );
        assert!(f.text_mismatch);
        assert!(f.host_not_in_message);
        assert_eq!(f.codes, 1);
    }

    #[test]
    fn a_code_to_the_domain_the_image_prints_is_not() {
        let urls = ["https://www.bank.example/app".to_string()];
        let f = assess(
            &urls,
            "bank.example",
            &[],
            "スマホで bank.example のアプリへ",
            never,
        );
        assert!(!f.text_mismatch);
        assert!(!f.host_not_in_message, "the sender's own domain");
    }

    #[test]
    fn a_code_to_a_linked_or_never_listed_host_is_in_the_message() {
        let urls = ["https://pay.shop.example/x".to_string()];
        let links = ["shop.example".to_string()];
        assert!(!assess(&urls, "mail.example", &links, "", never).host_not_in_message);
        let maps = ["https://maps.google.com/x".to_string()];
        assert!(!assess(&maps, "mail.example", &[], "", never).host_not_in_message);
    }

    #[test]
    fn text_without_a_domain_says_nothing_and_no_codes_say_nothing() {
        let urls = ["https://elsewhere.example/".to_string()];
        assert!(!assess(&urls, "a.example", &[], "請掃描二維碼 version 1.2", never).text_mismatch);
        assert_eq!(
            assess(&[], "a.example", &[], "x.example", never),
            QrFacts::default()
        );
    }
}
