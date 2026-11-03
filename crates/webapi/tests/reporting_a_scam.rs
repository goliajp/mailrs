//! Reporting a scam through the front door: the reader's copy is held,
//! the reader (and only the reader) reads what came of it, and the waiting
//! list is for super administrators.

mod live;

use reqwest::Method;

const KEVY: &str = "mem://webapi-reporting-a-scam";
const ALICE: &str = "alice@example.test";
const BOB: &str = "bob@example.test";

const SCAM: &str = "From 1@xxx Thu Jan 05 09:00:00 +0000 2026\n\
Message-ID: <parcel-1@parcel-desk.example>\n\
From: Parcel Desk <notice@parcel-desk.example>\n\
To: Alice <alice@example.test>\n\
Subject: Your parcel is held\n\
Date: Mon, 02 Nov 2026 09:00:00 +0000\n\
\n\
Dear Alice, your parcel could not be delivered because a customs fee is unpaid.\n\
Pay the fee at https://parcel-desk.example/pay within 24 hours.\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_report_holds_the_readers_copy_and_tells_only_them() {
    let tmp = tempfile::tempdir().expect("tmp");
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_MAILDIR", tmp.path());
    }
    let live = live::start(KEVY).await;
    live.add_account(ALICE, "pw").await;
    live.add_account(BOB, "pw").await;
    let alice = live.login(ALICE, "pw").await;
    let bob = live.login(BOB, "pw").await;

    let res = live
        .http
        .post(format!("{}/api/mail/import", live.base))
        .bearer_auth(&alice)
        .body(SCAM.as_bytes().to_vec())
        .send()
        .await
        .expect("import");
    assert_eq!(res.status(), 200);

    let path = "/api/messages/parcel-1@parcel-desk.example/report";
    let (status, outcome) = live.call(Method::POST, path, &alice, None).await;
    assert_eq!(status, 200, "{outcome}");
    assert_eq!(outcome["state"], "none_found", "{outcome}");
    let id = outcome["report_id"]
        .as_str()
        .expect("report id")
        .to_string();

    let (status, held) = live
        .call(Method::GET, "/api/quarantine", &alice, None)
        .await;
    assert_eq!(status, 200);
    let held: Vec<&str> = held["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|i| i["thread_id"].as_str())
        .collect();
    assert_eq!(held, vec!["parcel-1@parcel-desk.example"]);

    // a message Bob does not have is not his to report
    let (status, _) = live.call(Method::POST, path, &bob, None).await;
    assert_eq!(status, 404);

    let mine = format!("/api/reports/{id}");
    let (status, read) = live.call(Method::GET, &mine, &alice, None).await;
    assert_eq!(
        (status, read["report_id"].as_str()),
        (200, Some(id.as_str()))
    );
    let (status, _) = live.call(Method::GET, &mine, &bob, None).await;
    assert_eq!(status, 404, "someone else's report");

    let (status, _) = live
        .call(Method::GET, "/api/admin/reports/pending", &alice, None)
        .await;
    assert_eq!(status, 403, "the waiting list is for super administrators");
    live.make_super(ALICE);
    let (status, pending) = live
        .call(Method::GET, "/api/admin/reports/pending", &alice, None)
        .await;
    assert_eq!(status, 200, "{pending}");
    assert_eq!(pending["items"], serde_json::json!([]));
}
