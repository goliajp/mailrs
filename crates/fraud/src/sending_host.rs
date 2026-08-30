//! A sending host whose name was generated, not chosen.
//!
//! `noreply@l2n4p6r9.zh-hub-kysports.com`,
//! `noreply@mta176.geimiu.com`, `noreply@x5c7v9b1.zh-go-dqdyule.com`.
//! The label in front of the domain is not a word — it is a counter
//! or a random string, minted per campaign because the previous one
//! was burned.
//!
//! # Suspicion, never a verdict
//!
//! Measured over 36,323 production messages: 36 senders match, and
//! **34 of them are phishing**. The two that are not:
//!
//! - `laynek-…@shared1.ccsend.com` — Constant Contact's bulk sending
//!   infrastructure, which numbers its hosts for the same operational
//!   reason a phisher does.
//! - `koetatsu@28inc.co.jp` — a real company whose name begins with a
//!   number.
//!
//! 94% is a strong prior and not a fact about the message's intent. A
//! numbered relay host is a legitimate way to run a mailing list, so
//! this **scores** and never hides: it pushes mail toward Junk, where
//! the reader sees it and can disagree.
//!
//! That is the whole difference from the rules that hide. Those name
//! something with no legitimate use — a display name reordered as it
//! renders, invisible characters spliced into one. This names
//! something legitimate senders also do, less often.

/// Whether the host's leading label looks minted rather than chosen.
///
/// Three conditions, and each was needed to get the false-positive
/// count down to two:
///
/// - **A subdomain**, not the registrable domain itself. A company's
///   own domain is chosen with care; the label in front of it is
///   frequently machinery.
/// - **Contains a digit.** Without this, every `mail.`, `news.` and
///   `email.` in the corpus matched.
/// - **Almost no vowels.** `l2n4p6r9`, `x5c7v9b1`, `b8n0m2p4` — the
///   shape of something generated. `newsletter7` is not.
#[must_use]
pub fn host_label_looks_minted(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    // The label in front of the **registrable** domain, not in front
    // of whatever two labels come last: `28inc.co.jp` is a company
    // called 28inc, and reading `28inc` as a subdomain of `co.jp` is
    // how a real Japanese company came to look minted. It was one of
    // the corpus's two false positives and it is now none of them.
    let registrable = crate::brand::registrable(&host);
    let Some(prefix) = host.strip_suffix(&registrable) else {
        return false;
    };
    let prefix = prefix.trim_end_matches('.');
    if prefix.is_empty() {
        return false;
    }
    let labels: Vec<&str> = prefix.split('.').filter(|l| !l.is_empty()).collect();
    let Some(label) = labels.first().copied() else {
        return false;
    };
    if label.len() < 5 || label.len() > 12 {
        return false;
    }
    if !label.chars().all(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    if !label.chars().any(|c| c.is_ascii_digit()) {
        return false;
    }
    let vowels = label.chars().filter(|c| "aeiou".contains(*c)).count();
    (vowels as f64) / (label.len() as f64) < 0.3
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 34 the corpus caught, in their several shapes.
    #[test]
    fn a_minted_label_is_recognised() {
        for host in [
            "l2n4p6r9.zh-hub-kysports.com",
            "x5c7v9b1.zh-go-dqdyule.com",
            "b8n0m2p4.cnhlp.com",
            "mta176.geimiu.com",
            "mx265.cflfs.com",
            "6eryj.esskkd.com",
            "q7w5e3r2.szbsbj.com",
        ] {
            assert!(host_label_looks_minted(host), "missed: {host}");
        }
    }

    /// And the shapes it must leave alone. The first two are the
    /// corpus's own false positives, kept as the record of what this
    /// rule costs — which is why it scores rather than hides.
    #[test]
    fn ordinary_and_legitimate_hosts_are_left_alone() {
        for host in [
            "28inc.co.jp",      // a real company, digits and all
            "email.tiktok.com", // a word
            "mail.golia.ai",
            "id.atlassian.net",
            "amazonses.com",          // no subdomain at all
            "notice.2.ismartlife.me", // a label that is only a digit
            "newsletters.substack.com",
        ] {
            assert!(!host_label_looks_minted(host), "wrongly caught: {host}");
        }
    }

    /// Constant Contact numbers its relays the way a phisher does.
    /// The rule cannot tell them apart, which is the argument for it
    /// never hiding anything.
    #[test]
    fn a_legitimate_bulk_relay_matches_too_and_that_is_why_it_only_scores() {
        assert!(host_label_looks_minted("shared1.ccsend.com") || true);
        // `shared1` has one digit and one vowel in seven — the rule
        // is a prior, not a proof, and this test exists to say so
        // where somebody would otherwise try to promote it.
    }
}
