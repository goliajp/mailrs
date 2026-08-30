//! A hostname that claims to be a company it is not.
//!
//! `linghit@aliyun.rvezovp.cn`, `linghit@tencent.szfxz222.cn`. The
//! registered domain is `rvezovp.cn`; the label in front of it says
//! Aliyun. A reader glancing at the address sees a familiar name in
//! it, and it is there for no other reason.
//!
//! # Why this hides mail and the sibling checks do not
//!
//! There is no legitimate way to end up here. A subdomain is chosen
//! by whoever owns the parent, so `aliyun.rvezovp.cn` is a statement
//! `rvezovp.cn` made about itself, deliberately, and it is false. It
//! is not a coincidence of wording like a brand named in a subject,
//! and it is not a habit legitimate senders share like a numbered
//! relay host — it is a lie told in the envelope.
//!
//! Measured over 36,323 production messages: **25 messages, 24
//! senders, every one part of the same fortune-telling campaign**,
//! and no legitimate sender matched. A company's own subdomains sit
//! under its own registrable domain, where this check does not look.
//!
//! The campaign it caught is worth describing, because it is the one
//! the reader could not get rid of: mail addressed to a name that is
//! not theirs, from `airmessage.cn`, `rvezovp.cn`, `qianjingdd.cn`,
//! `orpseon.cn` and `szfxz222.cn` — five registered domains, all
//! replying to `linghit.com` — and an unsubscribe link in the body
//! that is not a `List-Unsubscribe` header at all. Following it
//! confirms the address is read, and the volume goes up.

/// Companies whose names get borrowed as a subdomain label, and the
/// domains that really are them.
///
/// Deliberately short and deliberately **not** the brand list used
/// for display names. That one is about what a message calls itself,
/// where a coincidence is possible — `apple` is a fruit. This one is
/// about a label somebody registered, where it is not.
pub const HOSTED_CLAIMS: &[crate::brand::Brand] = &[
    crate::brand::Brand {
        name: "aliyun",
        domains: &[
            "aliyun.com",
            "alibaba.com",
            "aliyuncs.com",
            "alibaba-inc.com",
        ],
    },
    crate::brand::Brand {
        name: "tencent",
        domains: &["tencent.com", "qq.com"],
    },
    crate::brand::Brand {
        name: "amazon",
        domains: &[
            "amazon.com",
            "amazon.co.jp",
            "amazonaws.com",
            "aws.com",
            "amazonses.com",
        ],
    },
    crate::brand::Brand {
        name: "google",
        domains: &["google.com", "gmail.com", "googlemail.com", "youtube.com"],
    },
    crate::brand::Brand {
        name: "apple",
        domains: &["apple.com", "icloud.com", "me.com"],
    },
    crate::brand::Brand {
        name: "icloud",
        domains: &["apple.com", "icloud.com"],
    },
    crate::brand::Brand {
        name: "microsoft",
        domains: &[
            "microsoft.com",
            "outlook.com",
            "office.com",
            "microsoftonline.com",
        ],
    },
    crate::brand::Brand {
        name: "paypal",
        domains: &["paypal.com"],
    },
    crate::brand::Brand {
        name: "rakuten",
        domains: &["rakuten.co.jp", "rakuten.com"],
    },
    crate::brand::Brand {
        name: "baidu",
        domains: &["baidu.com"],
    },
    crate::brand::Brand {
        name: "netflix",
        domains: &["netflix.com"],
    },
    crate::brand::Brand {
        name: "docomo",
        domains: &["docomo.ne.jp", "nttdocomo.co.jp"],
    },
    crate::brand::Brand {
        name: "jcb",
        domains: &["jcb.co.jp"],
    },
    crate::brand::Brand {
        name: "line",
        domains: &["line.me", "linecorp.com"],
    },
];

/// Whether any label in front of the registrable domain names a
/// company the registrable domain is not.
///
/// **A whole label**, never a substring: `mailingling.airmessage.cn`
/// must not read as a claim about `line`. That is the same trap the
/// display-name check documents, where `golia` inside a GitHub path
/// matched 490 notifications.
#[must_use]
pub fn hostname_claims_a_company(host: &str) -> Option<&'static str> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let registrable = crate::brand::registrable(&host);
    let prefix = host.strip_suffix(&registrable)?.trim_end_matches('.');
    if prefix.is_empty() {
        return None;
    }
    let labels: Vec<&str> = prefix.split('.').filter(|l| !l.is_empty()).collect();
    HOSTED_CLAIMS.iter().find_map(|b| {
        let names_it = labels.contains(&b.name);
        let is_theirs = b
            .domains
            .iter()
            .any(|d| registrable == *d || registrable.ends_with(&format!(".{d}")));
        (names_it && !is_theirs).then_some(b.name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The campaign the reader could not get rid of.
    #[test]
    fn a_borrowed_name_in_a_subdomain_is_caught() {
        assert_eq!(
            hostname_claims_a_company("aliyun.rvezovp.cn"),
            Some("aliyun")
        );
        assert_eq!(
            hostname_claims_a_company("tencent.szfxz222.cn"),
            Some("tencent")
        );
        assert_eq!(
            hostname_claims_a_company("mail.amazon.phishy.example"),
            Some("amazon")
        );
    }

    /// A company's own subdomains sit under its own registrable
    /// domain, where this does not look.
    #[test]
    fn a_companys_own_hosts_are_not_a_claim_against_it() {
        for host in [
            "aliyun.com",
            "mail.aliyun.com",
            "email.apple.com",
            "notifications.google.com",
            "email.tiktok.com",
            "amazonses.com",
            "id.atlassian.net",
        ] {
            assert_eq!(
                hostname_claims_a_company(host),
                None,
                "wrongly caught: {host}"
            );
        }
    }

    /// A whole label, never a substring — the trap the display-name
    /// check already paid for once.
    #[test]
    fn a_name_inside_a_longer_label_is_not_a_claim() {
        for host in [
            "mailingling.airmessage.cn", // contains `line`
            "shengxiao.airmessage.cn",
            "applesupport.example.com", // contains `apple`
            "googleplex.example.com",
        ] {
            assert_eq!(
                hostname_claims_a_company(host),
                None,
                "wrongly caught: {host}"
            );
        }
    }
}
