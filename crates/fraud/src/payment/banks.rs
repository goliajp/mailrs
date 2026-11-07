//! The Zengin bank list a Japanese account's bank is checked against.
//!
//! Reference data compiled into the binary from `data/zengin-banks.tsv`
//! (one bank per line, `code<TAB>name`, `#` lines are comments), never
//! fetched at run time. Names are compared after [`normalize`], so the
//! list's short forms (`三菱ＵＦＪ`, `北海道信金`, `横浜農協`) match what a
//! letter writes (`三菱UFJ銀行`, `北海道信用金庫`, `JA横浜`).
//!
//! An empty list verifies nothing: every account structure found is then
//! a suspected one.

use std::sync::LazyLock;

/// Bank codes by normalised name.
#[derive(Debug, Default)]
pub struct BankList {
    names: Vec<(String, String)>,
}

impl BankList {
    /// Read the list's text: `code<TAB>name` lines, `#` comments.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let names = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let mut cols = l.split('\t');
                let code = cols.next()?.trim();
                let name = normalize(cols.next()?);
                (!code.is_empty() && !name.is_empty()).then(|| (name, code.to_string()))
            })
            .collect();
        Self { names }
    }

    /// The code of the bank `written` names, if it is on the list.
    #[must_use]
    pub fn code_of(&self, written: &str) -> Option<&str> {
        let name = normalize(written);
        self.names
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, c)| c.as_str())
    }
}

static ZENGIN: LazyLock<BankList> =
    LazyLock::new(|| BankList::parse(include_str!("../../data/zengin-banks.tsv")));

/// The list compiled into this build.
pub(super) fn zengin() -> &'static BankList {
    &ZENGIN
}

/// Long forms a letter writes, and the short form the list uses.
const SHORT_FORMS: [(&str, &str); 4] = [
    ("信用金庫", "信金"),
    ("信用組合", "信組"),
    ("労働金庫", "労金"),
    ("農業協同組合", "農協"),
];

/// A bank name as the list compares it: full-width letters and digits
/// made ASCII and upper case, spaces and `株式会社` dropped, a trailing
/// `銀行` dropped, long forms of the other institution types shortened,
/// and `JA` written in front made `農協` behind.
#[must_use]
pub fn normalize(name: &str) -> String {
    let mut s: String = name
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
    if let Some(rest) = s.strip_prefix("JA")
        && !rest.is_empty()
        && !rest.ends_with("農協")
    {
        s = format!("{rest}農協");
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

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# sample\n0005\t三菱ＵＦＪ\n0009\t三井住友\n9900\tゆうちょ\n1001\t北海道信金\n5000\t横浜農協\n";

    #[test]
    fn written_forms_match_the_lists_short_forms() {
        let list = BankList::parse(SAMPLE);
        for (written, code) in [
            ("三菱UFJ銀行", "0005"),
            ("三菱ＵＦＪ銀行", "0005"),
            ("株式会社三菱ＵＦＪ銀行", "0005"),
            ("三菱UFJ", "0005"),
            ("三井住友銀行", "0009"),
            ("ゆうちょ銀行", "9900"),
            ("ゆうちょ", "9900"),
            ("北海道信用金庫", "1001"),
            ("JA横浜", "5000"),
            ("横浜農業協同組合", "5000"),
        ] {
            assert_eq!(list.code_of(written), Some(code), "{written}");
        }
        assert_eq!(list.code_of("三菱銀行"), None);
        assert_eq!(list.code_of("みずほ銀行"), None);
    }

    #[test]
    fn an_empty_list_verifies_nothing() {
        assert_eq!(BankList::parse("# nothing\n").code_of("三菱UFJ銀行"), None);
    }
}
