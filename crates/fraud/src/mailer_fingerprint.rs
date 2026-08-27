//! An `X-Mailer` that no mail client ever wrote.
//!
//! Real senders put a product there: `Microsoft Outlook 16.0`,
//! `Apple Mail (2.3696.120.41.1.1)`, `PHPMailer 6.8.0`, `Zoho Mail`.
//! The tool behind the BEC wave on this server puts a few random
//! letters and a run of dot-separated numbers:
//!
//! ```text
//! X-Mailer: phevb tmiyui 191.8187.55074.84700.25732
//! X-Mailer: wcQrmxYRtaKR FxqYLakkCh 441.28921.87867
//! X-Mailer: genectdgqceqnc zhfjqvzl 191.59556.76593.33332
//! ```
//!
//! Measured over 35,799 production messages: **29 matched and all 29
//! were the scam**, across every persona it wears — `齋藤 真`,
//! `富川 貴司`, `西口 征郎`, `工藤 智昭`, `GOLIA株式会社`, and one
//! impersonating the reader himself — and across `outlook.com`,
//! `hotmail.com` and six throwaway domains. It survives what the
//! display-name check cannot: a persona nobody has seen before.
//!
//! **It is a fingerprint of one tool, and tools change.** When this
//! one starts writing `Microsoft Outlook 16.0` the signal goes quiet
//! and nothing here will notice. It earns its place by being free —
//! one regex over a header that is already parsed — not by being
//! permanent.

/// Score contributed when `X-Mailer` is the dotted-number gibberish one
/// bulk tool writes (`mailrs_inbound::mailer_fingerprint`).
///
/// **Enough on its own**, which nothing else here is. Over 35,799
/// production messages it matched 29 and all 29 were the same BEC wave
/// — every persona it wears, across `outlook.com`, `hotmail.com` and
/// six throwaway domains — while the real clients in the same corpus,
/// including one whose `X-Mailer` reads `Apamanshop Operation System`,
/// matched none. A header no mail client writes is not a hint.
///
/// The honest limit: this is a fingerprint of one tool. When it starts
/// writing `Microsoft Outlook 16.0` the signal goes quiet and nothing
/// here will say so.
pub const GENERATED_MAILER_SCORE: f64 = 5.0;

/// Whether an `X-Mailer` value looks machine-generated rather than
/// named.
///
/// The shape is: one or two runs of letters, then a number with at
/// least one dot-separated group after it. A product name with a
/// version — `PHPMailer 6.8.0` — is excluded by the vendor check
/// rather than by the shape, because `Apamanshop Operation System`
/// matched an earlier, looser pattern and is a real sender.
pub fn is_generated_mailer(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || v.len() > 120 {
        return false;
    }
    if names_a_product(v) {
        return false;
    }
    let parts: Vec<&str> = v.split_whitespace().collect();
    // One to three runs of letters, then the number. Three is the
    // widest the corpus shows (`rHLjMg hWxaApuN lLlePDfxdgnWEE
    // 926.11756`); four stops looking like this generator.
    if parts.len() < 2 || parts.len() > 4 {
        return false;
    }
    let (numeric, letters) = parts.split_last().expect("checked non-empty");
    if !letters.iter().all(|w| is_letters(w, 4)) {
        return false;
    }
    is_dotted_number(numeric)
}

/// A run of at least `min` ASCII letters and nothing else.
fn is_letters(s: &str, min: usize) -> bool {
    s.len() >= min && s.chars().all(|c| c.is_ascii_alphabetic())
}

/// `191.8187.55074` — digits in two or more dot-separated groups.
///
/// Two groups minimum, so a plain build number (`428`) does not
/// qualify: one of those is in the corpus attached to a real sender.
fn is_dotted_number(s: &str) -> bool {
    let groups: Vec<&str> = s.split('.').collect();
    if groups.len() < 2 {
        return false;
    }
    groups
        .iter()
        .all(|g| !g.is_empty() && g.len() <= 6 && g.chars().all(|c| c.is_ascii_digit()))
}

/// Names of things that actually send mail. Present in the value, in
/// any case, means somebody is telling you what they used.
fn names_a_product(v: &str) -> bool {
    const VENDORS: &[&str] = &[
        "outlook",
        "thunderbird",
        "apple",
        "iphone",
        "ipad",
        "mail",
        "zimbra",
        "roundcube",
        "php",
        "sendgrid",
        "mailchimp",
        "amazon",
        "postfix",
        "exim",
        "zoho",
        "gmail",
        "yahoo",
        "becky",
        "shuriken",
        "edmax",
        "salesforce",
        "marketo",
        "sendinblue",
        "klaviyo",
        "mailer",
        "smtp",
        "python",
        "ruby",
        "java",
        "node",
        "swift",
        "sparkpost",
        "mandrill",
        "system",
        "server",
        "notes",
        "groupware",
        "cybozu",
        "desknet",
        "sakura",
        "xserver",
    ];
    let lower = v.to_lowercase();
    VENDORS.iter().any(|n| lower.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every distinct value the corpus caught, verbatim.
    #[test]
    fn the_real_ones_are_caught() {
        for v in [
            "phevb tmiyui 191.8187.55074.84700.25732",
            "wcQrmxYRtaKR FxqYLakkCh 441.28921.87867",
            "genectdgqceqnc zhfjqvzl 191.59556.76593.33332",
            "ixbhansvbsyf gstggfhr 222.68938.66411",
            "wterwvakd qhlkmitdbssxm 471.40762.62898",
            "VRMMI SJWJUYPARI 211.42928.66597.16552.62493",
            "zvfknex rrnkqvvpxnslkt 864.16882.37578",
            "YBPGKV FPLPI 140.59825.59030.27158",
            "rHLjMg hWxaApuN lLlePDfxdgnWEE 926.11756",
        ] {
            assert!(is_generated_mailer(v), "missed {v:?}");
        }
    }

    /// Real clients, including the two the corpus has that look odd —
    /// `Apamanshop Operation System` is a real sender, and `hitncezhvol
    /// 428` is a single build number rather than a dotted run.
    #[test]
    fn real_clients_are_not() {
        for v in [
            "Microsoft Outlook 16.0",
            "Microsoft Outlook Express 6.00.2900.2180",
            "Microsoft Outlook IMO, Build 9.0.2416 (9.0.2911.0)",
            "Apple Mail (2.3696.120.41.1.1)",
            "PHPMailer 6.8.0",
            "Zoho Mail",
            "Apamanshop Operation System",
            "hitncezhvol 428",
            "Becky! ver. 2.75.02",
            "",
            "   ",
        ] {
            assert!(!is_generated_mailer(v), "false positive on {v:?}");
        }
    }

    /// Shapes near the boundary, so the rule is described rather than
    /// merely satisfied.
    #[test]
    fn the_edges_are_where_the_rule_says() {
        // Three letter-runs then the number: that is the widest real form.
        assert!(is_generated_mailer(
            "rHLjMg hWxaApuN lLlePDfxdgnWEE 926.11756"
        ));
        // Four is not — it stops looking like the generator.
        assert!(!is_generated_mailer("aaaa bbbb cccc dddd 926.11756"));
        // The letters have to be letters.
        assert!(!is_generated_mailer("ab1cd efgh 926.11756"));
        // And short ones are initials, not the generator.
        assert!(!is_generated_mailer("ab cd 926.11756"));
        // A very long value is somebody's user-agent string.
        assert!(!is_generated_mailer(&format!(
            "aaaa bbbb {}",
            "1".repeat(130)
        )));
    }
}
