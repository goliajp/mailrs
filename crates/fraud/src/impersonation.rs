//! Someone claiming to be your own company from an address that is not.
//!
//! The scam this exists for: a From header reading
//! `GOLIA株式会社 <ipdxuawesj@auto360d.com>`, or the same with a made-up
//! colleague's name, asking the reader to reply with their LINE QR code
//! so "work contact" can move there. Measured over 35,799 production
//! messages, 25 of them arrived — and **every one passed SPF**, 18 of
//! them passed DKIM and DMARC as well. They authenticate correctly
//! because the sender owns the throwaway domain. Authentication has
//! nothing to say about this, and neither does reputation: the domains
//! rotate (`auto360d.com`, `mhsfwf.com`, `tylfjs.com`, `wzglff.com`)
//! and the local part is fresh random letters each time.
//!
//! What does not rotate is the claim. The display name has to say the
//! company's name for the mail to work at all.
//!
//! **The naive form of this rule is a disaster, and the corpus says so.**
//! "display name contains `golia`, domain is not ours" matches 534
//! messages, of which 490 are GitHub notifications — the repository is
//! `goliajp/mailrs`, so the org name is inside the display name of
//! every one. Nineteen more are the reader's own Gmail. Narrowed to the
//! full registered name it matches 12, nine of them scams and three of
//! them Slack, whose workspace is legitimately called that.
//!
//! Hence: full names only, and an allow-list for the services that
//! carry your name because you gave it to them.

/// Score contributed when the From display name claims to be the
/// receiving organisation itself while the address is somewhere else
/// (`mailrs_inbound::impersonation`).
///
/// **High, because authentication has nothing to say here.** All 25 of
/// these in a 35,799-message corpus passed SPF and 18 passed DKIM and
/// DMARC as well — the sender owns the throwaway domain and configures
/// it correctly. Reputation is no better: the domains rotate every few
/// messages. The claim in the display name is the only part that
/// cannot change, because without it the mail does not work.
///
/// Still a score and not a verdict. At 4.5 against the default 5.0 it
/// junks beside any content signal at all (0.5), and beside a
/// suspicious sender or zero-width padding on its own — but a lone
/// false positive from a service that carries your name legitimately
/// still reaches the inbox, where the reader can see it. The
/// allow-list is the real defence there; this is the second one.
pub const CLAIMS_OUR_NAME_SCORE: f64 = 4.5;

/// Whether `from` claims to be one of `names` while its address sits
/// outside `ours` and outside `allowed`.
///
/// `from` is the decoded `From:` value — display name and address, as
/// `mailrs_inbound::identity` produces it. Undecoded input is the way
/// to make this always answer false: the names arrive base64'd inside
/// `=?UTF-8?B?…?=` in every sample.
pub fn claims_our_name(from: &str, names: &[String], ours: &[String], allowed: &[String]) -> bool {
    let Some(address) = address_of(from) else {
        return false;
    };
    let Some(domain) = address.rsplit('@').next() else {
        return false;
    };
    let domain = domain.trim().trim_end_matches('>').to_ascii_lowercase();
    if domain.is_empty() {
        return false;
    }
    if in_domain_list(&domain, ours) || in_domain_list(&domain, allowed) {
        return false;
    }
    let display = display_name_of(from);
    if display.is_empty() {
        return false;
    }
    let folded = fold(&display);
    names
        .iter()
        .filter(|n| !n.trim().is_empty())
        .any(|n| folded.contains(&fold(n)))
}

/// The address inside `<…>`, or the whole field when there are no
/// angle brackets.
fn address_of(from: &str) -> Option<&str> {
    if let Some(open) = from.rfind('<') {
        let rest = &from[open + 1..];
        return Some(rest.split('>').next().unwrap_or(rest));
    }
    if from.contains('@') {
        return Some(from.trim());
    }
    None
}

/// Everything before the address, quotes stripped.
fn display_name_of(from: &str) -> String {
    let head = match from.rfind('<') {
        Some(open) => &from[..open],
        None => "",
    };
    head.trim().trim_matches('"').trim().to_string()
}

/// Case-folded and stripped of the spaces a sender puts between the
/// characters of a name to slip a literal comparison — `GOLIA 株式会社`
/// and `ＧＯＬＩＡ株式会社` are the same claim as `GOLIA株式会社`.
fn fold(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(fold_char)
        .collect::<String>()
        .to_lowercase()
}

