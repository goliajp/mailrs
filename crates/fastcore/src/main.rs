//! `mailrs-fastcore` binary entrypoint.

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    mailrs_fastcore::run().await
}
