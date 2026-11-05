//! A pasted image is kept only as long as a draft needs it: the upload
//! names who made it and expires, and one account cannot fill the store
//! with them.

mod live;

const KEVY: &str = "mem://webapi-inline-uploads-expire";
const ALICE: &str = "alice@example.test";

async fn upload(live: &live::Live, bearer: &str, bytes: &[u8]) -> (u16, serde_json::Value) {
    const BOUNDARY: &str = "paste-boundary";
    let mut form = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"p.png\"\r\nContent-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    form.extend_from_slice(bytes);
    form.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    let res = live
        .http
        .post(format!("{}/api/mail/inline-upload", live.base))
        .bearer_auth(bearer)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(form)
        .send()
        .await
        .expect("upload");
    let status = res.status().as_u16();
    (status, res.json().await.unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_names_its_owner_expires_and_is_capped() {
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let mut net = kevy_client::Connection::connect(KEVY).expect("kevy");

    let (status, got) = upload(&live, &alice, b"\x89PNG-pasted").await;
    assert_eq!(status, 200, "{got}");
    let id = got["url"]
        .as_str()
        .and_then(|u| u.rsplit('/').next())
        .expect("url")
        .to_string();
    let key = mailrs_core_sidestate::families::inline_images::key(&id);
    let owner = net.hget(key.as_bytes(), b"owner").expect("read");
    assert_eq!(owner.as_deref(), Some(ALICE.as_bytes()), "no owner on it");
    let ttl = net.ttl_ms(key.as_bytes()).expect("ttl");
    let thirty_days = 30 * 86_400 * 1000;
    assert!(ttl > 0 && ttl <= thirty_days, "kept for ever: ttl {ttl}");

    // the image still shows from its link while it is kept
    let res = live
        .http
        .get(format!("{}/api/mail/inline/{id}", live.base))
        .bearer_auth(&alice)
        .send()
        .await
        .expect("get");
    assert_eq!(res.status(), 200);

    // an account that has uploaded its share is refused, and nothing stored
    let counter = format!("inline-uploaded:{ALICE}");
    net.set(
        counter.as_bytes(),
        (200 * 1024 * 1024 - 4).to_string().as_bytes(),
    )
    .expect("near the cap");
    let before = net
        .keys(mailrs_core_sidestate::families::inline_images::key("*").as_bytes())
        .expect("keys")
        .len();
    let (status, _) = upload(&live, &alice, b"\x89PNG-too-much").await;
    assert_eq!(status, 429);
    assert_eq!(
        net.keys(mailrs_core_sidestate::families::inline_images::key("*").as_bytes())
            .expect("keys")
            .len(),
        before
    );
}
