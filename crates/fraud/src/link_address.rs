//! Links that carry an email address to a domain the message never
//! otherwise names.
//!
//! Phishing pages fill their fake sign-in form from the link: the reader's
//! address rides along, base64-encoded (`…/JjYrw7bGloYW9AZ29saWEuanA=…`)
//! or in the `#` part (`…/#lihao@golia.jp`), which the browser keeps from
//! the server and hands to the page's script. Genuine mail does the same
//! (a sign-in link to `claude.ai/magic-link#<base64>`), so the address is
//! not the evidence; where the link ends up is.
//!
//! Over 41,783 external production messages on 2026-11-12:
//!
//! - a base64 recipient address in a link that goes through one host to a
//!   final destination, both unrelated to every domain the message names:
//!   2, both credential phishing (`crockadoodle.com` and `satmagazine.com`
//!   forwarding to `smartechla.com`). Anthropic's 88 sign-in links go
//!   straight to `claude.ai`, which the message links elsewhere too;
//!   Slack, Binance, Gravatar and Zoom go to their own domains;
//! - any address in a link's `#` part, to a final destination unrelated
//!   to every domain the message names: 5, all credential phishing. An
//!   address in the path or query is everywhere in genuine mail
//!   (verification, invitations, invoices) and is not read.
//!
//! "Every domain the message names": the `From:`, `Sender:`,
//! `Return-Path:` and `Message-ID:` hosts and the DKIM signers (passed
//! in), the hosts of its images, and the hosts and destinations of its
//! other links, those that carry no address. A second copy of the same
//! button is not a second witness.
//!
//! The final destination: the host of the last `http://` or `https://`
//! written out in the link after its own host, read after undoing
//! percent-encoding twice (a `u=` or `url=` parameter, an `/away?url=`
//! path); the link's own host when there is none. A destination hidden
//! any other way (base64, a shortener) is not followed: the link's own
//! host stands for it, which only ever makes a match less likely when
//! that host is one the message names.

use crate::brand::registrable;

/// What the links say, for the rules: the registrable domain each kind
/// of link ends up at, or `None` when no link is of that kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkAddress {
    /// A link carries a recipient's address in base64 and passes through
    /// a host other than its destination, neither named by the message.
    pub base64_via_redirect: Option<String>,
    /// A link's `#` part carries an email address, and its destination is
    /// not named by the message.
    pub fragment_address: Option<String>,
}

/// How much of a link is read.
const LINK_LIMIT: usize = 4096;
/// Shortest base64 text that counts: a short address encodes to text
/// that turns up by chance in long tokens.
const B64_AT_LEAST: usize = 12;

/// Read `links` (every `href` that goes to the web) against `images` (the
/// hosts of the message's images), `named` (the hosts its headers name)
/// and `recipients` (its `To:` addresses).
#[must_use]
pub fn assess(
    links: &[&str],
    images: &[String],
    named: &[String],
    recipients: &[String],
) -> LinkAddress {
    let named: Vec<String> = named.iter().map(|h| registrable(h)).collect();
    let forms: Vec<String> = recipients.iter().flat_map(|a| base64_forms(a)).collect();
    let read: Vec<Link> = links
        .iter()
        .map(|l| Link::read(l, recipients, &forms))
        .collect();
    let mut elsewhere: Vec<String> = images.iter().map(|h| registrable(h)).collect();
    for l in read.iter().filter(|l| !l.carries) {
        elsewhere.push(l.own.clone());
        elsewhere.push(l.destination.clone());
    }
    let unrelated = |d: &str| !d.is_empty() && !named.iter().any(|n| n == d);
    let unseen = |d: &str| unrelated(d) && !elsewhere.iter().any(|e| e == d);
    let mut out = LinkAddress::default();
    for l in &read {
        if out.base64_via_redirect.is_none()
            && l.base64
            && l.own != l.destination
            && unrelated(&l.own)
            && unseen(&l.destination)
        {
            out.base64_via_redirect = Some(l.destination.clone());
        }
        if out.fragment_address.is_none() && l.fragment_address && unseen(&l.destination) {
            out.fragment_address = Some(l.destination.clone());
        }
    }
    out
}

struct Link {
    /// Registrable domain of the link's own host.
    own: String,
    /// Registrable domain of where it ends up.
    destination: String,
    base64: bool,
    fragment_address: bool,
    /// Carries a recipient in any form, or an address in its `#` part.
    carries: bool,
}

impl Link {
    fn read(url: &str, recipients: &[String], forms: &[String]) -> Link {
        let url = cut(url.trim());
        let (host, tail) = split(url);
        let decoded = percent_decode(&percent_decode(tail));
        let destination = last_embedded_host(&decoded).unwrap_or_else(|| host.to_string());
        let fragment = tail
            .split_once('#')
            .map(|(_, f)| percent_decode(f))
            .unwrap_or_default();
        let lower = decoded.to_ascii_lowercase();
        let plain = recipients
            .iter()
            .any(|r| !r.is_empty() && lower.contains(r.as_str()));
        let base64 = forms.iter().any(|f| tail.contains(f.as_str()));
        let fragment_address = has_address(&fragment);
        Link {
            own: registrable(host),
            destination: registrable(&destination),
            base64,
            fragment_address,
            carries: plain || base64 || fragment_address,
        }
    }
}

fn cut(s: &str) -> &str {
    if s.len() <= LINK_LIMIT {
        return s;
    }
    let mut end = LINK_LIMIT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `(host, everything after it)` of an `http(s)://` link; the host
/// lowercased, without credentials or port.
fn split(url: &str) -> (&str, &str) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    (host, &rest[end..])
}

/// The host of the last `http://` or `https://` in `s`, lowercased.
fn last_embedded_host(s: &str) -> Option<String> {
    let lower = s.to_ascii_lowercase();
    let at = ["http://", "https://"]
        .iter()
        .filter_map(|p| lower.rfind(p).map(|i| i + p.len()))
        .max()?;
    let host: String = lower[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        .collect();
    (host.contains('.')).then_some(host)
}

/// `%XX` undone once; anything malformed is left as it is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether `s` holds `local@domain.tld`.
fn has_address(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    chars.iter().enumerate().any(|(i, c)| {
        if *c != '@' {
            return false;
        }
        let local = chars[..i]
            .iter()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-'))
            .count();
        let domain: String = chars[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            .collect();
        let tld = domain
            .trim_end_matches('.')
            .rsplit_once('.')
            .map(|(_, t)| t);
        local > 0 && tld.is_some_and(|t| t.len() >= 2 && t.chars().all(|c| c.is_ascii_alphabetic()))
    })
}

/// The address base64-encoded the ways a link carries it: as written and
/// with its first letter capitalised, standard and URL-safe alphabets,
/// padding dropped.
fn base64_forms(address: &str) -> Vec<String> {
    let mut capital = address.to_string();
    if let Some(first) = capital.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    let mut out = Vec::new();
    for text in [address.to_string(), capital] {
        let standard = encode(text.as_bytes());
        let safe = standard.replace('+', "-").replace('/', "_");
        for f in [standard, safe] {
            if f.len() >= B64_AT_LEAST && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// Standard base64 without padding.
fn encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
#[path = "link_address/tests.rs"]
mod tests;
