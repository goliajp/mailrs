//! A message with an attachment larger than axum's 2 MiB default goes
//! through `/api/mail/send-multipart` and the core files its sent copy;
//! a form over the web tier's limit is turned away with a 413 that names
//! the limit.
//!
//! The core's route kept the 2 MiB default when the web tier began
//! handing it the sent copy, so every send with an attachment of a few
//! megabytes failed with an empty 500 and nothing was sent.
//!
//! Against a real kevy-server: the queue write after the copy is
//! pipelined, which the in-process kevy does not support.

mod live;

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const ALICE: &str = "alice@example.test";
const BOUNDARY: &str = "mailrs-test-boundary";

fn form(attachment_len: usize) -> Vec<u8> {
    let mut b = Vec::new();
    for (name, value) in [
        ("to", "bob@example.test"),
        ("subject", "weekly report"),
        ("body", "attached"),
    ] {
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
             filename=\"report.zip\"\r\nContent-Type: application/zip\r\n\r\n"
        )
        .as_bytes(),
    );
    // bytes that do not compress or encode away
    let mut x: u32 = 1;
    b.extend((0..attachment_len).map(|_| {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x as u8
    }));
    b.extend(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    b
}

/// The kevy-server tag production runs, as in `deploy/docker-compose.prod.yml`.
async fn kevy_server() -> (testcontainers::ContainerAsync<GenericImage>, String) {
    let _startup = mailrs_test_docker::startup_lock().await;
    let tag = std::env::var("MAILRS_TEST_KEVY_TAG").unwrap_or_else(|_| "6.4.0".to_string());
    let container = GenericImage::new("ghcr.io/goliajp/kevy", &tag)
        .with_wait_for(WaitFor::message_on_stderr("starting:"))
        .with_exposed_port(6379.tcp())
        .with_startup_timeout(mailrs_test_docker::STARTUP_TIMEOUT)
        .start()
        .await
        .expect("start kevy container");
    let host =
        mailrs_test_docker::ipv4_loopback(&container.get_host().await.expect("host").to_string());
    let port = container.get_host_port_ipv4(6379).await.expect("port");
    let url = format!("kevy://{host}:{port}");
    for _ in 0..50 {
        let u = url.clone();
        let up = tokio::task::spawn_blocking(move || {
            kevy_client::Connection::connect(&u).and_then(|mut c| c.ping())
        })
        .await
        .expect("join");
        if up.is_ok() {
            return (container, url);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("kevy-server at {url} never answered");
}

async fn post(live: &live::Live, token: &str, body: Vec<u8>) -> (u16, serde_json::Value) {
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
async fn an_eight_megabyte_attachment_is_filed_and_an_oversized_form_says_the_limit() {
    let (_kevy, kevy) = kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    let token = live.login(ALICE, "pw").await;

    let (status, body) = post(&live, &token, form(8 * 1024 * 1024)).await;
    assert_eq!(status, 200, "{body}");
    let cur = tmp.path().join("example.test/alice/.Sent/cur");
    let sizes: Vec<u64> = std::fs::read_dir(&cur)
        .expect("Sent/cur")
        .map(|e| e.expect("entry").metadata().expect("meta").len())
        .collect();
    assert_eq!(sizes.len(), 1, "{sizes:?}");
    assert!(
        sizes[0] > 8 * 1024 * 1024 * 4 / 3,
        "the copy is not the whole message: {sizes:?}"
    );

    let limit = mailrs_webapi::handlers::send_error::MAX_MULTIPART_BODY;
    let (status, body) = post(&live, &token, form(limit + 1)).await;
    assert_eq!(status, 413, "{body}");
    assert_eq!(body["limit_bytes"], limit, "{body}");
    assert!(
        body["error"].as_str().is_some_and(|e| e.contains("25 MiB")),
        "{body}"
    );
}
