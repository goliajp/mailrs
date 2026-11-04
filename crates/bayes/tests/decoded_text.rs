//! The tokenizer reads what a person reads: the decoded body text and the
//! decoded subject, not the transfer encoding, the markup or the CSS.
//! Every message here is synthetic.

use base64::Engine;
use mailrs_bayes::tokenize;

fn b64(s: &[u8]) -> String {
    let enc = base64::engine::general_purpose::STANDARD.encode(s);
    // wrapped at 76 like a real mailer, so the lines split the way they do
    // in transit
    enc.as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn has(toks: &[String], t: &str) -> bool {
    toks.iter().any(|x| x == t)
}

/// Body word tokens: the ones with no namespace prefix.
fn body_words(toks: &[String]) -> Vec<&String> {
    toks.iter()
        .filter(|t| !(t.starts_with("hdr:") || t.starts_with("from:")))
        .filter(|t| !(t.starts_with("sub:") || t.starts_with("url:")))
        .collect()
}

/// No ASCII body word is a piece of the encoded payload.
fn assert_no_fragments_of(toks: &[String], encoded: &str) {
    let lc = encoded.to_lowercase();
    for w in body_words(toks) {
        if w.is_ascii() {
            assert!(
                !lc.contains(w.as_str()),
                "body word {w:?} is a fragment of the encoded payload"
            );
        }
    }
}

const JA: &str = "お支払いが完了しました。ご利用ありがとうございます。";

#[test]
fn a_base64_japanese_body_yields_japanese_bigrams() {
    let payload = b64(JA.as_bytes());
    let raw = format!(
        "From: shop@pay.example\r\nSubject: receipt\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{payload}\r\n"
    );
    let toks = tokenize(raw.as_bytes());
    for t in ["支払", "完了", "利用"] {
        assert!(has(&toks, t), "missing bigram {t}: {toks:?}");
    }
    assert_no_fragments_of(&toks, &payload);
}

#[test]
fn a_quoted_printable_japanese_body_yields_japanese_bigrams() {
    let raw = "From: shop@pay.example\r\nSubject: receipt\r\nMIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: quoted-printable\r\n\r\n\
         =E3=81=8A=E6=94=AF=E6=89=95=E3=81=84=E3=81=8C=E5=AE=8C=E4=BA=86=E3=81=97=E3=\r\n\
         =81=BE=E3=81=97=E3=81=9F=E3=80=82\r\n";
    let toks = tokenize(raw.as_bytes());
    for t in ["支払", "完了"] {
        assert!(has(&toks, t), "missing bigram {t}: {toks:?}");
    }
    assert!(
        !toks.iter().any(|t| t.starts_with("e3") || t == "81"),
        "quoted-printable escapes leaked: {toks:?}"
    );
}

#[test]
fn an_iso_2022_jp_body_is_decoded() {
    let mut raw = b"From: a@b.example\r\nSubject: hi\r\nMIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=ISO-2022-JP\r\n\
Content-Transfer-Encoding: 7bit\r\n\r\n"
        .to_vec();
    // "こんにちは、お元気ですか" in ISO-2022-JP
    raw.extend_from_slice(b"\x1b$B$3$s$K$A$O!\"$*855$$G$9$+\x1b(B\r\n");
    let toks = tokenize(&raw);
    for t in ["こん", "元気"] {
        assert!(has(&toks, t), "missing bigram {t}: {toks:?}");
    }
}

#[test]
fn a_shift_jis_body_is_decoded() {
    let mut raw = b"From: a@b.example\r\nSubject: hi\r\nMIME-Version: 1.0\r\n\
Content-Type: text/plain; charset=Shift_JIS\r\n\
Content-Transfer-Encoding: 8bit\r\n\r\n"
        .to_vec();
    // "ご請求金額のお知らせ" in Shift_JIS
    raw.extend_from_slice(
        b"\x82\xb2\x90\xbf\x8b\x81\x8b\xe0\x8a\x7a\x82\xcc\x82\xa8\x92\x6d\x82\xe7\x82\xb9\r\n",
    );
    let toks = tokenize(&raw);
    for t in ["請求", "金額", "知ら"] {
        assert!(has(&toks, t), "missing bigram {t}: {toks:?}");
    }
}

const CSS_WORDS: [&str; 6] = [
    "border-spacing",
    "inherit",
    "table-layout",
    "bicubic",
    "font-family",
    "collapse",
];

#[test]
fn style_sheet_properties_are_not_words() {
    let raw = b"From: a@b.example\r\nSubject: hi\r\nMIME-Version: 1.0\r\n\
Content-Type: text/html; charset=utf-8\r\n\r\n\
<html><head><style>table{border-spacing:0;table-layout:fixed;border-collapse:collapse}\
td{color:inherit;font-family:sans-serif}img{-ms-interpolation-mode:bicubic}</style></head>\
<body><!-- tracking comment --><script>var trackme=1;</script>\
<p>Your order shipped today</p></body></html>\r\n";
    let toks = tokenize(raw);
    for t in CSS_WORDS {
        assert!(!has(&toks, t), "css token {t} survived: {toks:?}");
    }
    assert!(!has(&toks, "trackme"), "script text survived: {toks:?}");
    assert!(!has(&toks, "tracking"), "comment text survived: {toks:?}");
    assert!(has(&toks, "shipped"), "visible text lost: {toks:?}");
}

#[test]
fn an_encoded_word_subject_is_decoded_before_splitting() {
    // "お知らせ" in base64 UTF-8
    let raw = b"From: a@b.example\r\nSubject: =?UTF-8?B?44GK55+l44KJ44Gb?=\r\n\r\nhello there\r\n";
    let toks = tokenize(raw);
    for t in ["sub:お知", "sub:知ら", "sub:らせ"] {
        assert!(has(&toks, t), "missing {t}: {toks:?}");
    }
    assert!(
        !toks
            .iter()
            .any(|t| t.starts_with("sub:utf-8") || t.starts_with("sub:44g")),
        "encoded-word syntax leaked into the subject tokens: {toks:?}"
    );
}

#[test]
fn an_attachment_only_message_yields_no_body_words() {
    // binary-looking payload with short runs between '+' and '/'
    let payload = b64(&(0u8..=255).cycle().take(900).collect::<Vec<u8>>());
    let raw = format!(
        "From: a@b.example\r\nSubject: scan\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n\
         --b1\r\nContent-Type: application/pdf; name=\"scan.pdf\"\r\n\
         Content-Disposition: attachment; filename=\"scan.pdf\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{payload}\r\n--b1--\r\n"
    );
    let toks = tokenize(raw.as_bytes());
    assert!(
        body_words(&toks).is_empty(),
        "an attachment produced body words: {:?}",
        body_words(&toks)
    );
    assert!(has(&toks, "sub:scan"));
    assert!(has(&toks, "hdr:ct:multipart/mixed"));
}

#[test]
fn a_text_attachment_is_not_read_as_the_body() {
    let raw = b"From: a@b.example\r\nSubject: notes\r\nMIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n\
--b1\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nplease see attached\r\n\
--b1\r\nContent-Type: text/plain; name=\"log.txt\"\r\n\
Content-Disposition: attachment; filename=\"log.txt\"\r\n\r\nstacktrace overflow\r\n\
--b1--\r\n";
    let toks = tokenize(raw);
    assert!(has(&toks, "attached"));
    assert!(
        !has(&toks, "stacktrace"),
        "attachment text tokenized: {toks:?}"
    );
}

/// The shape of a payment app's HTML newsletter: no text part, table
/// layout, a head style sheet and inline styles, a dozen links, Japanese
/// copy, all base64. A fictional sender; no real message.
fn promo_newsletter() -> Vec<u8> {
    let mut rows = String::new();
    for i in 0..12 {
        rows.push_str(&format!(
            "<tr><td style=\"padding:0;border-spacing:0;table-layout:fixed;color:inherit\">\
             <a href=\"https://campaign.examplepay.example/c/{i}?u=a6s8923\" \
             style=\"color:inherit;text-decoration:none\">\
             <img src=\"https://img.examplepay.example/b/{i}.png\" width=\"600\" \
             style=\"display:block;-ms-interpolation-mode:bicubic\" alt=\"\"></a></td></tr>"
        ));
    }
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <style type=\"text/css\">body{{margin:0;padding:0}}\
         table{{border-spacing:0;border-collapse:collapse;table-layout:fixed}}\
         td{{font-family:'Hiragino Sans',sans-serif;color:inherit}}\
         img{{border:0;-ms-interpolation-mode:bicubic}}\
         @media only screen and (max-width:600px){{.wrap{{width:100%!important}}}}</style></head>\
         <body><table width=\"600\" cellpadding=\"0\" cellspacing=\"0\" \
         style=\"border-spacing:0;table-layout:fixed\">\
         <tr><td style=\"font-family:sans-serif;color:inherit\">\
         今月のキャンペーンのお知らせです。ポイント還元をお楽しみください。</td></tr>\
         {rows}\
         <tr><td><a href=\"https://www.examplepay.example/unsubscribe\">配信停止</a></td></tr>\
         </table></body></html>"
    );
    let payload = b64(html.as_bytes());
    format!(
        "From: ExamplePay <news@mail.examplepay.example>\r\n\
         Subject: =?UTF-8?B?{}?=\r\n\
         List-Unsubscribe: <https://www.examplepay.example/unsubscribe>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/html; charset=\"UTF-8\"\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{payload}\r\n",
        base64::engine::general_purpose::STANDARD.encode("キャンペーンのお知らせ")
    )
    .into_bytes()
}

#[test]
fn an_html_only_promo_newsletter_yields_its_words_and_no_css() {
    let raw = promo_newsletter();
    let toks = tokenize(&raw);
    for t in CSS_WORDS {
        assert!(!has(&toks, t), "css token {t} survived: {toks:?}");
    }
    for t in ["sans-serif", "important", "padding", "margin", "display"] {
        assert!(!has(&toks, t), "css token {t} survived: {toks:?}");
    }
    for t in ["還元", "配信", "sub:お知"] {
        assert!(has(&toks, t), "missing {t}: {toks:?}");
    }
    for t in [
        "url:campaign.examplepay.example",
        "url:www.examplepay.example",
        "from:mail.examplepay.example",
        "hdr:list-unsub",
        "hdr:ct:text/html",
    ] {
        assert!(has(&toks, t), "missing {t}: {toks:?}");
    }
}
