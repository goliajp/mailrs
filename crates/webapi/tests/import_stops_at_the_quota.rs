//! An import stops at the quota for the user's own mail as it does for
//! mail they received: a Sent folder brought over into a mailbox that is
//! full is not filed, and the report says the quota stopped it.

mod live;

const KEVY: &str = "mem://webapi-import-stops-at-the-quota";
const ALICE: &str = "alice@example.test";

const SENT_ONLY: &str = "From 1@xxx Thu Jan 05 09:00:00 +0000 2023\n\
X-Gmail-Labels: Sent\n\
Message-ID: <d@oldmail.net>\n\
From: Alice <alice@example.test>\n\
To: Carol <carol@partner.test>\n\
Subject: Lunch?\n\
Date: Thu, 05 Jan 2023 09:00:00 +0000\n\
\n\
Friday?\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_full_mailbox_takes_no_imported_sent_mail() {
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
    use mailrs_core_sidestate::keys::quota::{limit_key, used_key};
    net.set(limit_key(ALICE).as_bytes(), b"1000")
        .expect("limit");
    net.set(used_key(ALICE).as_bytes(), b"1000").expect("used");

    let res = live
        .http
        .post(format!("{}/api/mail/import", live.base))
        .bearer_auth(&alice)
        .body(SENT_ONLY.as_bytes().to_vec())
        .send()
        .await
        .expect("import");
    assert_eq!(res.status(), 200);
    let report: serde_json::Value = res.json().await.expect("report");
    assert_eq!(
        (report["sent"].as_u64(), report["over_quota"].as_bool()),
        (Some(0), Some(true)),
        "{report}"
    );
    let sent = tmp.path().join("example.test/alice/.Sent/cur");
    let filed = std::fs::read_dir(sent).map(|d| d.count()).unwrap_or(0);
    assert_eq!(filed, 0, "a copy was filed past the quota");
}
