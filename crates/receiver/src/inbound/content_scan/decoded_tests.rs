//! The body rules read the text the parse worker decodes, not the bytes
//! on the wire.

use base64::Engine as _;

use super::*;

/// A message with one base64 text part, its body text read the way the
/// receiver reads it.
fn base64_message(content_type: &str, text: &str) -> (Vec<u8>, Option<String>) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let mut wrapped = String::new();
    for line in encoded.as_bytes().chunks(76) {
        wrapped.push_str(std::str::from_utf8(line).unwrap());
        wrapped.push_str("\r\n");
    }
    let raw = format!(
        "From: news@shop.example\r\nSubject: Weekly picks\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
         Message-ID: <1@shop.example>\r\nMIME-Version: 1.0\r\n\
         Content-Type: {content_type}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{wrapped}"
    )
    .into_bytes();
    let body = mailrs_parse_worker::body_text(&raw, mailrs_parse_worker::Limit::Inbound)
        .read()
        .flatten();
    (raw, body)
}

fn rules_of(raw: &[u8], body_text: Option<&str>) -> Vec<String> {
    evaluate_rules(&Message { raw, body_text }).rules
}

#[test]
fn a_base64_newsletter_in_ordinary_case_is_not_shouting() {
    let prose = "Thanks for reading our weekly letter. This week we picked three new \
                 kitchen knives, a coffee grinder and a set of linen towels for spring. \
                 Every item ships free within the country, and members get early access \
                 to the sale that starts on Friday morning. ";
    let (raw, body) = base64_message("text/plain", &prose.repeat(2));
    // the wire bytes alone are capital-heavy enough to read as shouting
    let wire = String::from_utf8_lossy(&raw);
    let after_head = wire.split_once("\r\n\r\n").unwrap().1;
    assert!(is_shouting(after_head, 200, 3, 10), "base64 reads as caps");

    let body = body.expect("the parse worker decodes the part");
    assert!(body.contains("kitchen knives"));
    assert!(!rules_of(&raw, Some(&body)).contains(&"shouty_body".to_string()));
}

#[test]
fn a_decoded_body_that_really_shouts_is_caught() {
    let shout = "FREE CASH FOR EVERY WINNER TODAY ONLY CLICK THE LINK AND CLAIM YOUR PRIZE NOW ";
    let (raw, body) = base64_message("text/plain", &shout.repeat(5));
    let body = body.expect("the parse worker decodes the part");
    assert!(rules_of(&raw, Some(&body)).contains(&"shouty_body".to_string()));
}

#[test]
fn no_body_text_means_the_body_rule_adds_nothing() {
    let shout = "FREE CASH FOR EVERY WINNER TODAY ONLY CLICK THE LINK AND CLAIM YOUR PRIZE NOW ";
    let raw = format!("From: a@b.example\r\nSubject: x\r\n\r\n{}", shout.repeat(5));
    assert!(!rules_of(raw.as_bytes(), None).contains(&"shouty_body".to_string()));
}

#[test]
fn the_format_rules_are_counted_apart() {
    let mut raw = String::from(
        "From: a@b.example\r\nSubject: picks\r\nDate: Mon, 01 Jan 2024 00:00:00 +0000\r\n\
         Message-ID: <1@b.example>\r\nContent-Type: text/html\r\n\r\n",
    );
    for i in 0..8 {
        raw.push_str(&format!("<a href=\"https://b.example/{i}\">x</a> "));
    }
    // a format rule and a content rule: only the first counts as format
    raw.push_str("$10 off, limited time");
    let r = evaluate_rules(&Message {
        raw: raw.as_bytes(),
        body_text: Some("$10 off, limited time"),
    });
    assert_eq!(
        r.rules,
        ["html_only_no_text", "excessive_urls", "money_urgency"]
    );
    assert_eq!(r.format_score, 3.5);
    assert_eq!(r.score, 5.5);
}