/// Full-width Latin letters and digits folded to their ASCII forms.
fn fold_char(c: char) -> char {
    match c {
        'Ａ'..='Ｚ' | 'ａ'..='ｚ' | '０'..='９' => {
            char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
        }
        _ => c,
    }
}

/// A domain matches an entry exactly, or as a subdomain of it.
fn in_domain_list(domain: &str, list: &[String]) -> bool {
    list.iter().any(|d| {
        let d = d.trim().to_ascii_lowercase();
        !d.is_empty() && (domain == d || domain.ends_with(&format!(".{d}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
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

    /// The eight distinct senders the corpus caught, verbatim.
    #[test]
    fn the_real_ones_are_caught() {
        for from in [
            "GOLIA株式会社 <ipdxuawesj@auto360d.com>",
            "GOLIA株式会社 <ylcs@mhsfwf.com>",
            "GOLIA株式会社 <srxgri@qianshiqi.com>",
            "GOLIA株式会社 <services1@mhsfwf.com>",
            "GOLIA株式会社 <vqkpsfdl@qjymtxcy.com>",
            "GOLIA株式会社 <JoshuaDouglas2265@outlook.com>",
            "GOLIA株式会社 <AngelicaWilson9596@hotmail.com>",
            "GOLIA株式会社 <panicharenson5287@hotmail.com>",
        ] {
            assert!(
                claims_our_name(from, &names(), &ours(), &allowed()),
                "missed {from}"
            );
        }
    }

    /// And what must not be. Every one of these is in the same corpus.
    #[test]
    fn the_legitimate_ones_are_not() {
        for from in [
            // 490 of these. The org name is in the repository path.
            "goliajp/mailrs <notifications@github.com>",
            // The reader's own account.
            "GOLIA <goliaaccess@gmail.com>",
            // A workspace that is legitimately called this.
            "GOLIA株式会社 <notification@slack.com>",
            "GOLIA Jira <no-reply@golia.atlassian.net>",
            // Ourselves.
            "GOLIA株式会社 <lihao@golia.jp>",
            "GOLIA株式会社 <noreply@golia.ai>",
        ] {
            assert!(
                !claims_our_name(from, &names(), &ours(), &allowed()),
                "false positive on {from}"
            );
        }
    }

    /// A name is a claim wherever it sits in the display name, and
    /// spacing it out is not a way around.
    #[test]
    fn the_claim_survives_dressing_up() {
        for from in [
            "\"GOLIA株式会社 総務部\" <x@evil.test>",
            "GOLIA 株式会社 <x@evil.test>",
            "ＧＯＬＩＡ株式会社 <x@evil.test>",
            "【GOLIA株式会社】 <x@evil.test>",
        ] {
            assert!(
                claims_our_name(from, &names(), &ours(), &allowed()),
                "missed {from}"
            );
        }
    }

    /// Nothing configured is nothing claimed — the check must be off
    /// until someone says what their organisation is called.
    #[test]
    fn an_empty_configuration_never_fires() {
        let from = "GOLIA株式会社 <x@evil.test>";
        assert!(!claims_our_name(from, &[], &ours(), &allowed()));
        assert!(!claims_our_name(
            from,
            &["".into(), "  ".into()],
            &ours(),
            &allowed()
        ));
    }

    /// Malformed input answers false rather than panicking.
    #[test]
    fn odd_headers_do_not_panic() {
        for from in [
            "",
            "GOLIA株式会社",
            "<>",
            "GOLIA株式会社 <>",
            "@",
            "GOLIA株式会社 <@>",
        ] {
            let _ = claims_our_name(from, &names(), &ours(), &allowed());
        }
    }

    /// A subdomain of ours is still ours, and a subdomain of an allowed
    /// service is still allowed.
    #[test]
    fn subdomains_follow_their_parent() {
        assert!(!claims_our_name(
            "GOLIA株式会社 <bounce@mail.golia.jp>",
            &names(),
            &ours(),
            &allowed()
        ));
        assert!(!claims_our_name(
            "GOLIA株式会社 <x@team.slack.com>",
            &names(),
            &ours(),
            &allowed()
        ));
        // But a lookalike that merely ends in the same letters is not.
        assert!(claims_our_name(
            "GOLIA株式会社 <x@notgolia.jp>",
            &names(),
            &ours(),
            &allowed()
        ));
    }
}
