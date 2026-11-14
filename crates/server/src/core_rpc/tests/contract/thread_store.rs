//! Each core's `ThreadStore`, held to the trait's own check: the shared
//! routes are only as alike as the stores under them.

use super::harness::{self, Diffs};
use super::{ALICE, accounts, mail};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_stores_keep_a_readers_conversation_state_as_the_trait_says() {
    let c = harness::start_on("mem://thread-store-fastcore", "mem://thread-store-pg-core").await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    mail::seed(&c, &mut d).await;
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
    for (core, check) in [
        (
            "fastcore",
            Box::pin(mailrs_core_routes::store::check_thread_store(
                &*c.fastcore_state,
                ALICE,
                "t-sale@shop.test",
                "nobody-has-this@nowhere.test",
            )) as std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>>,
        ),
        (
            "pg-core",
            Box::pin(mailrs_core_routes::store::check_thread_store(
                &*c.pg_core_state,
                ALICE,
                "t-sale@shop.test",
                "nobody-has-this@nowhere.test",
            )),
        ),
    ] {
        eprintln!("checking {core}");
        check.await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn both_stores_read_a_users_messages_as_the_trait_says() {
    let c = harness::start_on(
        "mem://message-reads-fastcore",
        "mem://message-reads-pg-core",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&c, &mut d).await;
    mail::seed(&c, &mut d).await;
    assert!(d.0.is_empty(), "{}", d.0.join("\n\n"));
    let (own, stranger) = ("t-hello@partner.test", "nobody-has-this@nowhere.test");
    mailrs_core_routes::store::check_message_reads(&*c.fastcore_state, ALICE, own, stranger).await;
    mailrs_core_routes::store::check_message_reads(&*c.pg_core_state, ALICE, own, stranger).await;
}
