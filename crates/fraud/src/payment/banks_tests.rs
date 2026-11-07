use super::*;

const NAMES: &str = include_str!("../../data/jp-bank-names.tsv");
const ALIASES: &str = include_str!("../../data/jp-bank-aliases.tsv");

#[test]
fn written_forms_match_the_listed_names() {
    let list = listed();
    for (written, canonical) in [
        ("三菱UFJ銀行", "三菱UFJ"),
        ("三菱ＵＦＪ銀行", "三菱UFJ"),
        ("株式会社三菱ＵＦＪ銀行", "三菱UFJ"),
        ("三菱UFJ銀行株式会社", "三菱UFJ"),
        ("三菱UFJ", "三菱UFJ"),
        ("(株)三菱UFJ銀行", "三菱UFJ"),
        ("（株）三菱UFJ銀行", "三菱UFJ"),
        ("三井住友銀行", "三井住友"),
        ("みずほ銀行", "みずほ"),
        ("PayPay銀行", "PAYPAY"),
        ("ＰａｙＰａｙ銀行", "PAYPAY"),
        ("楽天銀行", "楽天"),
        ("旭川信用金庫", "旭川信金"),
        ("旭川信金", "旭川信金"),
        ("ウリ信用組合", "ウリ信組"),
        ("中央労働金庫", "中央労金"),
        ("ｽﾙｶﾞ銀行", "スルガ"),
        ("ゆうちょ銀行", "ゆうちょ"),
        ("ゆうちょ", "ゆうちょ"),
        ("信金中央金庫", "信金中央金庫"),
    ] {
        assert_eq!(
            list.canonical(written).as_deref(),
            Some(canonical),
            "{written}"
        );
    }
}

#[test]
fn a_former_name_is_the_bank_it_became() {
    let list = listed();
    for (former, current) in [
        ("三菱東京UFJ銀行", "三菱UFJ銀行"),
        ("東京三菱銀行", "三菱UFJ銀行"),
        ("新生銀行", "SBI新生銀行"),
        ("ジャパンネット銀行", "PayPay銀行"),
        ("八十二銀行", "八十二長野銀行"),
    ] {
        assert_eq!(list.canonical(former), list.canonical(current), "{former}");
        assert!(list.canonical(former).is_some(), "{former}");
    }
}

#[test]
fn every_alias_points_at_a_listed_bank_and_no_former_name_is_listed() {
    let names = BankList::parse(NAMES, "");
    let mut seen = 0;
    for line in ALIASES.lines().filter(|l| !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [former, current, year] = cols[..] else {
            panic!("alias line is not former<TAB>current<TAB>year: {line}");
        };
        assert!(
            names.canonical(current).is_some(),
            "{current} is not listed"
        );
        assert!(
            names.canonical(former).is_none(),
            "{former} is listed itself"
        );
        assert!(
            year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()),
            "{line}"
        );
        seen += 1;
    }
    assert!(seen >= 20, "read {seen} aliases");
}

#[test]
fn nothing_looser_than_normalising_matches() {
    let list = listed();
    for written in [
        // a letter off, a letter swapped
        "三菱UJF銀行",
        "三菱UF銀行",
        "みずほ銀",
        // agricultural cooperatives are not on the list
        "JA横浜",
        "横浜農業協同組合",
        // a bank name the list never had
        "ミツビシ銀行",
    ] {
        assert_eq!(list.canonical(written), None, "{written}");
    }
}

#[test]
fn the_snapshot_holds_every_kind_it_is_made_from() {
    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for line in NAMES.lines().filter(|l| !l.starts_with('#')) {
        *kinds
            .entry(line.split('\t').next().unwrap_or(""))
            .or_default() += 1;
    }
    for kind in ["銀行", "外国銀行支店", "信用金庫", "信用組合", "労働金庫"] {
        assert!(kinds.get(kind).is_some_and(|&n| n > 0), "{kind}: {kinds:?}");
    }
    assert!(listed().len() > 500, "{}", listed().len());
}

#[test]
fn half_width_katakana_folds_with_its_marks() {
    assert_eq!(fold_kana("ｶﾞｲｺｸ ﾊﾟﾘﾊﾞ ｳﾞｪ･ﾄﾞｲﾂ"), "ガイコク パリバ ヴェ・ドイツ");
    assert!(BankList::default().is_empty());
}
