//! Someone claiming to be a company you have an account with.
//!
//! The sibling of [`impersonation`](crate::impersonation), which
//! catches a sender claiming to be *your own* organisation. This one
//! catches the far commoner case: `iCloud+ <zkxfp@zkxfp.zctxiot.com>`
//! asking you to confirm a payment method, `Amazon.co.jp 配送について`
//! from `mail07.jqjintaiyang.com`, `AEON` from `6eryj.esskkd.com`.
//!
//! # The naive rule is a disaster, again, and the corpus says so
//!
//! "Display name contains a brand, address is not that brand's
//! domain" matches 32 distinct senders in a 35,962-message corpus. Of
//! those, twelve are entirely legitimate:
//!
//! | sender | why it is fine |
//! |---|---|
//! | `Amazon Web Services <…@aws.com>` | Amazon's own other domain |
//! | `Amazon通过领英发送 <…@linkedin.com>` | a recruiter's InMail |
//! | `Apple RING E3 ligase MdMIEL1 inhibits…` | a paper about the **fruit** |
//! | `SMBCコンシューマーファイナンス/アメブロ <…@ameba.jp>` | a newsletter naming its advertiser |
//! | `Microsoft Rewards <…@microsoftrewards.com>` | Microsoft's own other domain |
//!
//! A rule that hid those would be worse than no rule.
//!
//! # What separates them, measured
//!
//! Sorted by how many of the 35,962 messages came from the sender's
//! registrable domain, the corpus splits without a single crossing:
//!
//! | messages ever seen from that domain | senders | verdict |
//! |---|---|---|
//! | 1–2 | 20 | **every one a phish** |
//! | ≥ 3 | 12 | **every one legitimate** |
//!
//! That is not a coincidence, and [`impersonation`]'s own notes say
//! why: *"the domains rotate — `auto360d.com`, `mhsfwf.com`,
//! `tylfjs.com` — and the local part is fresh random letters each
//! time."* A domain that rotates is, by construction, a domain this
//! mailbox has never seen. The brands do not rotate, because the
//! claim is the whole point of the mail; the domains must, because
//! they are burned as soon as they are reported.
//!
//! So the check is the pair: **a brand claim from a domain with no
//! history here**. Neither half convicts alone, and the corpus is
//! what says where the line goes.

/// Messages ever seen from a domain, at or above which a brand claim
/// is treated as ordinary mail.
///
/// Three, from the split above. The twelve legitimate senders sit at
/// 3, 3, 4, 10, 16, 30, 30, 30, 41, 41, 893 and 1414; the twenty
/// phishing ones sit at 1 and 2. There is room on both sides, and the
/// number is deliberately at the low end of it — a legitimate sender
/// held on their first message is one release click, and a phish let
/// through is a credential.
pub const FAMILIAR_AFTER: u64 = 3;

/// Score for a brand claim from a domain with no history.
///
/// Same weight as [`CLAIMS_OUR_NAME_SCORE`](crate::impersonation::CLAIMS_OUR_NAME_SCORE)
/// and for the same reason: authentication has nothing to say about
/// it. `zkxfp@zkxfp.zctxiot.com` owns its domain and can sign for it.
pub const IMPERSONATES_BRAND_SCORE: f64 = 4.5;

/// One company somebody might be impersonated as.
#[derive(Debug, Clone)]
pub struct Brand {
    /// What the display name would have to say. Compared folded —
    /// case, whitespace and full-width forms are all normalised, so
    /// `ＡＭＡＺＯＮ` and `A m a z o n` are the same claim.
    pub name: &'static str,
    /// The domains that really are this company. A suffix match, so
    /// `email.tiktok.com` matches `tiktok.com`.
    pub domains: &'static [&'static str],
}

