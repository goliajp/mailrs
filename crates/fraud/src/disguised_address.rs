//! A link that carries the recipient's address with its letters swapped
//! for codes.
//!
//! Reported 2026-11-12 and found three times more in 46,219 production
//! messages (2025-05 to 2026-11): fake signature requests whose one
//! button goes to `…/v/l75b0haMfy1=Ulth=gMfy1l75b0ajg7Tjp` for
//! `lihao@golia.jp`. Every `i` became `75b0`, every `o` `Mfy1`, the `@`
//! `=Ulth=` and the `.` `jg7T`; the rest stayed. The page reads the
//! address back to fill in its fake sign-in form. A link that names its
//! reader in plain text or in base64 is common (sign-in links, tracking);
//! a fixed code per character exists only to get past filters that look
//! for an address, and nothing else in the corpus did it.
//!
//! The match: walking the address, each character is either kept or
//! replaced by a code of 2 to 8 letters, digits or `=`; a character is
//! always treated the same way, two characters never share a code, the
//! `@` and every `.` are replaced, and most of the address shows through:
//! at least three in five of its letters kept, four different ones among
//! them. The match must sit between URL separators. The host is not read:
//! a host name is not where a page gets its reader from.
//!
//! The first version kept five letters of any kind and allowed `-` in a
//! code; over the corpus it read `sakai@golia.jp` out of
//! `utm_campaign=469-motek-2026-jiandokoro-29-09-2026` (two letters, `a`
//! and `i`, kept; the rest swallowed as codes) in three newsletters. A
//! kit keeps most of the address so its page can still read it.

/// The shortest and longest code a character may be swapped for.
const CODE: std::ops::RangeInclusive<usize> = 2..=8;
/// Characters of the address that must be kept as they are.
const KEPT_AT_LEAST: usize = 5;
/// Different letters among them.
const DISTINCT_KEPT_AT_LEAST: usize = 4;
/// The search is cut off after this many steps, and a cut-off search is
/// no match: missing evidence lets the mail through.
const STEPS: usize = 50_000;
/// How much of a link is read.
const TAIL_LIMIT: usize = 2048;

/// Whether the part of `url` after its host carries `address` with its
/// characters swapped for codes.
#[must_use]
pub fn disguises(url: &str, address: &str) -> bool {
    let address: Vec<char> = address.trim().to_lowercase().chars().collect();
    if address.len() > 64 || !address.contains(&'@') {
        return false;
    }
    let tail = after_host(url);
    let tail: Vec<char> = tail.chars().take(TAIL_LIMIT).collect();
    let mut steps = 0;
    (0..tail.len()).any(|start| {
        (start == 0 || separator(tail[start - 1]))
            && Walk {
                tail: &tail,
                address: &address,
                steps: &mut steps,
            }
            .from(start, 0, &mut Vec::new(), 0)
    })
}

/// Everything after `scheme://host`, or the whole string when it has no
/// scheme.
fn after_host(url: &str) -> &str {
    let Some((_, rest)) = url.split_once("://") else {
        return url;
    };
    match rest.find(['/', '?', '#']) {
        Some(i) => &rest[i..],
        None => "",
    }
}

fn separator(c: char) -> bool {
    matches!(c, '/' | '#' | '?' | '&' | '=' | ';')
}

fn code_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '='
}

/// What one address character became: kept, or this code.
type Seen = Vec<(char, Option<Vec<char>>)>;

struct Walk<'a> {
    tail: &'a [char],
    address: &'a [char],
    steps: &'a mut usize,
}

