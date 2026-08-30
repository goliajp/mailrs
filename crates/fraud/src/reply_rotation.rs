//! Senders that change domain while the replies keep going one place.
//!
//! ```text
//! From: 八字精批 <marketing@shiye.airmessage.cn>
//! From: 生辰命运 <yunshi@linghit.qianjingdd.cn>
//! From: 命运解读 <marketing@ziwei.orpseon.cn>
//! Reply-To: suqiqi@linghit.com                      ← all three
//! ```
//!
//! Eighteen sending hosts across six registered domains, every one
//! pointing replies at a single mailbox on a seventh. The domains are
//! disposable and the mailbox is the business; when one domain is
//! blocked the next one is already sending.
//!
//! # Why this is a characteristic and not a suspicion
//!
//! Ordinary bulk mail is the other way round. A company sends from
//! `freee.work` and takes replies at `freee.co.jp` — two names, one
//! organisation, and it stays that way for years. Qiita sends from
//! `qiita.com` and replies land at Mailchimp. **One sending domain
//! per reply address** is what legitimate senders look like, because
//! a domain is something they own and advertise rather than something
//! they consume.
//!
//! Rotating registrable domains is not a thing a sender does by
//! accident, and it is not a thing about *us* — it is a measurable
//! property of the sender's own behaviour, visible in the mail
//! itself. That is the distinction that matters: whether *we* find a
//! sender familiar may not decide anything, but how many disposable
//! domains the sender burns is theirs.
//!
//! # Measured
//!
//! Over 36,318 production messages, grouping every off-domain
//! `Reply-To` by the registrable domains that send to it:
//!
//! | distinct sending domains | reply addresses | what they are |
//! |---|---|---|
//! | 1 | 257 | freee, Qiita, Amazon, Nitori, Qualcomm — all ordinary |
//! | 2 | 8 | Apple, mingdao — one company, two names |
//! | 3 | 1 | **JSWorld** — one organiser, three conferences. Legitimate. |
//! | 6 | 2 | 179 fortune-telling, 61 `*age.com` — both campaigns |
//! | 8 | 1 | 18 messages, hotmail/outlook/`mhsfwf.com` — a campaign |
//!
//! [`ROTATION_THRESHOLD`] sits above the JSWorld case and below the
//! campaigns: **258 messages held, nothing legitimate touched.**
//!
//! # Why it has to run again over old mail
//!
//! The fourth domain is what convicts, and by then the first three
//! have already been delivered. A rule that only ever looked at
//! arriving mail would catch the tail of a campaign and leave its
//! head in the inbox — which is the whole reason detection runs
//! asynchronously over history rather than only at the door.

/// How many registrable sending domains must funnel into one
/// off-domain reply address before that is a characteristic.
///
/// Four. Three is a conference organiser; see the table above.
pub const ROTATION_THRESHOLD: u32 = 4;

/// Whether a reply address has collected enough sending domains.
///
/// `domains` is the number of **distinct registrable** domains seen
/// sending to this reply address — hosts collapse to their domain
/// because the campaign rotates hosts too (`shiye.airmessage.cn`,
/// `huodong.airmessage.cn`), and counting hosts would convict the
/// per-campaign subdomains legitimate senders use.
#[must_use]
pub fn rotates(domains: u32) -> bool {
    domains >= ROTATION_THRESHOLD
}

/// The key under which a reply address's sending domains are kept.
#[must_use]
pub fn rotation_key(reply_addr: &str) -> String {
    format!("mailrs:replyrot:{}", reply_addr.trim().to_ascii_lowercase())
}

/// Whether a reply address is somewhere other than the sender's own
/// estate, and therefore worth counting at all.
///
/// Same registrable domain means the reply goes home, which is what
/// ordinary mail does and says nothing. This is the filter that keeps
/// the count over 36,318 messages down to 257 reply addresses instead
/// of every sender in the corpus.
#[must_use]
pub fn is_off_domain(from_host: &str, reply_addr: &str) -> bool {
    // Split on the last `@` and require there to have been one: a
    // bare word is not an address, and `rsplit` would hand back the
    // whole of it as though it were the host.
    let Some((_, reply_host)) = reply_addr.rsplit_once('@') else {
        return false;
    };
    if reply_host.is_empty() || from_host.is_empty() {
        return false;
    }
    let a = crate::brand::registrable(from_host);
    let b = crate::brand::registrable(reply_host);
    !a.is_empty() && !b.is_empty() && a != b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_threshold_sits_between_the_conference_and_the_campaigns() {
        assert!(!rotates(3), "JSWorld runs three conferences and is legitimate");
        assert!(rotates(4));
        assert!(rotates(6), "the fortune-telling campaign");
        assert!(rotates(8));
    }

    /// A reply address inside the sender's own estate is ordinary and
    /// is never counted — this is what keeps the population small.
    #[test]
    fn replying_to_yourself_is_not_rotation() {
        assert!(!is_off_domain("mail.qiita.com", "noreply@qiita.com"));
        assert!(!is_off_domain("shiye.airmessage.cn", "x@airmessage.cn"));
    }

    #[test]
    fn a_reply_somewhere_else_is_what_gets_counted() {
        assert!(is_off_domain("shiye.airmessage.cn", "suqiqi@linghit.com"));
        assert!(is_off_domain("freee.work", "noreply@freee.co.jp"));
    }

    #[test]
    fn nothing_to_read_counts_as_nothing() {
        assert!(!is_off_domain("", "a@b.com"));
        assert!(!is_off_domain("a.com", ""));
        assert!(!is_off_domain("a.com", "no-at-sign"));
    }

    /// The key is what a sweep and the live path must agree on, so
    /// case and stray space cannot make two of them.
    #[test]
    fn one_reply_address_is_one_key() {
        assert_eq!(rotation_key(" SuQiQi@Linghit.COM "), rotation_key("suqiqi@linghit.com"));
    }
}
