//! `mailrs-fastcore` binary entrypoint.

/// Resident parse workers for the spool drain and the background loops:
/// two, so a re-judging batch does not hold up delivery.
const PARSE_WORKERS: usize = 2;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    mailrs_core_api::refuse_blank_secrets();
    mailrs_parse_worker::install_or_exit("fastcore", PARSE_WORKERS);
    mailrs_fastcore::run().await
}
