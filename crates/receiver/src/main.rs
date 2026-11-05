//! The standalone `mailrs-receiver` process (P6 receiver/core split).

/// Resident parse workers: two, so one slow message does not hold up
/// every other session's scan.
const PARSE_WORKERS: usize = 2;

#[tokio::main]
async fn main() {
    mailrs_core_api::refuse_blank_secrets();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // install the process-wide rustls crypto provider before any TLS config
    // is built (SMTPS/STARTTLS listeners). Without this rustls 0.23 panics on
    // first use — mirrors what mailrs-server does in its run().
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    // message bytes are parsed in workers, so a parser that dies does
    // not end every SMTP session
    mailrs_parse_worker::install_or_exit("receiver", PARSE_WORKERS);

    mailrs_receiver::run().await;
}
