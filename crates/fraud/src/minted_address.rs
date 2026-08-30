//! Addresses whose local part *and* domain were both generated.
//!
//! ```text
//! From: 齋藤 真 <omqqy@wzglff.com>          ギリア株式会社 要請事項
//! From: GOLIA株式会社 <vqkpsfdl@qjymtxcy.com>
//! From: AEON <osjnb@osjnb.ksxls.com>        【重要通知】本人認証サービス…
//! ```
//!
//! Neither half is a word in any language. `wzglff`, `qjymtxcy`,
//! `ksxls` — consonant runs of the length a random generator
//! produces, on both sides of the `@`.
//!
//! # Why the conjunction is the rule
//!
//! Either half alone is ordinary. Measured over 35,575 production
//! messages:
//!
//! | shape | messages | what they are |
//! |---|---|---|
//! | domain looks minted | 161 | **slack.com**, sentry.io, npmjs.com, letsencrypt.org |
//! | local part looks minted | 128 | bounce ids, ESP return paths |
//! | **both** | **11** | **11 phishing or BEC, no exceptions** |
//!
//! `slack` is five letters with one vowel, and there is no threshold
//! on consonant runs that keeps it out and lets `wzglff` in. What
//! separates them is that a real sender bought a name somebody has
//! to be able to say — so the *mailbox* on it is `no-reply`,
//! `bounces`, `notifications`. A generated mailbox on a generated
//! domain means nobody ever intended to be written back to.
//!
//! The eleven: six are one BEC campaign impersonating this company's
//! own name and a manager's, five are Japanese brand phishing —
//! SAISON, AEON, a delivery notice — with zero-width characters
//! sprinkled through their subjects.
//!
//! # Not the same reading as [`super::sending_host`]
//!
//! That one wants a **digit** and looks at the label in front of the
//! registrable domain — `l2n4p6r9.zh-hub-kysports.com`, a throwaway
//! subdomain under a domain somebody owns. This one looks at the
//! registered domain itself and wants no digits at all. Two shapes,
//! two rules; folding them together would loosen both.

/// Whether a label reads as machine-minted: all letters, long
/// enough to be a word, and with too few vowels to be one.
fn looks_minted(label: &str) -> bool {
    if !(5..=12).contains(&label.len()) {
        return false;
    }
    if !label.chars().all(|c| c.is_ascii_lowercase()) {
        return false;
    }
    let vowels = label.chars().filter(|c| "aeiou".contains(*c)).count();
    // One vowel in five is `slack`; the ratio is what admits it, and
    // the conjunction below is what excludes it again.
    (vowels as f64) / (label.len() as f64) <= 0.2
}

/// Whether both halves of an address were generated.
///
/// `host` is the full sending host; the registered domain is taken
/// from it, so `osjnb@osjnb.ksxls.com` is judged on `ksxls` rather
/// than on the throwaway `osjnb.` in front of it.
#[must_use]
pub fn address_looks_minted(from: &str) -> bool {
    let addr = match (from.rfind('<'), from.rfind('>')) {
        (Some(a), Some(b)) if a < b => &from[a + 1..b],
        _ => from.trim(),
    };
    let lowered = addr.trim().to_ascii_lowercase();
    let Some((local, host)) = lowered.rsplit_once('@') else {
        return false;
    };
    let registrable = crate::brand::registrable(host.trim_end_matches('.'));
    let Some(sld) = registrable.split('.').next() else {
        return false;
    };
    looks_minted(local.trim()) && looks_minted(sld)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All eleven from the corpus, in the two forms they arrive in.
    #[test]
    fn both_halves_generated_is_recognised() {
        for from in [
            "齋藤 真 <omqqy@wzglff.com>",
            "GOLIA株式会社 <vqkpsfdl@qjymtxcy.com>",
            "齋藤 真 <qclwx@mhsfwf.com>",
            "<etkrytn@tylfjs.com>",
            "cssem@wzglff.com",
            "onfknp@mhsfwf.com",
            "AEON <osjnb@osjnb.ksxls.com>",
            "<bgqzb@bgqzb.gzfxn.com>",
            "<wspquf@wspquf.smcxgj.com>",
            "<church@church.ffqpsq.com>",
            "<funny@funny.lcrwa.com>",
        ] {
            assert!(address_looks_minted(from), "missed: {from}");
        }
    }

    /// The 161 domains that look minted on their own. Every one of
    /// these is real mail somebody wanted, and the conjunction is
    /// the only thing standing between them and a hold.
    #[test]
    fn a_terse_domain_with_a_real_mailbox_is_ordinary() {
        for from in [
            "Slack <no-reply@slack.com>",
            "Sentry <noreply@md.sentry.io>",
            "npm <support@npmjs.com>",
            "Let's Encrypt <expiry@letsencrypt.org>",
            "Typst <hello@typst.app>",
        ] {
            assert!(!address_looks_minted(from), "wrongly caught: {from}");
        }
    }

    /// And the 128 whose mailbox is a generated id — ESP bounce
    /// addresses, which say nothing about the sender.
    #[test]
    fn a_generated_mailbox_on_a_real_domain_is_ordinary() {
        for from in [
            "<bncbdhjqkm@googlegroups.com>",
            "<srsvzqx@bounce.linkedin.com>",
            "Amazon <shipment-tracking@amazon.co.jp>",
        ] {
            assert!(!address_looks_minted(from), "wrongly caught: {from}");
        }
    }

    #[test]
    fn nothing_to_read_is_not_a_finding() {
        for from in ["", "no-at-sign", "@", "a@", "@b.com"] {
            assert!(!address_looks_minted(from));
        }
    }
}
