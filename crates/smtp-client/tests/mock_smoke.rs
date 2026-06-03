//! Smoke test for `tests/common/mock_smtp.rs`: client can connect, get
//! the greeting, EHLO, and QUIT cleanly.

mod common;

use common::mock_smtp::{Behavior, spawn_mock_smtp};
use mailrs_smtp_client::SmtpConnection;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mock_accepts_connect_ehlo_quit() {
    let mock = spawn_mock_smtp(Behavior::Accept).await;

    let mut conn = SmtpConnection::connect("127.0.0.1", mock.addr.port())
        .await
        .expect("connect to mock");
    let resp = conn.ehlo("client.test").await.expect("ehlo");
    assert_eq!(resp.code, 250);
    let body = resp.message();
    assert!(
        body.contains("STARTTLS"),
        "EHLO advertises STARTTLS: {body}"
    );

    conn.quit().await.expect("quit");
}
