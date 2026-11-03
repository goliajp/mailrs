//! A mailbox at its quota, alike on both cores: the copy of a message the
//! user sends is filed whatever the quota says, and a Sent message an
//! import brings over is refused with 507.

use super::harness::{self, Diffs};
use super::{ALICE, accounts};

const SIDE_FASTCORE: &str = "mem://quota-fastcore-side-state";
const SIDE_PG_CORE: &str = "mem://quota-pg-core-side-state";

fn mail(id: &str) -> Vec<u8> {
    format!(
        "Date: Tue, 14 Nov 2023 22:40:00 +0000\r\nFrom: Alice <alice@example.test>\r\n\
         To: Carol <carol@partner.test>\r\nSubject: {id}\r\nMessage-ID: <{id}@example.test>\r\n\
         \r\nhello\r\n"
    )
    .into_bytes()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_an_import_stops_at_the_quota_on_both_cores() {
    let cores = harness::start_on(SIDE_FASTCORE, SIDE_PG_CORE).await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;
    use mailrs_core_sidestate::keys::quota::{limit_key, used_key};
    for url in [SIDE_FASTCORE, SIDE_PG_CORE] {
        let mut c = kevy_client::Connection::connect(url).expect("side state");
        c.set(limit_key(ALICE).as_bytes(), b"1000").expect("limit");
        c.set(used_key(ALICE).as_bytes(), b"1000").expect("used");
    }
    let rcpts = vec!["Carol <carol@partner.test>".to_string()];

    let sent = d.both(
        "file_sent at the quota",
        cores.fastcore.file_sent(ALICE, &mail("q1"), &rcpts).await,
        cores.pg_core.file_sent(ALICE, &mail("q1"), &rcpts).await,
        &["blob_ref"],
    );
    assert!(sent.is_some(), "a send's copy was refused at the quota");

    for (core, client) in [("fastcore", &cores.fastcore), ("pg-core", &cores.pg_core)] {
        let got = client
            .file_imported_sent(ALICE, &mail("q2"), &rcpts)
            .await
            .map(|_| ());
        assert_eq!(
            got.map_err(|e| e.status_code()),
            Err(507),
            "{core} filed an imported Sent message past the quota"
        );
    }
    assert!(d.0.is_empty(), "the cores differ:\n{}", d.0.join("\n"));
}
