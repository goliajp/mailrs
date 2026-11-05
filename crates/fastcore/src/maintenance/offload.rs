//! Maintenance work on a runtime of its own, away from the one that serves.
//!
//! Most maintenance routes and several periodic tasks walk the corpus with
//! synchronous calls inside an `async fn`. On the serving runtime such a
//! stretch holds a worker for its whole length, and one that starts when a
//! timer wakes the task (a pause inside the walk, a periodic task's sleep)
//! costs more than that: it runs on the worker that was polling the I/O
//! driver, tokio hands the driver to no other worker while only one task
//! is runnable, and no socket is read until the stretch ends. Measured on
//! a prod copy during the fraud backfill: three of four workers parked
//! through a 5 s sample, half of all requests over 0.5 s. Here a stretch
//! blocks only this runtime.

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
