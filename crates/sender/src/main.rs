//! `mailrs-sender` binary entrypoint.

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    mailrs_sender::run().await
}
