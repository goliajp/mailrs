//! Maintenance work on a runtime of its own, away from the one that serves.
//!
//! Most maintenance routes and several periodic tasks walk the corpus with
//! synchronous calls inside an `async fn`. On the serving runtime such a
//! stretch does not cost one worker out of four: the worker that last
//! polled the I/O driver runs it, the other workers stay parked without
//! being told there are sockets to read, and every request waits until the
//! stretch ends (measured on a prod copy while the fraud backfill ran:
//! three of four workers parked for the whole 5 s sample, half of all
//! requests over 0.5 s). Here the stretch blocks only this runtime.

use std::sync::LazyLock;

use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};

static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("maintenance")
        .enable_all()
        .build()
        .expect("the maintenance runtime starts")
});

/// Run `task` on the maintenance runtime.
pub(crate) fn spawn<F>(task: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.spawn(task)
}

/// Cancels the task when the request awaiting it goes away, as dropping the
/// handler's own future would have; [`super::hangup`] relies on that drop.
struct AbortOnDrop(tokio::task::JoinHandle<Response>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Middleware: the rest of the request, handler included, runs on the
/// maintenance runtime.
pub(crate) async fn off_serving_runtime(req: Request, next: Next) -> Response {
    let mut task = AbortOnDrop(spawn(next.run(req)));
    match (&mut task.0).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!(err = %e, "maintenance request task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
