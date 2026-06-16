//! The standalone `mailrs-receiver` process (P6 receiver/core split).

#[tokio::main]
async fn main() {
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

    mailrs_receiver::run().await;
}
