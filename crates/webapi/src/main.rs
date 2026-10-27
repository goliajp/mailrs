//! `mailrs-webapi` binary entrypoint.

#[tokio::main(flavor = "multi_thread")]
async fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = mailrs_webapi::cli::run(&args).await {
        return code;
    }
    mailrs_webapi::run().await;
    std::process::ExitCode::SUCCESS
}
