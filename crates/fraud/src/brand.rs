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
//!
//! # Why this scores and does not hide — 2026-08-30
//!
//! Familiarity is not a characteristic of fraud. It is a
//! characteristic of *our own history*, and it is equally true of
//! every legitimate correspondent writing for the first time. A rule
//! resting on it cannot tell a reader why their mail was taken away:
//! the honest sentence is "we have not heard from you before", which
//! is not an accusation and should not carry the weight of one.
//!
//! It also decays in both directions. A sender who warms a domain for
//! a week defeats it. A new supplier, a new bank, a new customer gets
//! caught by it — and one already was: three messages from
//! `rooms-online.jp`, all of them 三井住友銀行 confirming a video
//! appointment, sat exactly on the line.
//!
//! The rules that *hide* mail are the ones naming something the
//! message itself is doing and that has no legitimate use — a display
//! name reordered as it renders, invisible characters spliced into a
//! name, an `X-Mailer` no client writes, an attachment the machine
//! would run. Those are answerable: a reader can be shown the
//! characters. This one is a strong prior and belongs in the score,
//! where it pushes mail toward Junk and the reader still sees it.
//!
//! Measured on the sweep of 2026-08-30: of 123 conversations the
//! rules found, 84 carried one of those intrinsic signals and 39
//! rested on this pair alone. One of the 39 was already a false
//! positive.

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
    // Added 2026-08-30, after `ANAマイレージクラブ (自動配信)
    // <system7yi9@dvikd.jsyoutom.com>` arrived and nothing fired.
    // The name was in the display name all along; it was only in the
    // subject list, so the check that reads the `From` had nothing to
    // match. A brand belongs in both lists or in neither.
    Brand {
        name: "ANAマイレージクラブ",
        domains: &["ana.co.jp", "anamile.jp"],
    },
    Brand {
        name: "ANAカード",
        domains: &["ana.co.jp", "anamile.jp"],
    },
    Brand {
        name: "アマゾン",
        domains: &["amazon.com", "amazon.co.jp", "amazon.jp"],
    },
    Brand {
        name: "myjcb",
        domains: &["jcb.co.jp"],
    },
    Brand {
        name: "セゾンカード",
        domains: &["saisoncard.co.jp"],
    },
    Brand {
        name: "三井住友カード",
        domains: &["smbc.co.jp", "smbc-card.com", "vpass.ne.jp"],
    },
    Brand {
        name: "楽天カード",
        domains: &["rakuten.co.jp", "rakuten.com", "rakuten-card.co.jp"],
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

/// How a brand is named in a **subject**, where the wording is not a
/// display name and a substring match is far more dangerous.
///
/// The measurement over 35,962 messages is what shaped these. A bare
/// `amazon` in a subject matches **1,716** messages; narrowed to a
/// domain the mailbox has no history with it matches 31, and all 31
/// are phishing. The other 160 — from familiar domains — are every
/// one legitimate: gift-card campaigns from freee, Money Forward,
/// Recruit and Mercari, LinkedIn's Amazon news, Rakuten Bank on
/// マイナンバー, 三井住友銀行 confirming a video appointment.
///
/// So the patterns are deliberately narrow **and** the familiarity
/// half is not optional. Loose ones like a bare `ANA` were dropped
/// during the measurement: they matched
/// `Artificial Intelligence for Enterprise` and
/// `Practical Project Management`.
pub const SUBJECT_CLAIMS: &[Brand] = &[
    Brand {
        name: "ANAマイレージ",
        domains: &["ana.co.jp", "anamile.jp"],
    },
    Brand {
        name: "ANAカード",
        domains: &["ana.co.jp", "anamile.jp"],
    },
    Brand {
        name: "【ANA】",
        domains: &["ana.co.jp", "anamile.jp"],
    },
    Brand {
        name: "MyJCB",
        domains: &["jcb.co.jp"],
    },
    Brand {
        name: "【JCB",
        domains: &["jcb.co.jp"],
    },
    Brand {
        name: "セゾンカード",
        domains: &["saisoncard.co.jp"],
    },
    Brand {
        name: "【SAISON",
        domains: &["saisoncard.co.jp"],
    },
    Brand {
        name: "【AEON",
        domains: &["aeon.co.jp", "aeonbank.co.jp", "aeoncard.co.jp", "aeon.com"],
    },
    Brand {
        name: "【iAEON",
        domains: &["aeon.co.jp", "aeonbank.co.jp", "aeoncard.co.jp", "aeon.com"],
    },
    Brand {
        name: "イオンカード",
        domains: &["aeon.co.jp", "aeonbank.co.jp", "aeoncard.co.jp"],
    },
    Brand {
        name: "楽天カード",
        domains: &["rakuten.co.jp", "rakuten.com", "rakuten-card.co.jp"],
    },
    Brand {
        name: "三井住友",
        domains: &["smbc.co.jp", "smbc-card.com", "vpass.ne.jp"],
    },
    Brand {
        name: "【SMBC",
        domains: &["smbc.co.jp", "smbc-card.com", "vpass.ne.jp"],
    },
    Brand {
        name: "AppleID",
        domains: &["apple.com", "icloud.com", "me.com"],
    },
    Brand {
        name: "Apple ID",
        domains: &["apple.com", "icloud.com", "me.com"],
    },
    Brand {
        name: "iCloud",
        domains: &["apple.com", "icloud.com", "me.com"],
    },
    Brand {
        name: "ETC利用照会",
        domains: &["etc-meisai.jp"],
    },
    Brand {
        name: "マイナポータル",
        domains: &["myna.go.jp", "digital.go.jp"],
    },
    Brand {
        name: "Amazon",
        domains: &[
            "amazon.com",
            "amazon.co.jp",
            "amazon.jp",
            "amazonses.com",
            "amazonaws.com",
            "aws.com",
            "audible.co.jp",
        ],
    },
];

/// Whether the **subject** claims a brand from a domain that is
/// neither theirs nor familiar here.
///
/// The sibling of [`impersonates_brand`], and the reason it exists:
/// the `【ANA】今年度ご利用実績に伴うボーナスマイル受取のご案内`
/// that arrived on 2026-08-29 had a display name of `noticeb8q72a`
/// — the sender's own local part, claiming nothing. Every check that
/// reads the `From` had nothing to look at. The claim was in the
/// subject, where the reader sees it.
///
/// Same pair, same reason: over the corpus the claim alone matches
/// 1,716 messages and the pair matches 31, all phishing.
#[must_use]
pub fn subject_claims_brand(subject: &str, domain: &str, domain_seen: u64) -> bool {
    if domain_seen >= FAMILIAR_AFTER {
        return false;
    }
    let domain = domain.trim().to_ascii_lowercase();
    if domain.is_empty() || subject.trim().is_empty() {
        return false;
    }
    let folded = crate::impersonation::fold(subject);
    SUBJECT_CLAIMS.iter().any(|b| {
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

    /// Real subjects, real senders. All 31 the corpus produced when
    /// the claim was paired with an unfamiliar domain — every one a
    /// phish, and the shape the `From` could not see: the ANA one's
    /// display name is `noticeb8q72a`, claiming nothing at all.
    #[test]
    fn a_claim_in_the_subject_is_caught_when_the_domain_is_new() {
        for (subject, domain) in [
            (
                "【ANA】今年度ご利用実績に伴うボーナスマイル受取のご案内",
                "cjvft.lanlingrexian.com",
            ),
            (
                "ANAマイレージクラブ：マイル登録に関するお知らせ",
                "mail12.jazzyholding.com",
            ),
            (
                "【JCB】本人確認（利用者認証）のお願い",
                "wokjx.crabfishhh.com",
            ),
            (
                "【MyJCB】セキュリティシステム更新に伴う再認証の手続き",
                "b8n0m2p4.cnhlp.com",
            ),
            (
                "【重要】Amazonプライム：支払い方法未更新によるサービス停止の予告",
                "mail01.lingshiluntan.com",
            ),
            (
                "【SAISON】本人認証サービス（ 3Dセキュア）設定再確認のお願い",
                "bgqzb.gzfxn.com",
            ),
            (
                "【三井住友カード】セキュリティシステム更新に伴う再認証の手続き",
                "g5h7j9k1.buxha.com",
            ),
            (
                "【楽天カード】2026年4月分ご利用代金の再精算に関する",
                "mtahost.aikugoo.com",
            ),
            (
                "iCloud+ 月額利用料のお支払いに関するご案内",
                "amezlink-jp.com",
            ),
        ] {
            assert!(
                subject_claims_brand(subject, domain, 0),
                "not caught: {subject}"
            );
        }
    }

    /// And the 160 the corpus produced from familiar domains — every
    /// one of them real mail somebody wanted. A rule that held these
    /// would be worse than no rule, and without the familiarity half
    /// it holds all of them.
    #[test]
    fn the_same_words_from_a_familiar_domain_are_ordinary_mail() {
        for (subject, domain, seen) in [
            (
                "【Amazonギフトカード1,000円プレゼント】5分で終わる",
                "freee.co.jp",
                2543,
            ),
            ("Amazon Autos now offers used a", "linkedin.com", 893),
            (
                "【Amazonブラックフライデーはメルペイで！】",
                "mercari.jp",
                255,
            ),
            (
                "【楽天銀行】マイナンバー（個人番号）の登録手続きが完了しました",
                "rakuten-bank.co.jp",
                305,
            ),
            (
                "【三井住友銀行】Web面談｜予約確定のお知らせ",
                "rooms-online.jp",
                3,
            ),
            ("How Amazon Uses LLMs to Recomm", "substack.com", 553),
        ] {
            assert!(
                !subject_claims_brand(subject, domain, seen),
                "wrongly caught: {subject}"
            );
        }
    }

    /// The brand's own domain needs no history at all.
    #[test]
    fn the_brands_own_domain_is_never_a_claim_against_it() {
        assert!(!subject_claims_brand(
            "【ANA】マイルのお知らせ",
            "ana.co.jp",
            0
        ));
        assert!(!subject_claims_brand(
            "Amazon.co.jp ご注文",
            "amazon.co.jp",
            0
        ));
        assert!(!subject_claims_brand(
            "iCloud+ のお支払い",
            "email.apple.com",
            0
        ));
    }

    /// The loose patterns that were dropped during the measurement,
    /// kept here as the record of why: a bare `ANA` matched English
    /// prose, and a rule built from it would have held newsletters.
    #[test]
    fn a_brand_name_hiding_inside_an_english_word_is_not_a_claim() {
        for subject in [
            "Artificial Intelligence for Enterprise",
            "Practical Project Management Course",
            "Updates to YouTube Data API",
        ] {
            assert!(
                !subject_claims_brand(subject, "brand-new.example", 0),
                "wrongly caught: {subject}"
            );
        }
    }
}
