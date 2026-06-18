#[tokio::main]
async fn main() {
    mailrs_server::run().await;
}
