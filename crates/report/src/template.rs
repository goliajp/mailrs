//! When two messages are copies of one template.
//!
//! A close body alone is not enough: a bank's genuine notice and a scam
//! copying its wording read alike. Copies of one campaign also share where
//! they send the reader or what they carry, so a match needs the bodies at
//! most [`MAX_DISTANCE`] bits apart and one of: a link domain in common, an
//! attachment in common, or the same sender domain.

use serde::{Deserialize, Serialize};

/// The widest SimHash distance still counted as one template.
pub const MAX_DISTANCE: u32 = 3;

/// What a message is compared on. Derived from the message, so it can be
/// computed again from the stored file at any time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub simhash: u64,
    /// Registrable domains of the hosts its links point to.
    pub link_domains: Vec<String>,
    /// SHA-256 of each attachment, lowercase hex.
    pub attachments: Vec<String>,
    /// Registrable domain of the `From:` address.
    pub sender_domain: String,
}

/// What made two messages one template, beside a close body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shared {
    LinkDomain,
    Attachment,
    SenderDomain,
}

/// Whether `a` and `b` are one template, and on what besides the body.
pub fn same(a: &Template, b: &Template) -> Option<Shared> {
    if crate::fingerprint::distance(a.simhash, b.simhash) > MAX_DISTANCE {
        return None;
    }
    let meets = |x: &[String], y: &[String]| x.iter().any(|v| !v.is_empty() && y.contains(v));
    if meets(&a.link_domains, &b.link_domains) {
        return Some(Shared::LinkDomain);
    }
    if meets(&a.attachments, &b.attachments) {
        return Some(Shared::Attachment);
    }
    if !a.sender_domain.is_empty() && a.sender_domain == b.sender_domain {
        return Some(Shared::SenderDomain);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u64, links: &[&str], files: &[&str], sender: &str) -> Template {
        Template {
            simhash: h,
            link_domains: links.iter().map(|s| s.to_string()).collect(),
            attachments: files.iter().map(|s| s.to_string()).collect(),
            sender_domain: sender.to_string(),
        }
    }

    #[test]
    fn a_close_body_needs_something_else_in_common() {
        let a = t(0b1011, &["bank.example"], &[], "a.example");
        let b = t(0b1010, &["other.example"], &[], "b.example");
        assert_eq!(same(&a, &b), None);
    }

    #[test]
    fn each_shared_fact_is_enough_with_a_close_body() {
        let a = t(0, &["evil.example"], &["f1"], "s.example");
        assert_eq!(
            same(&a, &t(0b111, &["evil.example"], &[], "x.example")),
            Some(Shared::LinkDomain)
        );
        assert_eq!(
            same(&a, &t(0b111, &[], &["f1"], "x.example")),
            Some(Shared::Attachment)
        );
        assert_eq!(
            same(&a, &t(0b111, &[], &[], "s.example")),
            Some(Shared::SenderDomain)
        );
    }

    #[test]
    fn four_bits_apart_is_another_template() {
        let a = t(0, &["evil.example"], &[], "s.example");
        assert_eq!(
            same(&a, &t(0b1111, &["evil.example"], &[], "s.example")),
            None
        );
    }

    #[test]
    fn empty_facts_are_not_shared() {
        let a = t(0, &[""], &[""], "");
        assert_eq!(same(&a, &t(0, &[""], &[""], "")), None);
    }
}
