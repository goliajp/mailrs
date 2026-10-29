//! The access log never writes a credential: a query-string session token,
//! or the id that lets anyone fetch an inline image. A binary of
//! its own: tracing caches "no subscriber" per call site, so a log-capturing
//! test that shares a process with others sees nothing whenever another
//! thread touched the call site first.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

fn router() -> axum::Router {
    let state = Arc::new(mailrs_webapi::WebState {
        core: Arc::new(mailrs_core_api::client::Client::new(
            "http://127.0.0.1:1",
            "test-secret",
        )),
        bind_addr: "127.0.0.1:0".into(),
        event_bus: std::sync::OnceLock::new(),
        trace_bus: std::sync::OnceLock::new(),
        receiver_stats: Arc::new(mailrs_webapi::handlers::smtp_monitor::StatsCache::default()),
        started_at: std::time::Instant::now(),
        llm_config: None,
    });
    mailrs_webapi::build_router(state)
}

async fn get(path: &str) {
    let _ = router()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
}

#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A browser can send a WebSocket's or a download link's session token
/// only in the query string; the access log must not write it down.
#[tokio::test(flavor = "current_thread")]
async fn the_access_log_never_contains_a_query_token() {
    let captured = Captured::default();
    let sink = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || sink.clone())
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    get("/api/admin/audit/messages/1/raw?token=SECRET-TOKEN-VALUE").await;
    // an inline image's id is what lets anyone fetch it
    get("/api/mail/inline/SECRET-INLINE-ID").await;
    let log = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("/api/admin/audit/messages/1/raw"),
        "nothing was logged: {log}"
    );
    assert!(
        !log.contains("SECRET-TOKEN-VALUE"),
        "the token reached the log: {log}"
    );
    assert!(log.contains("/api/mail/inline/{id}"), "{log}");
    assert!(
        !log.contains("SECRET-INLINE-ID"),
        "an inline image id reached the log: {log}"
    );
}
