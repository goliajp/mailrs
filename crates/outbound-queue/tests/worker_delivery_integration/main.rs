//! Full-path worker delivery integration tests.
//!
//! Wires `deliver_domain_static` / `try_deliver_via_mx` against:
//! - a real Postgres container (testcontainers)
//! - the in-process mock SMTP server (`tests/common/mock_smtp.rs`)
//! - a real `TokioResolver` (used only for the per-MX TLSA lookup;
//!   resolving `127.0.0.1` returns NXDOMAIN so `has_dane` stays
//!   `false` and the worker takes the plain / opportunistic path)
//!
//! `port` parameter on `deliver_domain_static` + `try_deliver_via_mx`
//! is the test-injection seam — production wires `25`, these tests
//! inject the mock's ephemeral port.

#[path = "../common/mod.rs"]
mod common;

use mailrs_smtp_client::TokioResolver;

mod per_domain;
mod via_mx;
mod worker;

pub(crate) fn resolver() -> TokioResolver {
    TokioResolver::builder_tokio()
        .expect("hickory builder_tokio")
        .build()
        .expect("hickory resolver build")
}
