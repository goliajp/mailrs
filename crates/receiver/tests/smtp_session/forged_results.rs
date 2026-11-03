use crate::harness::{Client, REAL, SPF_DOMAIN, files_under, receiver};

/// A sender cannot hand in an `Authentication-Results:` that says it is
/// this server's: the rules would read `dkim=pass header.d=stripe.com`
/// in it as our own finding.
#[tokio::test]
async fn a_results_header_naming_us_is_removed_and_others_are_kept() {
    let rx = receiver("forged-results", None).await;
    let mut c = Client::connect(rx.port).await;
    let from = format!("billing@{SPF_DOMAIN}");
    assert!(
        c.say(&format!("MAIL FROM:<{from}>"))
            .await
            .starts_with("250")
    );
    let r = c.say(&format!("RCPT TO:<{REAL}>")).await;
    assert!(r.starts_with("250"), "{r}");
    assert!(c.say("DATA").await.starts_with("354"));
    let r = c
        .say(&format!(
            "Authentication-Results: mx.golia.test;\r\n\tdkim=pass header.d=stripe.com header.s=s1\r\n\
Authentication-Results: MX.GOLIA.TEST; spf=pass\r\n\
Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com\r\n\
From: Billing <{from}>\r\nTo: {REAL}\r\nSubject: invoice\r\n\r\n\
Authentication-Results: mx.golia.test; quoted in the body\r\n."
        ))
        .await;
    assert!(r.starts_with("250"), "{r}");

    let spooled = files_under(&rx.spool);
    assert_eq!(spooled.len(), 1, "one spooled message");
    let (_, body) = mailrs_core::spool::decode_spool_blob(&spooled[0]).unwrap();
    let text = String::from_utf8_lossy(body);
    let (head, rest) = text.split_once("\r\n\r\n").unwrap();
    assert!(
        !head
            .to_ascii_lowercase()
            .contains("authentication-results: mx.golia.test"),
        "a header naming this host survived:\n{head}"
    );
    assert!(
        !head.contains("header.s=s1"),
        "a continuation line of the forged header survived:\n{head}"
    );
    assert!(
        head.contains("Authentication-Results: mx.google.com; dkim=pass header.d=stripe.com"),
        "another server's header is not ours to remove:\n{head}"
    );
    assert!(
        rest.contains("Authentication-Results: mx.golia.test; quoted in the body"),
        "the body is not a header:\n{rest}"
    );
}