/// The companies this deployment has actually seen impersonated,
/// plus the ones every phishing kit ships with.
///
/// Deliberately short. Every entry is a chance to hide somebody's
/// real mail, and the corpus above shows how easily a common word
/// does that — `apple` alone matched three papers about apples. A
/// name earns its place by being a company whose mail asks for money
/// or credentials, not by being well known.
pub const BRANDS: &[Brand] = &[
    Brand {
        name: "icloud",
        domains: &["apple.com", "icloud.com", "me.com"],
    },
    Brand {
        name: "apple",
        domains: &["apple.com", "icloud.com", "itunes.com", "me.com"],
    },
    Brand {
        name: "amazon",
        domains: &[
            "amazon.com",
            "amazon.co.jp",
            "amazon.jp",
            "amazonses.com",
            "amazonaws.com",
            "aws.com",
        ],
    },
    Brand {
        name: "paypal",
        domains: &["paypal.com", "paypal.co.jp"],
    },
    Brand {
        name: "netflix",
        domains: &["netflix.com"],
    },
    Brand {
        name: "aeon",
        domains: &["aeon.co.jp", "aeonbank.co.jp", "aeoncard.co.jp"],
    },
    Brand {
        name: "mufg",
        domains: &["mufg.jp", "bk.mufg.jp"],
    },
    Brand {
        name: "smbc",
        domains: &["smbc.co.jp", "smbc-card.com"],
    },
    Brand {
        name: "jcb",
        domains: &["jcb.co.jp"],
    },
    Brand {
        name: "rakuten",
        domains: &["rakuten.co.jp", "rakuten.com", "rakuten-card.co.jp"],
    },
    Brand {
        name: "etc利用照会",
        domains: &["etc-meisai.jp"],
    },
    Brand {
        name: "sagawa",
        domains: &["sagawa-exp.co.jp"],
    },
    Brand {
        name: "ヤマト運輸",
        domains: &["kuronekoyamato.co.jp"],
    },
];

/// Whether `from` claims to be one of `brands` from a domain that is
/// neither theirs nor familiar here.
///
/// `domain_seen` is how many messages this deployment has ever had
/// from the sender's registrable domain, **not counting this one**.
/// The caller owns that count; this crate does no I/O.
///
/// `from` is the decoded `From:` value. Undecoded input is the way to
/// make this always answer false — the names arrive base64'd.
#[must_use]
pub fn impersonates_brand(from: &str, brands: &[Brand], domain_seen: u64) -> bool {
    if domain_seen >= FAMILIAR_AFTER {
        return false;
    }
    let Some(address) = crate::impersonation::address_of(from) else {
        return false;
    };
    let Some(domain) = address.rsplit('@').next() else {
        return false;
    };
    let domain = domain.trim().trim_end_matches('>').to_ascii_lowercase();
    if domain.is_empty() {
        return false;
    }
    let display = crate::impersonation::display_name_of(from);
    if display.is_empty() {
        return false;
    }
    let folded = crate::impersonation::fold(&display);
    brands.iter().any(|b| {
        folded.contains(&crate::impersonation::fold(b.name))
            && !b
                .domains
                .iter()
                .any(|d| domain == *d || domain.ends_with(&format!(".{d}")))
    })
}

/// The registrable domain of a host: `mail02.marriottanji.com` →
/// `marriottanji.com`, `email.tiktok.com` → `tiktok.com`.
///
/// Here rather than beside the counter that uses it, because three
/// processes need the same answer — the receiver deciding, the drain
/// counting, and the re-scan re-deciding — and three copies of a
/// suffix rule is three chances to disagree about whether
/// `smbc.co.jp` is one domain or two.
///
/// Handles the Japanese second-level suffixes this deployment
/// receives (`co.jp`, `or.jp`, `ne.jp`, `ac.jp`, `go.jp`); without
/// them `smbc.co.jp` would reduce to `co.jp` and every Japanese
/// sender would share one counter. Not a full public-suffix list —
/// that is a data file that goes stale, and a suffix this does not
/// know makes a domain look *less* familiar, which holds mail rather
/// than delivering it.
#[must_use]
pub fn registrable(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = host.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 3 {
        let second = parts[parts.len() - 2];
        if parts[parts.len() - 1] == "jp" && matches!(second, "co" | "or" | "ne" | "ac" | "go") {
            return parts[parts.len() - 3..].join(".");
        }
    }
    if parts.len() >= 2 {
        return parts[parts.len() - 2..].join(".");
    }
    host
}

