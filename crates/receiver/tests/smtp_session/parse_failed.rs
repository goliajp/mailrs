//! When the parse worker fails, the message is still accepted and goes
//! where it would have gone: a failure on this message is a fact for the
//! rules (a score, never a hold), and a worker that is merely unavailable
//! leaves nothing at all.

use crate::harness::{Client, REAL, SPF_DOMAIN, files_under, receiver_with};

/// An ordinary letter, with the fault header the test backend reads.
fn letter(fault: &str) -> String {
    format!(
        "From: Billing <billing@{SPF_DOMAIN}>\r\nTo: {REAL}\r\nSubject: invoice\r\n\
X-Parse-Worker-Fault: {fault}\r\nContent-Type: text/html\r\n\r\n\
<p>Your invoice for October is attached.</p>\r\n."
    )
}

async fn send(port: u16, body: &str) -> String {
    let mut c = Client::connect(port).await;
    let from = format!("billing@{SPF_DOMAIN}");
    assert!(
        c.say(&format!("MAIL FROM:<{from}>"))
            .await
            .starts_with("250")
    );
    let r = c.say(&format!("RCPT TO:<{REAL}>")).await;
    assert!(r.starts_with("250"), "{r}");
    assert!(c.say("DATA").await.starts_with("354"));
    c.say(body).await
}

fn spooled(dir: &std::path::Path) -> mailrs_core::spool::SpoolEnvelope {
    let files = files_under(dir);
    assert_eq!(files.len(), 1, "one spooled message");
    mailrs_core::spool::decode_spool_blob(&files[0]).unwrap().0
}

#[tokio::test]
async fn a_clean_message_the_parser_failed_on_is_accepted_into_the_inbox_and_scored() {
    mailrs_parse_worker::install_faults("receiver");
    let before = mailrs_parse_worker::health::snapshot().failures_24h;
    let rx = receiver_with("parse-failed-on", None, true).await;
    let r = send(rx.port, &letter("abort")).await;
    assert!(
        r.starts_with("250"),
        "accepted, not refused or deferred: {r}"
    );
    let env = spooled(&rx.spool);
    assert_eq!(env.target_folder, "INBOX", "nothing holds or junks it");
    // The verdict's layers list identity and provenance findings only;
    // a content-layer rule shows as the verdict existing at all, which
    // the same letter read in full does not produce (the last test).
    let verdict = env.fraud_verdict.expect("the finding is recorded");
    assert!(verdict.contains("\"quarantined\":false"), "{verdict}");
    assert!(
        mailrs_parse_worker::health::snapshot().failures_24h > before,
        "the failure is counted"
    );
}

#[tokio::test]
async fn a_worker_that_is_unavailable_leaves_no_fact_and_no_score() {
    mailrs_parse_worker::install_faults("receiver");
    let rx = receiver_with("parse-unavailable", None, true).await;
    let r = send(rx.port, &letter("unavailable")).await;
    assert!(r.starts_with("250"), "{r}");
    let env = spooled(&rx.spool);
    assert_eq!(env.target_folder, "INBOX");
    assert_eq!(env.fraud_verdict, None, "nothing was found");
}

#[tokio::test]
async fn a_message_read_in_full_carries_no_parser_finding() {
    mailrs_parse_worker::install_faults("receiver");
    let rx = receiver_with("parse-read", None, true).await;
    let r = send(rx.port, &letter("none")).await;
    assert!(r.starts_with("250"), "{r}");
    let env = spooled(&rx.spool);
    assert_eq!(env.target_folder, "INBOX");
    assert_eq!(env.fraud_verdict, None, "nothing was found");
}
