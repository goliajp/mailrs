use super::*;

const TEMPLATE: &str = "Dear {name}, your parcel {n} could not be delivered. \
    Pay the customs fee of {n} yen at https://parcel-desk.example/pay?id={n}&to={addr} \
    within 24 hours, otherwise the parcel is returned to the sender. \
    Reference number {n}. Thank you for using our delivery service.";

fn copy(name: &str, addr: &str, n: &str) -> String {
    TEMPLATE
        .replace("{name}", name)
        .replace("{addr}", addr)
        .replace("{n}", n)
}

fn h(text: &str, recipient: &[&str]) -> u64 {
    of_text(text, recipient).expect("enough text for a fingerprint")
}

#[test]
fn copies_for_different_people_stay_within_three_bits() {
    let a = h(
        &copy("Alice Smith", "alice@a.example", "40291"),
        &["alice@a.example", "alice", "Alice Smith"],
    );
    let b = h(
        &copy("Bob Jones", "bob@b.example", "77"),
        &["bob@b.example", "bob", "Bob Jones"],
    );
    assert!(
        distance(a, b) <= crate::MAX_DISTANCE,
        "distance {}",
        distance(a, b)
    );
}

#[test]
fn a_different_message_is_far_away() {
    let a = h(&copy("Alice", "alice@a.example", "1"), &["alice"]);
    let other = h(
        "The quarterly board meeting moves to Thursday afternoon. Please bring the \
         revised budget, the hiring plan and your notes on the vendor contract.",
        &[],
    );
    assert!(
        distance(a, other) > crate::MAX_DISTANCE,
        "distance {}",
        distance(a, other)
    );
}

#[test]
fn a_link_keeps_only_its_host() {
    let a = h(&copy("x", "x@x.example", "1"), &[]);
    let b = h(
        &copy("x", "x@x.example", "1").replace("/pay?id=1&to=x@x.example", "/other/path?q=zzz"),
        &[],
    );
    assert_eq!(a, b);
}

#[test]
fn japanese_copies_hash_alike_and_differ_from_other_japanese() {
    let t = |name: &str, n: &str| {
        format!(
            "{name}様 お客様のアカウントは{n}日後に停止されます。下記のリンクから本人確認を行ってください。確認が取れない場合はご利用いただけなくなります。"
        )
    };
    let a = h(&t("山田", "3"), &["山田"]);
    let b = h(&t("佐藤", "7"), &["佐藤"]);
    assert!(
        distance(a, b) <= crate::MAX_DISTANCE,
        "distance {}",
        distance(a, b)
    );
    let other = h(
        "来週の定例会議は木曜日の午後に変更になりました。資料は前日までに共有フォルダへ置いてください。",
        &[],
    );
    assert!(distance(a, other) > crate::MAX_DISTANCE);
}

#[test]
fn a_short_body_has_no_fingerprint() {
    assert_eq!(of_text("Hi, see you soon", &[]), None);
    assert_eq!(of_text("", &[]), None);
}

#[test]
fn an_html_body_is_read_without_its_tags_and_styles() {
    let text = copy("A", "a@a.example", "1");
    let html = format!(
        "<html><head><style>.x{{color:red}} p{{margin:0}}</style></head><body><p>{}</p>\
         <script>var tracking = 'abc def ghi';</script></body></html>",
        text.replace(". ", ".</p><p>")
    );
    let raw = format!(
        "From: a@evil.example\r\nTo: a@a.example\r\nSubject: s\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/html; charset=utf-8\r\n\r\n{html}"
    );
    let from_html = of_message(raw.as_bytes(), &[]).expect("fingerprint");
    assert!(distance(from_html, h(&text, &[])) <= 1);
}

#[test]
fn a_plain_part_is_preferred_over_html() {
    let text = copy("A", "a@a.example", "1");
    let raw = format!(
        "From: a@evil.example\r\nTo: a@a.example\r\nSubject: s\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/alternative; boundary=b\r\n\r\n--b\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\n{text}\r\n--b\r\n\
         Content-Type: text/html; charset=utf-8\r\n\r\n<p>something else entirely here with many words of its own</p>\r\n--b--\r\n"
    );
    assert_eq!(of_message(raw.as_bytes(), &[]), Some(h(&text, &[])));
}

#[test]
fn segments_cover_the_hash_and_a_three_bit_change_keeps_one() {
    let a: u64 = 0x0123_4567_89ab_cdef;
    let s = segments(a);
    assert_eq!(s, [0xcdef, 0x89ab, 0x4567, 0x0123]);
    // one bit flipped in each of three pieces: the fourth still matches
    let b = a ^ (1 | (1 << 16) | (1 << 32));
    let shared = segments(a)
        .iter()
        .zip(segments(b))
        .filter(|(x, y)| **x == *y)
        .count();
    assert_eq!(shared, 1);
}