/// Where one domain's message count is stored.
///
/// The key name lives with the rule that reads it so the writer and
/// the readers cannot spell it differently — the failure this
/// repository has already had twice, once with an outbound queue key
/// and once with a fraud verdict.
#[must_use]
pub fn seen_key(registrable_domain: &str) -> String {
    format!("mailrs:domseen:{registrable_domain}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every one of these arrived. The count beside each is how many
    /// messages the corpus had from that registrable domain.
    #[test]
    fn the_twenty_phishing_senders_are_caught() {
        for (from, seen) in [
            ("iCloud+ <zkxfp@zkxfp.zctxiot.com>", 0),
            ("AEON <a@l2n4p6r9.bbmghome.com>", 1),
            ("AEON <a@osjnb.ksxls.com>", 1),
            ("Amazon 配送センター <a@mail02.marriottanji.com>", 1),
            ("Amazon.co.jp デリバリー <a@mail28.aochengkaihao.com>", 1),
            ("Amazon.co.jp 配送について <a@mail01.zjw-flyway.com>", 1),
            (
                "【Amazon】配送状況のお知らせ <a@mail24.lingtuoshiye.com>",
                1,
            ),
            (
                "【自動送信】Amazonカスタマーサービス <a@oxxyib.hycdkj.com>",
                1,
            ),
            ("Apple\u{a0}請求書 <a@amezlink-jp.com>", 2),
            ("ETC利用照会サービス事務局 <a@dlsite.com>", 1),
        ] {
            assert!(impersonates_brand(from, BRANDS, seen), "not caught: {from}");
        }
    }

    /// Zero-width characters inserted between the letters of the
    /// brand. Six of the twenty carried them — they are there to beat
    /// exactly this comparison, and `fold` removes them along with
    /// the spaces and the full-width forms.
    #[test]
    fn zero_width_padding_does_not_hide_the_claim() {
        for from in [
            "Amazon\u{200d}.co.jp (自動\u{200b}送\u{200c}信メー\u{feff}ル\u{2060}) <a@top.shop-hupu.com>",
            "Amazon.co.jp デ\u{200c}リバリ\u{200d}ー <a@talk.yueshangjiaju.com>",
            "AEON\u{200d} <a@6eryj.esskkd.com>",
        ] {
            assert!(impersonates_brand(from, BRANDS, 1), "not caught: {from}");
        }
    }

    /// And every one of these is real mail somebody wanted. A rule
    /// that hid them would be worse than no rule at all — which is
    /// what the first draft of it did.
    #[test]
    fn the_twelve_legitimate_senders_are_left_alone() {
        for (from, seen) in [
            // The brand's own other domains — no history needed.
            ("Amazon Web Services <a@amazonaws.com>", 0),
            ("Amazon Web Services <a@aws.com>", 0),
            ("iCloud+ <noreply@icloud.com>", 0),
            ("TikTok Shop Partner Center <partner@email.tiktok.com>", 0),
            // Familiar domains: the mailbox has a history with them.
            ("Amazon通过领英发送 <a@linkedin.com>", 893),
            (
                "Apple RING E3 ligase MdMIEL1 inhibits anthocyanin <a@academia-mail.com>",
                30,
            ),
            ("SMBCコンシューマーファイナンス/アメブロ <a@ameba.jp>", 41),
            (
                "Microsoft Rewards <a@customeremail.microsoftrewards.com>",
                3,
            ),
            ("Netflix <a@golia.jp>", 1414),
        ] {
            assert!(
                !impersonates_brand(from, BRANDS, seen),
                "wrongly caught: {from}"
            );
        }
    }

    /// The two halves are a pair. Neither convicts alone, and the
    /// test says so in both directions — otherwise the familiarity
    /// count could be dropped and nothing would fail.
    #[test]
    fn neither_half_convicts_alone() {
        // A brand claim, but the domain is familiar.
        assert!(!impersonates_brand(
            "Amazon <a@known.example>",
            BRANDS,
            FAMILIAR_AFTER
        ));
        // An unknown domain, but no brand claim.
        assert!(!impersonates_brand(
            "Some Person <a@brand-new.example>",
            BRANDS,
            0
        ));
        // Both.
        assert!(impersonates_brand(
            "Amazon <a@brand-new.example>",
            BRANDS,
            0
        ));
    }

    /// A display name with no name at all cannot make a claim.
    #[test]
    fn a_bare_address_claims_nothing() {
        assert!(!impersonates_brand("<a@brand-new.example>", BRANDS, 0));
        assert!(!impersonates_brand("a@brand-new.example", BRANDS, 0));
    }

    #[test]
    fn a_host_reduces_to_the_domain_that_was_registered() {
        assert_eq!(registrable("mail02.marriottanji.com"), "marriottanji.com");
        assert_eq!(registrable("email.tiktok.com"), "tiktok.com");
        assert_eq!(registrable("tiktok.com"), "tiktok.com");
        assert_eq!(registrable("A.B.Example.COM."), "example.com");
    }

    /// Without the `co.jp` case every Japanese sender would share one
    /// counter and look permanently familiar.
    #[test]
    fn japanese_second_level_suffixes_are_not_the_registrable_domain() {
        assert_eq!(registrable("www.smbc.co.jp"), "smbc.co.jp");
        assert_eq!(registrable("a.b.example.or.jp"), "example.or.jp");
        assert_eq!(registrable("mail.mufg.jp"), "mufg.jp");
    }

    #[test]
    fn nothing_sensible_reduces_to_nothing_surprising() {
        assert_eq!(registrable(""), "");
        assert_eq!(registrable("localhost"), "localhost");
    }
}
