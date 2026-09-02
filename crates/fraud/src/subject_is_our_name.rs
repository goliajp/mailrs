//! A subject that is this organisation's name and almost nothing else.
//!
//! ```text
//! From: 富川貴司 <very7wauross@outlook.com>
//! Subject: Re：【業務連絡】ＧＯＬＩＡ株式会社
//!
//! 今後の業務連絡を円滑にするため、恐れ入りますが、本メール受信後にご自身の
//! LINEアカウントのQRコードと併せて、所属部署および役職をご記載のうえ…
//! ```
//!
//! # Why every other rule missed this
//!
//! The display name is `富川貴司` — a Japanese personal name that is
//! **not** one of ours, not a brand, and not the organisation. The
//! company name is in the **subject**, where nothing looked. And the
//! sender is `outlook.com`: real, ordinary, high-reputation, so not
//! one of the domain-shaped rules can touch it.
//!
//! # The line, and where it falls
//!
//! Measured over 36,976 production messages. 90 carry the full
//! organisation name in the subject from a domain that is neither
//! ours nor allow-listed — and most of those are legitimate: an
//! accountant's tax correspondence, freee's payroll, Stripe, a Tokyo
//! metropolitan agency, LinkedIn. **Naming the company is ordinary.**
//!
//! What is not ordinary is a subject that is *only* the name. Strip
//! the org name and the decorations a subject carries — `Re:`,
//! `【…】`, brackets, punctuation — and count what is left:
//!
//! | characters left | messages | what they are |
//! |---|---|---|
//! | **0** | 15 | every one fraud |
//! | **4** | 9 | every one fraud — `業務変更`, `業務指示`, `要請事項`, `業務通知` |
//! | 6 | 2 | an accountant: `Re: RE：年末調整の件（GOLIA株式会社）` |
//! | 9–19 | 64 | ChatGPT, a Tokyo agency, Stripe, LinkedIn, freee |
//!
//! [`BARE_NAME_SLACK`] sits at 4: **24 caught, nothing legitimate
//! touched**, and the first legitimate message has 6.
//!
//! The 24 are one campaign wearing several names — `富川貴司`,
//! `齊藤 真`, and **`李好`, the reader's own** — across `outlook.com`,
//! `hotmail.com`, a `.cn`, and a dozen throwaway domains. Nine of
//! them put the org name in the display name too and are already
//! caught by `claims_our_name`; the other fifteen are not caught by
//! anything.
//!
//! # Why this needs no list of ours beyond the one it has
//!
//! It compares against `MAILRS_ORG_NAMES`, which a deployment states
//! about itself, and it fires only when the domain is neither ours
//! nor allow-listed. Empty org names switch it off, which delivers
//! rather than holds — the same shape as `claims_our_name`, and for
//! the same reason.

/// How many characters may remain beside the organisation's name.
///
/// Four. `GOLIA株式会社 業務指示` is a subject that says nothing but
/// the company and a two-word imperative; the accountant's
/// `年末調整の件` is six. See the table above.
pub const BARE_NAME_SLACK: usize = 4;

/// Punctuation and reply prefixes, which say nothing about content.
///
/// **The brackets go; what is inside them stays.** The first version
/// dropped bracketed spans whole, on the theory that `【業務連絡】` is
/// a tag rather than content — and a test written from the corpus
/// refused it within a minute: a Tokyo metropolitan agency sends
/// `12/9【PWご連絡】GOLIA株式会社様`, where the bracket carries the
/// entire subject. Dropping it left `12/9様` and convicted them.
///
/// It is also what the measurement did. The Python that produced the
/// 0/4/6 table stripped bracket characters, not bracketed spans, so
/// a Rust version that dropped the spans was answering a different
/// question from the one the threshold was chosen for.
fn strip_decoration(folded: &str) -> String {
    let mut out: String = folded
        .chars()
        .filter(|c| !is_punctuation(*c) && !is_bracket(*c))
        .collect();
    let _ = &mut out;
    // `re:` / `fw:` / `fwd:` survive the punctuation strip as letters.
    let mut s = out.as_str();
    loop {
        let t = s
            .strip_prefix("re")
            .or_else(|| s.strip_prefix("fwd"))
            .or_else(|| s.strip_prefix("fw"));
        match t {
            Some(rest) if rest.len() < s.len() => s = rest,
            _ => break,
        }
    }
    s.to_string()
}

fn is_bracket(c: char) -> bool {
    matches!(
        c,
        '(' | '（' | ')' | '）' | '[' | '［' | ']' | '］' | '【' | '】' | '〔' | '〕' | '《' | '》'
    )
}

fn is_punctuation(c: char) -> bool {
    matches!(
        c,
        ':' | '：'
            | '.'
            | '。'
            | ','
            | '、'
            | '-'
            | '_'
            | '|'
            | '｜'
            | '/'
            | '／'
            | '!'
            | '！'
            | '?'
            | '？'
            | '~'
            | '～'
            | '*'
            | '#'
            | '＃'
            | '>'
            | '<'
            | '＞'
            | '＜'
            | '·'
            | '・'
    )
}

