//! Where a message's QR codes point, recorded and counted only.
//!
//! The two facts (`mailrs_fraud::qr`) are logged as `event=qr_shadow` and
//! counted per round. No rule reads them and nothing is moved: what they
//! would act on is measured first.

use mailrs_fraud::qr::QrFacts;

/// The QR facts of `raw`, given the codes' addresses and the text read off
/// its attachments. `None` when no code carried a web address.
pub(crate) fn assess(raw: &[u8], texts: &[(usize, String)], urls: &[String]) -> Option<QrFacts> {
    if urls.is_empty() {
        return None;
    }
    let parsed = mailrs_mime::parse(raw);
    let from = mailrs_inbound::identity::from_address(raw).to_ascii_lowercase();
    let sender_host = from.rsplit_once('@').map(|(_, d)| d).unwrap_or_default();
    let links = mailrs_inbound::scan_input::link_hosts(&parsed);
    let image_text: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
    let facts = mailrs_fraud::qr::assess(urls, sender_host, &links, &image_text.join("\n"), |d| {
        crate::ledger::NEVER.contains(d)
    });
    (facts.codes > 0).then_some(facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_off_the_message_is_seen_and_no_code_is_nothing() {
        let raw = b"From: Bank <info@bank.example>\r\nSubject: x\r\n\r\nscan the code\r\n";
        let urls = ["https://login.other.example/x".to_string()];
        let texts = [(0, "www.bank.example".to_string())];
        let f = assess(raw, &texts, &urls).expect("a code");
        assert!(f.host_not_in_message);
        assert!(f.text_mismatch);
        assert_eq!(assess(raw, &texts, &[]), None);
    }
}
