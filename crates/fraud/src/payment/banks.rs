//! The Japanese bank names a written bank name is checked against.
//!
//! Reference data compiled into the binary, never fetched at run time:
//! `data/jp-bank-names.tsv` (`kind<TAB>name<TAB>corporate number`, `#`
//! lines are comments) lists the banks, foreign bank branches, shinkin
//! banks, credit cooperatives and labour banks licensed in Japan, with
//! their federations; `data/jp-bank-aliases.tsv` (`former<TAB>current<TAB>year`)
//! maps former names mail still writes to the bank they became.
//! ゆうちょ銀行 is added here, so it does not depend on the list.
//!
//! A name matches only after [`normalize`]: width, `株式会社`, a trailing
//! `銀行` and the long and short forms of the institution type. Nothing
//! looser: a name that does not match is not on the list, and an account
//! at it is a suspected one. Agricultural cooperatives (JA) are not on the
//! list.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Names added to the list.
const EXTRA_NAMES: [&str; 1] = ["ゆうちょ銀行"];

/// Bank names, normalised, and former names to current ones.
#[derive(Debug, Default)]
pub struct BankList {
    names: HashSet<String>,
    aliases: HashMap<String, String>,
}

impl BankList {
    /// Read the name list (`kind<TAB>name…`) and the alias list
    /// (`former<TAB>current<TAB>year`); `#` lines are comments.
    #[must_use]
    pub fn parse(names: &str, aliases: &str) -> Self {
        let rows = |text: &str| -> Vec<Vec<String>> {
            text.lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .map(|l| l.split('\t').map(str::to_string).collect())
                .collect()
        };
        let mut list = Self::default();
        for cols in rows(names) {
            if let Some(name) = cols.get(1) {
                list.names.insert(normalize(name));
            }
        }
        list.names.extend(EXTRA_NAMES.iter().map(|n| normalize(n)));
        for cols in rows(aliases) {
            if let [former, current, ..] = &cols[..] {
                list.aliases.insert(normalize(former), normalize(current));
            }
        }
        list
    }

    /// The listed bank `written` names, as its normalised current name:
    /// the same for every way of writing it and for a former name.
    #[must_use]
    pub fn canonical(&self, written: &str) -> Option<String> {
        let name = normalize(written);
        if self.names.contains(&name) {
            return Some(name);
        }
        self.aliases.get(&name).cloned()
    }

    /// How many names the list holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// The list holds no names.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

static LIST: LazyLock<BankList> = LazyLock::new(|| {
    BankList::parse(
        include_str!("../../data/jp-bank-names.tsv"),
        include_str!("../../data/jp-bank-aliases.tsv"),
    )
});

/// The list compiled into this build.
pub(super) fn listed() -> &'static BankList {
    &LIST
}

/// Long forms of the institution types, and the short forms mail and the
/// list are both brought to.
const SHORT_FORMS: [(&str, &str); 4] = [
    ("信用金庫", "信金"),
    ("信用組合", "信組"),
    ("労働金庫", "労金"),
    ("農業協同組合", "農協"),
];

/// A bank name as the list compares it: full-width letters and digits
/// made ASCII and upper case, half-width katakana made full-width, spaces
/// and `株式会社` dropped, a trailing `銀行` dropped, and the long forms of
/// the other institution types shortened.
#[must_use]
pub fn normalize(name: &str) -> String {
    let mut s: String = fold_kana(name)
        .chars()
        .map(fold_width)
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    for corp in ["株式会社", "(株)"] {
        s = s.replace(corp, "");
    }
    if let Some(rest) = s.strip_suffix("銀行") {
        s = rest.to_string();
    }
    for (long, short) in SHORT_FORMS {
        if let Some(rest) = s.strip_suffix(long) {
            s = format!("{rest}{short}");
        }
    }
    s
}

/// A full-width ASCII character as ASCII, the ideographic space as a
/// space; anything else as it is.
pub(super) fn fold_width(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{3000}' => ' ',
        _ => c,
    }
}

/// Half-width katakana U+FF65..U+FF9D as full-width, in order.
const HALF_KANA: &str = "・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン";

/// Half-width katakana as full-width, a following voiced or semi-voiced
/// mark joined to the letter before it (dropped where it cannot join).
fn fold_kana(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{FF9E}' | '\u{FF9F}' => {
                let step = c as u32 - 0xFF9D;
                if let Some(prev) = out.pop() {
                    out.push(voiced(prev, step).unwrap_or(prev));
                }
            }
            '\u{FF65}'..='\u{FF9D}' => out.extend(HALF_KANA.chars().nth(c as usize - 0xFF65)),
            _ => out.push(c),
        }
    }
    out
}

/// `kana` with a voiced (`step` 1) or semi-voiced (2) mark, when it takes
/// one.
fn voiced(kana: char, step: u32) -> Option<char> {
    if kana == 'ウ' && step == 1 {
        return Some('ヴ');
    }
    let takes = match step {
        1 => "カキクケコサシスセソタチツテトハヒフヘホ".contains(kana),
        _ => "ハヒフヘホ".contains(kana),
    };
    takes.then(|| char::from_u32(kana as u32 + step)).flatten()
}

#[cfg(test)]
#[path = "banks_tests.rs"]
mod tests;
