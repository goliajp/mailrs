//! Shared kevy connection helper. Every handler in this crate that
//! reads or writes the network kevy at `MAILRS_KEVY_URL` used to carry
//! its own copy of `with_kevy` — 6 near-duplicates. This module owns
//! the single definition so callers just `use crate::handlers::kevy_util::with_kevy`.
//!
//! The helper spawns a blocking OS thread and opens a fresh connection
//! per call. Chose OS-thread over `tokio::task::spawn_blocking` because
//! `kevy_client::Connection` is `!Send` on some platforms and we want
//! this to work in every async context.

use axum::http::StatusCode;

/// Run `f` against a fresh kevy connection on a blocking thread.
/// Any I/O error surfaces as `INTERNAL_SERVER_ERROR`. Callers that
/// need to distinguish (e.g., NOT_FOUND on empty key) should peek the
/// returned value before mapping.
pub fn with_kevy<F, T>(f: F) -> Result<T, StatusCode>
where
    F: FnOnce(&mut kevy_client::Connection) -> std::io::Result<T> + Send + 'static,
    T: Send + 'static,
{
    let url = std::env::var("MAILRS_KEVY_URL").map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let handle = std::thread::spawn(move || -> std::io::Result<T> {
        let mut c = kevy_client::Connection::open(&url)?;
        f(&mut c)
    });
    handle
        .join()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
