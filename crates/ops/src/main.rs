//! `mailrs-ops` binary entrypoint.

#[tokio::main]
async fn main() -> std::process::ExitCode {
    mailrs_core_api::refuse_blank_secrets();
    mailrs_ops_report::build::mark_start();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("serve") => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
                )
                .init();
            match mailrs_ops::config::Config::from_env() {
                Ok(cfg) => mailrs_ops::daemon::serve(cfg).await,
                Err(e) => {
                    eprintln!("{e}");
                    std::process::ExitCode::from(2)
                }
            }
        }
        Some(_) => mailrs_ops::cli::run(&args).await,
    }
}
