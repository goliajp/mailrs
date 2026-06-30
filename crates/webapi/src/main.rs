//! `mailrs-webapi` binary entrypoint.

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    mailrs_webapi::run().await
}
