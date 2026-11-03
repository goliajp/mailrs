//! Sends refused before anything is filed or queued:
//!
//! - a value written into a header as it is carries a line break, which
//!   would end that header and start another of the sender's choosing,
//!   such as a Bcc nobody sees in the composer (400, on the JSON form and
//!   the multipart form alike);
//! - a forward whose original cannot be found, which would otherwise go
//!   out as the user's leading line alone (404).

mod live;

use reqwest::Method;
use serde_json::{Value, json};

const KEVY: &str = "mem://webapi-refused-sends";
const ALICE: &str = "alice@example.test";

fn sent_files(root: &std::path::Path) -> usize {
    let cur = root.join("example.test/alice/.Sent/cur");
    std::fs::read_dir(cur).map(|d| d.count()).unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_that_cannot_be_what_it_says_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;

    let base = json!({
        "from": ALICE, "to": ["bob@partner.test"], "subject": "hi", "body": "x",
    });
    let cases: [(&str, Value); 5] = [
        ("to", json!(["bob@partner.test\r\nBcc: eve@evil.test"])),
        ("cc", json!(["carol@partner.test\nBcc: eve@evil.test"])),
        ("bcc", json!(["dave@partner.test\rX-Evil: 1"])),
        ("subject", json!("hi\r\nBcc: eve@evil.test")),
        (
            "in_reply_to",
            json!("parent@partner.test>\r\nBcc: <eve@evil.test"),
        ),
    ];
    for (field, value) in cases {
        let mut mail = base.clone();
        mail[field] = value;
        let (status, got) = live
            .call(Method::POST, "/api/mail/send", &alice, Some(mail))
            .await;
        assert_eq!(
            (status, got["error"].as_str()),
            (400, Some("header_line_break")),
            "{field}: {got}"
        );
    }

    const BOUNDARY: &str = "hdr-boundary";
    let mut form = String::new();
    for (name, value) in [
        ("to", "bob@partner.test"),
        ("subject", "hi\r\nBcc: eve@evil.test"),
        ("body", "x"),
    ] {
        form.push_str(&format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    form.push_str(&format!("--{BOUNDARY}--\r\n"));
    let res = live
        .http
        .post(format!("{}/api/mail/send-multipart", live.base))
        .bearer_auth(&alice)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(form)
        .send()
        .await
        .expect("multipart send");
    let status = res.status().as_u16();
    let got: Value = res.json().await.unwrap_or_default();
    assert_eq!(
        (status, got["error"].as_str()),
        (400, Some("header_line_break")),
        "multipart: {got}"
    );

    let forward = json!({
        "from": ALICE, "to": ["bob@partner.test"], "subject": "Fwd: plans", "body": "FYI",
        "forward_message_id": "never-received@partner.test",
    });
    let (status, got) = live
        .call(Method::POST, "/api/mail/send", &alice, Some(forward))
        .await;
    assert_eq!(
        (status, got["error"].as_str()),
        (404, Some("forward_source_not_found")),
        "{got}"
    );

    assert_eq!(sent_files(tmp.path()), 0, "a refused send left a Sent copy");
}