impl Walk<'_> {
    /// Match `address[i..]` at `tail[at..]`, given what each character
    /// already became and how many were kept.
    fn from(&mut self, at: usize, i: usize, seen: &mut Seen, kept: usize) -> bool {
        *self.steps += 1;
        if *self.steps > STEPS {
            return false;
        }
        let Some(&c) = self.address.get(i) else {
            let ends = at == self.tail.len() || separator(self.tail[at]);
            let letters = self
                .address
                .iter()
                .filter(|c| **c != '@' && **c != '.')
                .count();
            let distinct = seen.iter().filter(|(_, v)| v.is_none()).count();
            return ends
                && kept >= KEPT_AT_LEAST
                && kept * 5 >= letters * 3
                && distinct >= DISTINCT_KEPT_AT_LEAST;
        };
        let before = seen.iter().find(|(s, _)| *s == c).map(|(_, v)| v.clone());
        match before {
            Some(None) => self.keep(at, i, seen, kept, c, false),
            Some(Some(code)) => {
                let end = at + code.len();
                end <= self.tail.len()
                    && self.tail[at..end] == code[..]
                    && self.from(end, i + 1, seen, kept)
            }
            None => {
                let must_swap = c == '@' || c == '.';
                (!must_swap && self.keep(at, i, seen, kept, c, true))
                    || self.swap(at, i, seen, kept, c)
            }
        }
    }

    fn keep(
        &mut self,
        at: usize,
        i: usize,
        seen: &mut Seen,
        kept: usize,
        c: char,
        new: bool,
    ) -> bool {
        let Some(&t) = self.tail.get(at) else {
            return false;
        };
        if t.to_ascii_lowercase() != c {
            return false;
        }
        if new {
            seen.push((c, None));
        }
        let found = self.from(at + 1, i + 1, seen, kept + 1);
        if new {
            seen.pop();
        }
        found
    }

    fn swap(&mut self, at: usize, i: usize, seen: &mut Seen, kept: usize, c: char) -> bool {
        for len in CODE {
            let end = at + len;
            if end > self.tail.len() || !self.tail[at..end].iter().all(|t| code_char(*t)) {
                return false;
            }
            let code = self.tail[at..end].to_vec();
            // a code is not the character it stands for, and is no other
            // character's code
            let plain = code.len() == 1 && code[0].to_ascii_lowercase() == c;
            let taken = seen.iter().any(|(_, v)| v.as_ref() == Some(&code));
            if plain || taken {
                continue;
            }
            seen.push((c, Some(code)));
            let found = self.from(end, i + 1, seen, kept);
            seen.pop();
            if found {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::disguises;

    const ME: &str = "lihao@golia.jp";

    /// The four links production had, three code tables between them.
    #[test]
    fn the_links_production_had_are_found() {
        for url in [
            "https://burgesscorporation.biz/v/l75b0haMfy1=Ulth=gMfy1l75b0ajg7Tjp",
            "https://ruddiskincare.org/saro/ld6FmhaP13vr90UgP13vld6Fmajk7Tjp",
            "https://ruddiskincare.org/sm/ld6FmhaP13vr90UgP13vld6Fmajk7Tjp",
            "https://redium.org/nnr/#lj2PhaB14q09gB14lj2Pagx3jp",
        ] {
            assert!(disguises(url, ME), "{url}");
        }
    }

    /// Not in the corpus: another reader, another table, codes of two
    /// and of six, upper-case kept letters, a query parameter.
    #[test]
    fn other_tables_and_readers_are_found() {
        assert!(disguises(
            "https://x.example/p/WSq9Q9GoLiAxX4X4jp?a=1",
            "ws@golia.jp"
        ));
        // `e` is `Q7` throughout, `@` is `ZED`, `.` is `Kk`
        assert!(disguises(
            "https://x.example/r?e=alicQ7ZEDQ7xamplQ7Kkcom",
            "alice@example.com"
        ));
    }

    #[test]
    fn plain_and_encoded_addresses_are_not_this() {
        for url in [
            "https://claude.ai/magic-link#lihao@golia.jp",
            "https://x.example/login?email=lihao%40golia.jp",
            "https://x.example/c/bGloYW9AZ29saWEuanA=",
            "https://x.example/u/lihao@golia_jp",
        ] {
            assert!(!disguises(url, ME), "{url}");
        }
    }

    /// The host is not read, and a match must be a whole segment.
    #[test]
    fn hosts_and_partial_segments_do_not_count() {
        assert!(!disguises(
            "https://l75b0haMfy1=Ulth=gMfy1l75b0ajg7Tjp.example/",
            ME
        ));
        assert!(!disguises(
            "https://x.example/zzl75b0haMfy1=Ulth=gMfy1l75b0ajg7Tjp",
            ME
        ));
    }

    /// Two characters sharing a code, or too little kept, is not the table.
    #[test]
    fn a_table_that_is_not_one_does_not_count() {
        // `i` and `o` both written `ab`
        assert!(!disguises("https://x.example/labhaabXYgablabaQQjp", ME));
        // only `@` and `.` written, nothing else differs: five kept is met,
        // but a code that is the URL's own separator is not a code
        assert!(!disguises("https://x.example/lihao/golia/jp", ME));
    }

    /// A campaign name is not an address with most of its letters gone.
    #[test]
    fn a_newsletter_campaign_parameter_is_not_an_address() {
        assert!(!disguises(
            "https://www.directindustry.com/newsletter/20260929/di-469-Ja.html?token=d101484f#utm_medium=email&utm_source=insight&utm_campaign=469-motek-2026-jiandokoro-29-09-2026&utm_content=newsletter",
            "sakai@golia.jp"
        ));
    }

    #[test]
    fn a_long_link_costs_a_bounded_search() {
        let url = format!("https://x.example/{}", "l".repeat(5000));
        assert!(!disguises(&url, "llllllllllll@llllll.ll"));
    }
}
