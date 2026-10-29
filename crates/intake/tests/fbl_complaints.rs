//! An ARF complaint suppresses an address only when the message it
//! carries is one we sent to that address, and one reporter cannot
//! suppress more than its daily limit. Against a real kevy-server: the
//! send record is written with a pipeline, which is what production reads.

use mailrs_core_sidestate::families::outbound::PENDING_IDX;
use mailrs_core_sidestate::families::send::{self, SendRow, Status};
use mailrs_core_sidestate::families::suppression;
use mailrs_intake::reports::fbl::maybe_record_complaint;

const SENDER: &str = "qa@golia.jp";
const MESSAGE_ID: &str = "abc123@golia.jp";

fn report(reporter: &str, complainant: &str, original: &str) -> Vec<u8> {
    format!(
        "From: Feedback <fbl@{reporter}>\r\n\
         To: abuse@golia.jp\r\n\
         Content-Type: multipart/report; report-type=feedback-report; boundary=\"b\"\r\n\r\n\
         --b\r\nContent-Type: text/plain\r\n\r\nA complaint.\r\n\
         --b\r\nContent-Type: message/feedback-report\r\n\r\n\
         Feedback-Type: abuse\r\nUser-Agent: test/1\r\nVersion: 1\r\n\
         Original-Rcpt-To: <{complainant}>\r\n\r\n\
         --b\r\n{original}\
         --b--\r\n"
    )
    .into_bytes()
}

fn ours(message_id: &str) -> String {
    format!(
        "Content-Type: message/rfc822\r\n\r\n\
         From: Qa <{SENDER}>\r\nMessage-ID: <{message_id}>\r\nSubject: news\r\n\r\nbody\r\n"
    )
}

fn sent(conn: &mut kevy_client::Connection, message_id: &str, to: &[String]) {
    let row = SendRow {
        send_id: message_id.into(),
        message_id: message_id.into(),
        thread_id: message_id.into(),
        subject: "news".into(),
        to_csv: to.join(","),
        cc_csv: String::new(),
        created_at: 1,
        status: Status::Delivered,
        envelope_ref: String::new(),
        resent_from: None,
        key_id: None,
    };
    send::write_send(conn, SENDER, &row, to).expect("send record");
}

fn audit_actions(conn: &mut kevy_client::Connection) -> Vec<String> {
    conn.lrange(mailrs_core_sidestate::keys::admin::AUDIT_LOG, 0, -1)
        .expect("audit log")
        .iter()
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
        .filter_map(|v| v["action"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
async fn only_a_complaint_about_our_own_message_suppresses_and_a_reporter_is_capped() {
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    // SAFETY: this binary holds only this test, so nothing reads the
    // environment concurrently.
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &url);
        std::env::set_var("MAILRS_SECURITY_ALERT_TO", "devops@golia.jp");
        std::env::set_var("MAILRS_SECURITY_ALERT_FROM", "noreply@golia.jp");
    }
    let maildir = std::env::temp_dir().join(format!("fbl-test-{}", std::process::id()));
    let root = maildir.to_string_lossy().to_string();

    tokio::task::spawn_blocking(move || {
        let mut conn = kevy_client::Connection::connect(&url).expect("kevy");
        let victims: Vec<String> = (0..25)
            .map(|i| format!("reader{i}@outside.example"))
            .collect();
        sent(&mut conn, MESSAGE_ID, &victims);

        // the report's word alone: a forged report about an address we
        // never wrote to, one with no original, one about a message we
        // never sent
        maybe_record_complaint(
            &root,
            "abuse@golia.jp",
            &report(
                "forger.example",
                "target@outside.example",
                &ours(MESSAGE_ID),
            ),
        );
        maybe_record_complaint(
            &root,
            "abuse@golia.jp",
            &report("forger.example", "reader0@outside.example", ""),
        );
        maybe_record_complaint(
            &root,
            "abuse@golia.jp",
            &report(
                "forger.example",
                "reader0@outside.example",
                &ours("never@golia.jp"),
            ),
        );
        assert!(!suppression::is_suppressed(
            &mut conn,
            "target@outside.example"
        ));
        assert!(!suppression::is_suppressed(
            &mut conn,
            "reader0@outside.example"
        ));

        // a real complaint suppresses, as a complaint, not a bounce
        maybe_record_complaint(
            &root,
            "postmaster@golia.jp",
            &report("fbl.example", "Reader0@outside.example", &ours(MESSAGE_ID)),
        );
        assert!(suppression::is_suppressed(
            &mut conn,
            "reader0@outside.example"
        ));
        let source = conn
            .hget(b"mailrs:suppress:reader0@outside.example", b"source")
            .unwrap()
            .unwrap();
        assert_eq!(source, b"complaint");

        // one reporter, past its daily limit, suppresses no more and
        // raises exactly one alert
        for v in &victims[1..] {
            maybe_record_complaint(
                &root,
                "abuse@golia.jp",
                &report("fbl.example", v, &ours(MESSAGE_ID)),
            );
        }
        let suppressed = victims
            .iter()
            .filter(|v| suppression::is_suppressed(&mut conn, v))
            .count();
        assert_eq!(suppressed, 20);
        let pending = conn.lrange(PENDING_IDX, 0, -1).unwrap();
        let alerts = pending
            .iter()
            .filter_map(|id| {
                let id = String::from_utf8_lossy(id).to_string();
                conn.hget(
                    mailrs_core_sidestate::families::outbound::job_key(&id).as_bytes(),
                    b"blob",
                )
                .ok()
                .flatten()
            })
            .filter(|blob| {
                String::from_utf8_lossy(blob).contains("\"recipient\":\"devops@golia.jp\"")
            })
            .count();
        assert_eq!(alerts, 1, "one alert for one burst");

        let actions = audit_actions(&mut conn);
        let count = |a: &str| actions.iter().filter(|x| *x == a).count();
        assert_eq!(count("fbl.report"), 28, "every report is audited");
        assert_eq!(count("fbl.suppressed"), 20);
        assert_eq!(count("fbl.rate_limited"), 5);

        // an operator sees each one and what put it there, and can take
        // one off or all of them
        let listed = suppression::list(&mut conn).unwrap();
        assert_eq!(listed.len(), 20);
        assert!(
            listed
                .iter()
                .all(|e| e.source == "complaint" && e.reason.contains("fbl.example"))
        );
        assert!(suppression::remove(&mut conn, "reader0@outside.example").unwrap());
        assert!(!suppression::is_suppressed(
            &mut conn,
            "reader0@outside.example"
        ));
        assert_eq!(suppression::clear(&mut conn).unwrap(), 19);
        assert!(suppression::list(&mut conn).unwrap().is_empty());
    })
    .await
    .unwrap();
}
