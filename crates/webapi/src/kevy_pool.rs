//! Pooled blocking connections to the shared kevy-server.
//!
//! Every `with_kevy` call used to open a TCP connection, and most of them
//! also spawned an OS thread to do it on — there were eight definitions of
//! that helper across this crate with two different bodies, and seven of them
//! ran `std::thread::spawn(…).join()` per call. Two per-operation costs
//! stacked, paid per kevy op rather than per request, on 201 call sites.
//!
//! The session path pays it on **every authenticated request**: resolving a
//! bearer token reads the session out of the shared store, so a connect (and
//! a thread) sat in front of every endpoint in the API.
//!
//! Connections round-robin across the pool so concurrent tasks do not
//! serialize on one RESP socket, open lazily so an unreachable server
//! surfaces as a per-op error rather than a boot failure, and are dropped on
//! error so the next use reconnects — which is what makes a kevy-server
//! restart transparent instead of sticky.
//!
//! Three sites are deliberately left un-pooled, with reasons rather than by
//! omission. `handlers::events` holds one connection per kevy shard for the
//! life of the process and blocks on it — a pooled connection handed back
//! between reads could be desynced mid-frame by another caller, and there is
//! nothing to amortize since the connect happens once. The three in
//! `handlers::auth::session` are per-login rather than per-request, sit behind
//! argon2, and converting them means wrapping a long statement sequence into
//! a closure at each site: structural risk for no measurable gain.
//!
//! This is the second copy of this shape in the tree; the first is
//! `crates/receiver/src/kevy_net.rs`, whose `KevyNetClient` this follows
//! closely. By `steel-cement-stone` a second instance is the signal to lift
//! it into shared steel — it is business-agnostic and would publish cleanly.
//! Not done here: that is a new crate boundary, and this change is already
//! touching 201 call sites.

use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use kevy_client::Connection;

/// Eight, matching `receiver`'s. Enough that concurrent handlers do not
/// queue on one socket, small enough that an idle process is not holding a
/// pile of them open.
const POOL_SIZE: usize = 8;

struct Pool {
    url: String,
    slots: Vec<Mutex<Option<Connection>>>,
    next: AtomicUsize,
}

/// One pool per process, built on first use from `MAILRS_KEVY_URL`.
///
/// The URL is read once. It does not change while a process runs, and
/// re-reading it per call was part of what made the old helper look cheap.
fn pool() -> Option<&'static Pool> {
    static POOL: OnceLock<Option<Pool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let url = std::env::var("MAILRS_KEVY_URL").ok()?;
        let mut slots = Vec::with_capacity(POOL_SIZE);
        for _ in 0..POOL_SIZE {
            slots.push(Mutex::new(None));
        }
        Some(Pool {
            url,
            slots,
            next: AtomicUsize::new(0),
        })
    })
    .as_ref()
}

/// Whether this process has a network kevy configured at all.
///
/// Callers that have a dev fallback (the session middleware's
/// `X-Mailrs-User` path) branch on this rather than on an error.
pub fn configured() -> bool {
    pool().is_some()
}

/// Run `f` against a pooled connection, synchronously.
///
/// **Blocking.** Call it from `spawn_blocking`, or from `block_in_place` on a
/// multi-thread runtime — `with_kevy` in `handlers::kevy_util` does the
/// latter and is what handlers should use.
///
/// On any error from `f` the connection is dropped, so the next call through
/// that slot reopens. A poisoned mutex is recovered rather than propagated: a
/// panic inside one closure must not wedge a slot for the process's life.
pub fn with_conn<F, T>(f: F) -> io::Result<T>
where
    F: FnOnce(&mut Connection) -> io::Result<T>,
{
    let p = pool().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "MAILRS_KEVY_URL is unset — no network kevy configured",
        )
    })?;
    let idx = p.next.fetch_add(1, Ordering::Relaxed) % p.slots.len();
    let mut slot = p.slots[idx].lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(Connection::connect(&p.url)?);
    }
    let conn = slot.as_mut().expect("just ensured Some");
    match f(conn) {
        Ok(v) => Ok(v),
        Err(e) => {
            // Possibly broken — a half-consumed RESP reply would desync
            // every later op on this socket, which is worse than a reconnect.
            *slot = None;
            Err(e)
        }
    }
}
