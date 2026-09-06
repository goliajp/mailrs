//! The companies, and the domains that really are them.
//!
//! Split from the matching in `super`: this file is data that grows
//! every time a phish names somebody new, and that one is the
//! comparison. Keeping them apart is what lets the comparison stay
//! short enough to read.
//!
//! **A short `domains` list convicts the brand's own mail** — see
//! `super::brand_is_the_display_name`. Adding a domain is the safe
//! direction; adding a *brand* is not.

use super::Brand;

// Official email domains verified 2026-09-06 against:
// https://help.openai.com/en/articles/11725090-verifying-communications-from-openai
// The matcher includes subdomains, covering email/mail/tm/ads/sales.openai.com.
const OPENAI_DOMAINS: &[&str] = &["openai.com", "c-openai.com"];

/// The companies this deployment has actually seen impersonated,
/// plus the ones every phishing kit ships with.
///
/// Deliberately short. Every entry is a chance to hide somebody's
/// real mail, and the corpus above shows how easily a common word
/// does that — `apple` alone matched three papers about apples. A
/// name earns its place by being a company whose mail asks for money
/// or credentials, not by being well known.
pub const BRANDS: &[Brand] = &[
    // User-reported screenshot, 2026-09-06: ChatGPT from
    // admin@heavenerandassociates.com asks to update a failed payment.
    // This is one reported sample, not a new corpus measurement.
    Brand {
        name: "chatgpt",
        domains: OPENAI_DOMAINS,
    },
    Brand {
        name: "openai",
        domains: OPENAI_DOMAINS,
    },
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
        // `aeon.com` is the iAEON app's own sender, and it was
        // missing until a test caught it. See the warning above
        // `brand_is_the_display_name`: a short domain list convicts
        // the brand's own mail.
        domains: &["aeon.co.jp", "aeon.com", "aeonbank.co.jp", "aeoncard.co.jp"],
    },
    // Added 2026-08-31: `American Express <custsvc@lcacosmeticos.com>`
    // arrived, linking to `fortreese-business.com`, and nothing
    // fired because the name was not here at all. 96 production
    // messages name a brand and nothing else from somebody else's
    // domain, and most of them say American Express.
    Brand {
        name: "american express",
        domains: &["americanexpress.com", "aexp.com"],
    },
    Brand {
        name: "amex",
        domains: &["americanexpress.com", "aexp.com"],
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
    // The Latin spelling the card itself uses. `S\u{feff}AISON
    // <noreply@hxqsym.dwnrk.com>` was in the corpus and only the
    // katakana was listed, so nothing recognised the claim.
    Brand {
        name: "saison",
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
        name: "chatgpt",
        domains: OPENAI_DOMAINS,
    },
    Brand {
        name: "openai",
        domains: OPENAI_DOMAINS,
    },
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
