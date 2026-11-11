//! Host names a display name writes out as words of their own.
use crate::impersonation::{display_name_of, fold_char};

/// Every host name the display name of `from` writes out on its own,
/// lowercased: `Mail Delivery System - golia.jp` and `golia.jp I HR`
/// both give `golia.jp`.
///
/// Read from the display name as written, not from `fold`, which drops
/// the spaces and turns `golia.jp I HR` into `golia.jpihr`. A host
/// inside an address — `lihao@golia.jp (Google Drive)`, the name a
/// sharing notice wears — belongs to that address and is left out.
pub fn display_domains(from: &str) -> Vec<String> {
    let display: String = display_name_of(from)
        .chars()
        .map(|c| match c {
            '．' => '.',
            c => fold_char(c),
        })
        .collect::<String>()
        .to_lowercase();
    display
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '@' | '_' | '+')))
        .filter(|word| !word.contains('@'))
        .map(|word| word.trim_matches(|c| c == '.' || c == '-'))
        .filter(|word| is_host(word))
        .map(str::to_string)
        .collect()
}

/// At least two labels, and a last one of letters only: `golia.jp`,
/// not `v1.2` or a bare word.
fn is_host(word: &str) -> bool {
    let Some((head, tld)) = word.rsplit_once('.') else {
        return false;
    };
    !head.is_empty()
        && !head.split('.').any(str::is_empty)
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::display_domains;

    #[test]
    fn a_host_written_as_its_own_word_is_found() {
        for (from, want) in [
            (
                "Mail Delivery System - golia.jp<jade@stellarbbs.com>",
                "golia.jp",
            ),
            ("golia.jp I HR <admin@exportesram-ems.cam>", "golia.jp"),
            ("golia.jp | HR <admin@halfwaylexus.cam>", "golia.jp"),
            ("\"FileRespond - golia.jp\" <a@pge.id>", "golia.jp"),
            ("Mail.Golia.JP <a@x.example>", "mail.golia.jp"),
            ("ＧＯＬＩＡ．ＪＰ 管理者 <a@x.example>", "golia.jp"),
            ("golia.jp様 <a@x.example>", "golia.jp"),
            ("(golia.jp.) <a@x.example>", "golia.jp"),
        ] {
            assert_eq!(display_domains(from), vec![want.to_string()], "{from}");
        }
    }

    #[test]
    fn a_host_inside_an_address_is_not_a_claim() {
        assert!(display_domains("\"lihao@golia.jp (Google Drive)\" <drive@google.com>").is_empty());
    }

    #[test]
    fn words_that_are_not_hosts_give_nothing() {
        for from in [
            "Li Hao <lihao@golia.jp>",
            "Release v1.2 <a@x.example>",
            "golia.jp-team <a@x.example>",
            "a..jp <a@x.example>",
            "lihao@golia.jp",
            "",
        ] {
            assert!(display_domains(from).is_empty(), "{from}");
        }
    }
}
