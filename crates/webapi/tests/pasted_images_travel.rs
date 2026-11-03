//! An image pasted into a message travels inside it: what is queued for
//! the recipient carries the image as a part under a Content-ID, and the
//! HTML points at that, not at this server's upload URL, which nobody
//! outside can open.
//!
//! Against a real kevy-server: the queue write is pipelined.

mod live;

use base64::Engine as _;
use mailrs_core_sidestate::families::outbound as ob;
use reqwest::Method;
use serde_json::json;

const ALICE: &str = "alice@example.test";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pasted_image_is_sent_inside_the_message() {
    let (_kevy, kevy) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", &kevy);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(&kevy).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;

    const BOUNDARY: &str = "paste-boundary";
    let mut form = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"p.png\"\r\nContent-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    form.extend_from_slice(b"\x89PNG-pasted");
    form.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    let uploaded: serde_json::Value = live
        .http
        .post(format!("{}/api/mail/inline-upload", live.base))
        .bearer_auth(&alice)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(form)
        .send()
        .await
        .expect("upload")
        .json()
        .await
        .expect("upload body");
    let url = uploaded["url"].as_str().expect("url").to_string();
    let id = url.rsplit('/').next().expect("id").to_string();

    let mail = json!({
        "from": ALICE, "to": ["bob@elsewhere.test"], "subject": "look",
        "body": "look", "html_body": format!("<p>look</p><p><img src=\"{url}\"></p>"),
    });
    let (status, got) = live
        .call(Method::POST, "/api/mail/send", &alice, Some(mail))
        .await;
    assert_eq!(status, 200, "{got}");

    let mut c = kevy_client::Connection::connect(&kevy).unwrap();
    let keys = c.keys(ob::JOB_PATTERN).unwrap();
    assert_eq!(keys.len(), 1);
    let blob = c.hget(&keys[0], b"blob").unwrap().unwrap();
    let job: serde_json::Value = serde_json::from_slice(&blob).unwrap();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(job["message_data_b64"].as_str().unwrap())
        .unwrap();
    let parsed = mailrs_mime::parse(&raw);
    let cid = format!("{id}@inline");
    let image = parsed
        .files()
        .find(|f| f.content_id.as_deref() == Some(cid.as_str()))
        .unwrap_or_else(|| panic!("no part under {cid}: {}", String::from_utf8_lossy(&raw)));
    assert_eq!(image.body.as_ref(), b"\x89PNG-pasted");
    let html = parsed
        .walk()
        .find(|p| p.content_type.mime_type() == "text/html")
        .map(|p| String::from_utf8_lossy(&p.body).to_string())
        .expect("html");
    assert!(html.contains(&format!("src=\"cid:{cid}\"")), "{html}");
    assert!(!html.contains("/api/mail/inline/"), "{html}");
}
