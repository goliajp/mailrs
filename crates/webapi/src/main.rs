//! `mailrs-webapi` binary entrypoint.

fn main() -> std::process::ExitCode {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let code = runtime.block_on(async {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if let Some(code) = mailrs_webapi::cli::run(&args).await {
            return code;
        }
        mailrs_webapi::run().await;
        std::process::ExitCode::SUCCESS
    });
    // The kevy feed readers block in `spawn_blocking` reads that never
    // return on their own. Dropping the runtime waits for blocking tasks
    // without limit, which turned every stop into a SIGKILL after the
    // container's grace period.
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    code
}
