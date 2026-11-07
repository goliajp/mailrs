use super::*;

const SAMPLE: &str =
    "# sample\n0001\tみずほ\n0005\t三菱ＵＦＪ\n0009\t三井住友\n9900\tゆうちょ\n1001\t北海道信金\n";

fn list() -> BankList {
    BankList::parse(SAMPLE)
}

fn one(text: &str) -> JpAccount {
    let found = find(text, &list());
    assert_eq!(found.len(), 1, "{text}: {found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn one_written_line_is_an_account() {
    let a = one("お振込先：三菱UFJ銀行 渋谷支店 普通 1234567 カ）ゴリア");
    assert!(a.verified);
    assert_eq!(a.masked, "三菱UFJ銀行 渋谷支店 普通 ****567");
}

#[test]
fn an_invoice_block_with_one_label_per_line_is_an_account() {
    let text = "請求書\n\n【振込先】\n銀行名：三井住友銀行\n支店名：新宿\n口座種別：普通預金\n口座番号：７６５４３２１\n口座名義：ゴリア（カ\n";
    let a = one(text);
    assert!(a.verified);
    assert_eq!(a.masked, "三井住友銀行 新宿 普通 ****321");
}

#[test]
fn a_table_flattened_to_one_cell_per_line_is_an_account() {
    let text = "振込先\n銀行名\n三菱ＵＦＪ銀行\n支店名\n渋谷支店\n口座種別\n当座\n口座番号\n1234567\n口座名義\nゴリア";
    let a = one(text);
    assert!(a.verified);
    assert_eq!(a.masked, "三菱UFJ銀行 渋谷支店 当座 ****567");
}

#[test]
fn branch_numbers_abbreviations_and_full_width_forms_are_read() {
    let a = one("みずほ銀行　店番１２３　（普）１２３４５６７");
    assert_eq!(a.masked, "みずほ銀行 店番123 普通 ****567");
    let a = one("北海道信用金庫 本店営業部 ﾌﾂｳ 7654321");
    assert!(a.verified);
    assert_eq!(a.masked, "北海道信用金庫 本店営業部 普通 ****321");
    let a = one("ゆうちょ銀行 〇一八店 普通預金 1234567");
    assert_eq!(a.masked, "ゆうちょ銀行 〇一八店 普通 ****567");
}

#[test]
fn the_number_may_come_before_the_other_parts() {
    let a = one("口座番号 1234567（普通）三菱UFJ銀行 渋谷支店");
    assert_eq!(a.masked, "三菱UFJ銀行 渋谷支店 普通 ****567");
}

#[test]
fn a_bank_not_on_the_list_is_a_suspected_account() {
    let a = one("ミツビシ銀行 渋谷支店 普通 1234567");
    assert!(!a.verified);
    assert_eq!(a.masked, "ミツビシ銀行 渋谷支店 普通 ****567");
    // an empty list verifies nothing
    let found = find("三菱UFJ銀行 渋谷支店 普通 1234567", &BankList::default());
    assert!(!found[0].verified);
}

#[test]
fn the_same_account_written_twice_ways_is_one_key() {
    let a = one("三菱UFJ銀行 渋谷支店 普通 1234567");
    let b = one("三菱ＵＦＪ銀行\n支店名：渋谷\n普通預金\n１２３４５６７");
    assert_eq!(a.key, b.key, "the bank's code and the number identify it");
    let c = one("三菱UFJ銀行 渋谷支店 普通 1234568");
    assert_ne!(a.key, c.key);
}

#[test]
fn two_accounts_in_one_text_are_both_found() {
    let text =
        "三菱UFJ銀行 渋谷支店 普通 1234567\n\n\n\n\n\n\n\n\n\n\nみずほ銀行 新宿支店 当座 7654321";
    let found = find(text, &list());
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn yucho_symbol_and_number_is_an_account() {
    for text in [
        "ゆうちょ銀行\n記号 10120 番号 12345671",
        "ゆうちょ銀行　記号：１０１２０　番号：１２３４５６７１",
        "郵便局 記号番号 10120-12345671",
    ] {
        let a = one(text);
        assert!(a.verified, "{text}");
        assert_eq!(a.masked, "ゆうちょ銀行 記号10120 ****671", "{text}");
    }
    assert_eq!(
        one("ゆうちょ銀行 記号 10120 番号 12345671").key,
        one("郵便局 記号番号 10120-12345671").key
    );
}

#[test]
fn a_yucho_number_without_yucho_named_is_not_an_account() {
    assert!(find("記号 10120 番号 12345671", &list()).is_empty());
    // the symbol starts with 1
    assert!(find("ゆうちょ銀行 記号 20120 番号 12345671", &list()).is_empty());
}

#[test]
fn a_part_missing_is_not_an_account() {
    for text in [
        // no account type
        "三菱UFJ銀行 渋谷支店 1234567",
        // no branch
        "三菱UFJ銀行 普通 1234567",
        // no bank
        "渋谷支店 普通 1234567",
        // eight digits
        "三菱UFJ銀行 渋谷支店 普通 12345678",
        // a phone number, a postcode, an amount
        "三菱UFJ銀行 渋谷支店 普通 TEL 03-1234567",
        "三菱UFJ銀行 渋谷支店 普通 〒150-0001",
        "三菱UFJ銀行 渋谷支店 普通 1,234,567円",
        // an order number with no bank anywhere
        "注文番号 1234567 の件、普通便で発送しました",
        // 当 inside a word is not 当座
        "三菱UFJ銀行 渋谷支店 担当 1234567",
    ] {
        assert!(find(text, &list()).is_empty(), "{text}");
    }
}

#[test]
fn parts_outside_the_window_do_not_pair() {
    let far = format!(
        "三菱UFJ銀行 渋谷支店 普通 のご案内{}口座番号 1234567",
        "\n".repeat(LINES_BEFORE + 1)
    );
    assert!(find(&far, &list()).is_empty());
    let long = format!(
        "三菱UFJ銀行 渋谷支店 普通 {} 1234567",
        "あ".repeat(CHARS_BEFORE)
    );
    assert!(find(&long, &list()).is_empty());
}

#[test]
fn the_full_number_never_leaves_in_the_account() {
    let a = one("三菱UFJ銀行 渋谷支店 普通 1234567");
    let shown = format!("{a:?}");
    assert!(!shown.contains("1234567"), "{shown}");
    assert!(shown.contains("****567"));
    let y = one("ゆうちょ銀行 記号 10120 番号 12345671");
    assert!(!format!("{y:?}").contains("12345671"));
}