/// Whether the subject is this organisation's name and little else,
/// sent from a domain that is not ours.
///
/// `from` is the decoded `From:` value; `subject` the decoded
/// subject. `names`, `ours` and `allowed` are the same three lists
/// [`crate::impersonation::claims_our_name`] takes, and an empty
/// `names` switches the check off.
#[must_use]
pub fn subject_is_our_name(
    from: &str,
    subject: &str,
    names: &[String],
    ours: &[String],
    allowed: &[String],
) -> bool {
    if subject.trim().is_empty() {
        return false;
    }
    let Some(address) = crate::impersonation::address_of(from) else {
        return false;
    };
    let Some(domain) = address.rsplit('@').next() else {
        return false;
    };
    let domain = domain.trim().trim_end_matches('>').to_ascii_lowercase();
    if domain.is_empty()
        || crate::impersonation::in_domain_list(&domain, ours)
        || crate::impersonation::in_domain_list(&domain, allowed)
    {
        return false;
    }
    let folded = crate::impersonation::fold(subject);
    names
        .iter()
        .filter(|n| !n.trim().is_empty())
        .map(|n| crate::impersonation::fold(n))
        .filter(|n| !n.is_empty())
        .any(|n| match folded.find(&n) {
            None => false,
            Some(at) => {
                let mut rest = folded.clone();
                rest.replace_range(at..at + n.len(), "");
                strip_decoration(&rest).chars().count() <= BARE_NAME_SLACK
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn org() -> Vec<String> {
        vec!["GOLIA株式会社".into(), "GOLIA K.K.".into()]
    }
    fn ours() -> Vec<String> {
        vec!["golia.jp".into(), "golia.ai".into()]
    }
    fn allowed() -> Vec<String> {
        vec![
            "slack.com".into(),
            "github.com".into(),
            "atlassian.net".into(),
        ]
    }
    fn hit(from: &str, subject: &str) -> bool {
        subject_is_our_name(from, subject, &org(), &ours(), &allowed())
    }

    /// All 24, in the shapes production sent them. Fullwidth, a reply
    /// prefix, a bracket tag, and a two-word imperative all count as
    /// nothing beside the name.
    #[test]
    fn a_subject_that_is_only_our_name_is_caught() {
        for (from, subject) in [
            // The one that prompted this, verbatim.
            (
                "富川貴司 <very7wauross@outlook.com>",
                "Re：【業務連絡】ＧＯＬＩＡ株式会社",
            ),
            (
                "富川貴司 <DeltoroDevall1503@hotmail.com>",
                "ＧＯＬＩＡ株式会社",
            ),
            // Wearing the reader's own name.
            ("李好 <hkpwnbxv08@outlook.com>", "ＧＯＬＩＡ株式会社"),
            ("齊藤 真 <fkflaug@ghcvnt.cn>", "ｇｏｌｉａ株式会社"),
            ("LI HAO <rupture@jadhj.com>", "[業務連絡]GOLIA株式会社"),
            ("<Rodger1984Lynne30123@outlook.com>", "ＧＯＬＩＡ株式会社"),
            // Four characters of imperative — the other half of the族.
            (
                "GOLIA株式会社 <services1@mhsfwf.com>",
                "GOLIA株式会社 業務指示",
            ),
            (
                "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
                "GOLIA株式会社 業務変更",
            ),
            (
                "GOLIA株式会社 <srxgri@qianshiqi.com>",
                "GOLIA株式会社 要請事項",
            ),
            ("GOLIA株式会社 <ylcs@mhsfwf.com>", "GOLIA株式会社 業務通知"),
        ] {
            assert!(hit(from, subject), "not caught: {subject}");
        }
    }

    /// **Naming the company is ordinary**, and these are the 66 that
    /// do it legitimately. The nearest one has six characters beside
    /// the name; the threshold is four.
    #[test]
    fn mail_that_merely_names_the_company_is_ordinary() {
        for (from, subject) in [
            (
                "Masato Nagata <nagata@nagatax.tokyo.jp>",
                "Re: RE：年末調整の件（GOLIA株式会社）",
            ),
            (
                "Masato Nagata <nagata@nagatax.tokyo.jp>",
                "Re: 【訂正】源泉所得税の電子納付につきまして（GOLIA株式会社）",
            ),
            (
                "Roman Daneghyan <roman@thebusinessrover.com>",
                "GOLIA株式会社 in ChatGPT",
            ),
            (
                "与那覇幸子 <yu-yonaha@tokyo-kosha.or.jp>",
                "12/9【PWご連絡】GOLIA株式会社様",
            ),
            (
                "Stripe <notifications@stripe.com>",
                "[需要执行操作] 请提供有关 GOLIA K.K. 的补充信息",
            ),
            (
                "領英 <messages-noreply@linkedin.com>",
                "查看您上周错过的GOLIA株式会社的内容",
            ),
            (
                "freee人事労務 <noreply@freee.co.jp>",
                "入社手続きが完了しました｜GOLIA株式会社",
            ),
        ] {
            assert!(!hit(from, subject), "wrongly caught: {subject}");
        }
    }

    /// Our own mail, and the services we told our name to.
    #[test]
    fn our_own_domains_and_the_allow_list_are_spared() {
        for from in [
            "誰か <someone@golia.jp>",
            "GOLIA株式会社 <noreply@golia.ai>",
            "GitHub <noreply@github.com>",
            "Jira <jira@golia.atlassian.net>",
        ] {
            assert!(!hit(from, "GOLIA株式会社"), "wrongly caught: {from}");
        }
    }

    /// No org names configured is the check off, not every subject
    /// convicted — the same direction `claims_our_name` fails in.
    #[test]
    fn an_unconfigured_deployment_convicts_nobody() {
        assert!(!subject_is_our_name(
            "x <a@evil.invalid>",
            "GOLIA株式会社",
            &[],
            &ours(),
            &allowed()
        ));
        assert!(!subject_is_our_name(
            "x <a@evil.invalid>",
            "GOLIA株式会社",
            &["".into(), "  ".into()],
            &ours(),
            &allowed()
        ));
    }

    /// A subject with no organisation name in it at all.
    #[test]
    fn a_subject_without_the_name_is_not_this_rule() {
        for s in ["請求書のご送付", "", "Re: 見積書の件", "Meeting tomorrow"] {
            assert!(!hit("x <a@evil.invalid>", s), "wrongly caught: {s:?}");
        }
    }
}
