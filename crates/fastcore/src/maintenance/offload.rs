//! Maintenance work on runtimes of its own, away from the one that serves.
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
//! blocks only its own runtime.
//!
//! Requests and periodic tasks get separate runtimes, so a daily self-heal
//! never takes the worker a request was promised. Requests are admitted
//! only while a worker is free; one more is answered `429` with what is
//! running, rather than left waiting behind a walk of half an hour.

use std::sync::{Arc, LazyLock, Mutex};

use axum::{
    Json,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Maintenance requests that run at once; one worker each.
pub(crate) const REQUEST_WORKERS: usize = 2;

fn runtime(workers: usize, name: &str) -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name(name)
        .enable_all()
        .build()
        .expect("a maintenance runtime starts")
}

static REQUESTS: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| runtime(REQUEST_WORKERS, "maintenance"));

static BACKGROUND: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| runtime(2, "maintenance-bg"));

/// Run a periodic task on the background maintenance runtime.
pub(crate) fn spawn<F>(task: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    BACKGROUND.spawn(task)
}

/// One admitted request, for the busy answer.
#[derive(Clone, serde::Serialize)]
struct Running {
    route: String,
    started_at: i64,
}

/// The maintenance requests one router is running.
#[derive(Default)]
pub(crate) struct Admission {
    running: Mutex<Vec<(u64, Running)>>,
    next_id: std::sync::atomic::AtomicU64,
}

/// Holds a request's place until its task ends or is cancelled.
struct Admitted {
    admission: Arc<Admission>,
    id: u64,
}

impl Drop for Admitted {
    fn drop(&mut self) {
        let mut running = self.admission.running.lock().expect("admission lock");
        running.retain(|(id, _)| *id != self.id);
    }
}

impl Admission {
    /// A place for `route`, or what holds every place.
    fn admit(self: &Arc<Self>, route: &str) -> Result<Admitted, Vec<Running>> {
        let mut running = self.running.lock().expect("admission lock");
        if running.len() >= REQUEST_WORKERS {
            return Err(running.iter().map(|(_, r)| r.clone()).collect());
        }
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        running.push((
            id,
            Running {
                route: route.to_string(),
                started_at: chrono::Utc::now().timestamp(),
            },
        ));
        Ok(Admitted {
            admission: self.clone(),
            id,
        })
    }
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
/// maintenance request runtime, or is answered `429` when every worker
/// there already has a request.
pub(crate) async fn off_serving_runtime(
    State(admission): State<Arc<Admission>>,
    req: Request,
    next: Next,
) -> Response {
    let admitted = match admission.admit(req.uri().path()) {
        Ok(admitted) => admitted,
        Err(running) => {
            return (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "error": "busy",
                    "limit": REQUEST_WORKERS,
                    "running": running,
                })),
            )
                .into_response();
        }
    };
    let mut task = AbortOnDrop(REQUESTS.spawn(async move {
        let _admitted = admitted;
        next.run(req).await
    }));
    match (&mut task.0).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!(err = %e, "maintenance request task failed");
            axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
