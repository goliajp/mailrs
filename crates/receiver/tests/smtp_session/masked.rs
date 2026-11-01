use mailrs_core_sidestate::families::masked_email::submit_log;
use mailrs_core_sidestate::keys::admin::AUDIT_LOG;

use crate::harness::{Client, MASK, ME, files_under, receiver};

/// Send one message signed in as [`ME`] with the mask in `From:`; the
/// final reply.
async fn send_from_mask(port: u16) -> String {
    let mut c = Client::signed_in(port).await;
    assert!(c.say(&format!("MAIL FROM:<{ME}>")).await.starts_with("250"));
    assert!(
        c.say("RCPT TO:<shop@remote.example>")
            .await
            .starts_with("250")
    );
    assert!(c.say("DATA").await.starts_with("354"));
    c.say(&format!(
        "From: Shopper <{MASK}>\r\nTo: shop@remote.example\r\nSubject: order\r\n\r\nwhere is it\r\n."
    ))
    .await
}

#[tokio::test]
async fn a_submission_from_a_mask_is_logged_apart_and_names_no_client() {
    let url = format!("mem://mask-submit-{}", std::process::id());
    // held for the whole test: the in-process store goes with its last
    // connection
    let mut kv = kevy_client::Connection::connect(&url).unwrap();
    let rx = receiver("mask-submit", Some(&url)).await;
    let r = send_from_mask(rx.port).await;
    assert!(r.starts_with("250"), "{r}");

    let spooled = files_under(&rx.spool);
    assert_eq!(spooled.len(), 1, "one spooled message");
    let (envelope, body) = mailrs_core::spool::decode_spool_blob(&spooled[0]).unwrap();
    assert_eq!(envelope.reverse_path, MASK, "queued under the mask");
    let text = String::from_utf8_lossy(body);
    let head = text.split("\r\n\r\n").next().unwrap();
    assert!(head.contains("Received: by mx.golia.test"), "{head}");
    assert!(!head.contains("127.0.0.1"), "the client IP leaked:\n{head}");
    assert!(
        !head.contains("probe-host"),
        "the EHLO name leaked:\n{head}"
    );
    assert!(!head.contains(ME), "the account leaked:\n{head}");

    let rows = submit_log::recent(&mut kv, 10).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].account, ME);
    assert_eq!(rows[0].mask, "amber.ot");
    assert_eq!(rows[0].ip, "127.0.0.1");
    assert!(rows[0].at > 0);
    assert_eq!(
        kv.llen(AUDIT_LOG).unwrap(),
        0,
        "the admin audit log is not used"
    );
    assert_eq!(rx.counts.log_failed(), 0);
}

#[tokio::test]
async fn a_log_that_cannot_be_written_is_counted_and_the_mail_goes_on() {
    // nothing listens on port 1
    let rx = receiver("mask-submit-down", Some("kevy://127.0.0.1:1")).await;
    let r = send_from_mask(rx.port).await;
    assert!(r.starts_with("250"), "the mail is accepted: {r}");
    assert_eq!(files_under(&rx.spool).len(), 1);
    assert_eq!(rx.counts.log_failed(), 1);
}
