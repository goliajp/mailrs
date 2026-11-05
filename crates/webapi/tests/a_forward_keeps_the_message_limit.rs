//! A forward whose attachments come to more than a draft may hold (25 MiB)
//! still sends while the whole message fits the message limit (50 MiB): the
//! draft limit bounds what a draft stores, not what a send carries.
//!
//! Against a real kevy-server: the queue write after the copy is
//! pipelined, which the in-process kevy does not support.

mod live;

const ALICE: &str = "alice@example.test";
const BOUNDARY: &str = "mailrs-forward-boundary";
const MIB: usize = 1024 * 1024;

fn form(fields: &[(&str, &str)], file: (&str, usize)) -> Vec<u8> {
    let mut b = Vec::new();
    for (name, value) in fields {
        b.extend(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    b.extend(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"attachments\"; \
             filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
            file.0
        )
        .as_bytes(),
    );
    b.extend(std::iter::repeat_n(b'f', file.1));
    b.extend(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    b
}

async fn send(live: &live::Live, token: &str, body: Vec<u8>) -> (u16, serde_json::Value) {
    let res = live
        .http
        .post(format!("{}/api/mail/send-multipart", live.base))
        .bearer_auth(token)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .send()
        .await
        .expect("request");
    let status = res.status().as_u16();
    let text = res.text().await.expect("body");
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_forward_past_the_draft_limit_but_within_the_message_limit_sends() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    let token = live.login(ALICE, "pw").await;

    let (status, first) = send(
        &live,
        &token,
        form(
            &[("to", "bob@example.test"), ("subject", "the footage")],
            ("footage.bin", 20 * MIB),
        ),
    )
    .await;
    assert_eq!(status, 200, "{first}");
    let original = first["message_id"].as_str().expect("message_id");

    // 20 MiB carried from the original and 10 MiB added: 30 MiB of files,
    // about 41 MiB as a message
    let (status, got) = send(
        &live,
        &token,
        form(
            &[
                ("to", "carol@example.test"),
                ("subject", "Fwd: the footage"),
                ("forward_message_id", original),
            ],
            ("notes.bin", 10 * MIB),
        ),
    )
    .await;
    assert_eq!(status, 200, "{got}");
    let cur = tmp.path().join("example.test/alice/.Sent/cur");
    let mut sizes: Vec<u64> = std::fs::read_dir(&cur)
        .expect("Sent/cur")
        .map(|e| e.expect("entry").metadata().expect("meta").len())
        .collect();
    sizes.sort();
    assert_eq!(sizes.len(), 2, "{sizes:?}");
    assert!(
        sizes[1] > (30 * MIB * 4 / 3) as u64,
        "the forward carries both files: {sizes:?}"
    );
}
