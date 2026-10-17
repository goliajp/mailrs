//! A core that answers a write with 204 and no body: the client used to
//! fail decoding the empty body, so the page reported an error for a
//! quota or recovery address that had in fact been saved.

use axum::http::StatusCode;
use axum::routing::post;
use mailrs_core_api::method::admin::{SetQuotaRequest, UpdateRecoveryEmailRequest};

async fn serve() -> String {
    let app = axum::Router::new()
        .route(
            "/v1/admin/accounts/{a}/quota",
            post(|| async { StatusCode::NO_CONTENT }),
        )
        .route(
            "/v1/admin/accounts/{a}/recovery-email",
            post(|| async { StatusCode::NO_CONTENT }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    format!("http://{addr}")
}

#[tokio::test]
async fn a_204_write_is_a_success() {
    let core = mailrs_core_api::client::Client::new(serve().await, "secret");
    core.set_quota("a@b.test", &SetQuotaRequest { quota_bytes: 1 })
        .await
        .expect("set_quota");
    let req: UpdateRecoveryEmailRequest =
        serde_json::from_value(serde_json::json!({ "recovery_email": "r@b.test" })).unwrap();
    core.set_recovery_email("a@b.test", &req)
        .await
        .expect("set_recovery_email");
}
