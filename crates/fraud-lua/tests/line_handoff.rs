//! A letter that asks its reader for their LINE QR code.
//!
//! Reported 2026-10-23: `齋藤 真 <ptvmfekd@nogz.cn>`, `ギリア株式会社
//! 業務指示`, SPF, DKIM and DMARC all passing for the throwaway domain,
//! and nothing else in it for an address or header rule to read. The
//! request in the body is the whole case.
use mailrs_fraud::{Facts, Policy};
use mailrs_fraud_lua::{DEFAULT_SOURCE, Rules};

const RULE: &str = "asks-for-the-readers-line";

const REPORTED: &str = "Authentication-Results: mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass;\r\n\tdmarc=pass\r\n\
Reply-To: roselynmuselabarre@zohomail.jp\r\n\
Subject: =?utf-8?B?44Ku44Oq44Ki5qCq5byP5Lya56S+IOalreWLmeaMh+ekug==?=\r\n\
From: =?utf-8?B?6b2L6JekIOecnw==?= <ptvmfekd@nogz.cn>\r\n\
To: contact <contact@golia.jp>\r\n\
Content-Type: text/html; charset=\"UTF-8\"\r\n\
Content-Transfer-Encoding: 8bit\r\n\
\r\n\
<HTML><BODY><SPAN>お疲れ様です。</SPAN><BR>\
<SPAN>ご自身の個人LINEのQRコードまでこのメールアドレス宛に返信してください。</SPAN>\
</BODY></HTML>\r\n";

fn findings(raw: &[u8]) -> mailrs_fraud::Findings {
    let parsed = mailrs_mime::parse(raw);
    let from = mailrs_inbound::from_header(raw);
    let subject = mailrs_inbound::subject_header(raw);
    let facts = Facts {
        from: &from,
        subject: &subject,
        spf: "pass",
        dkim: "pass",
        dmarc: "pass",
        asks_for_the_readers_line: mailrs_inbound::body::asks_for_the_readers_line(&parsed),
        ..Facts::default()
    };
    let mut rules = Rules::compile(DEFAULT_SOURCE).expect("the shipped bundle compiles");
    let out = rules.classify(&facts, &Policy::default());
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    out.findings
}

/// The request is the fraud itself, so it holds the letter for review.
#[test]
fn the_reported_letter_is_held() {
    let f = findings(REPORTED.as_bytes());
    assert!(f.has(RULE), "{:?}", f.rules());
    assert!(f.hold_worthy());
}

/// A shop's newsletter with a LINE QR code to scan asks for nothing.
#[test]
fn a_shop_newsletter_is_not_held() {
    let raw = "From: shop <info@shop.example>\r\nSubject: coupon\r\n\
Content-Type: text/plain; charset=UTF-8\r\n\r\n\
LINE限定クーポン配信中！ ＼友達追加／ QRコードはこちら\r\n";
    assert!(!findings(raw.as_bytes()).has(RULE));
}
