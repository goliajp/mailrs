//! `mailrs-fastcore` binary entrypoint.

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    mailrs_core_api::refuse_blank_secrets();
    mailrs_fastcore::run().await
}
