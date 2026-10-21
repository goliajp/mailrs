//! Both cores, side by side, each recording which routes it was asked for.
//!
//! Each core gets its own in-process network kevy, because the side-state
//! families live there and two cores writing the same keys would make every
//! side-state comparison compare a store with itself.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use mailrs_core_api::client::Client;
use spg_sqlx::SpgPoolExt;

use crate::core_rpc::{CoreRpcState, build_full_router};

const SCHEMA_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/init-schema.sql"
));

/// Route templates a core answered, as its router names them.
pub(super) type Hits = Arc<Mutex<BTreeSet<String>>>;

pub(super) struct Cores {
    pub fastcore: Client,
    pub pg_core: Client,
    pub fastcore_hits: Hits,
    pub pg_core_hits: Hits,
    // The network kevy each core writes side state to. Held for the whole
    // run: a `mem://` store lives only while something has it open, and a
    // first open racing another can get a store of its own.
    _fastcore_kevy: kevy_client::Connection,
    _pg_core_kevy: kevy_client::Connection,
    _maildir: tempfile::TempDir,
}

const FASTCORE_KEVY: &str = "mem://contract-fastcore-side-state";
const PG_CORE_KEVY: &str = "mem://contract-pg-core-side-state";

async fn record(State(hits): State<Hits>, req: Request, next: Next) -> Response {
    if let Some(p) = req.extensions().get::<MatchedPath>() {
        hits.lock().expect("hits").insert(p.as_str().to_string());
    }
    next.run(req).await
}

async fn serve(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{addr}")
}

pub(super) async fn start() -> Cores {
    let fastcore_kevy = kevy_client::Connection::connect(FASTCORE_KEVY).expect("kevy");
    let pg_core_kevy = kevy_client::Connection::connect(PG_CORE_KEVY).expect("kevy");
    let maildir = tempfile::tempdir().expect("maildir");

    let store = Arc::new(kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap());
    let mut fc =
        mailrs_fastcore::FastcoreState::new(mailrs_mailbox_kevy::KevyMailboxStore::new(store));
    fc.net_url = Some(FASTCORE_KEVY.to_string());
    let fastcore_hits: Hits = Arc::default();
    let fastcore = serve(mailrs_fastcore::build_router(Arc::new(fc)).route_layer(
        axum::middleware::from_fn_with_state(fastcore_hits.clone(), record),
    ))
    .await;

    let pool = spg_sqlx::SpgPool::connect_in_memory()
        .await
        .expect("open in-memory spg");
    sqlx::raw_sql(SCHEMA_SQL)
        .execute(&pool)
        .await
        .expect("apply init-schema.sql");
    let state = Arc::new(CoreRpcState {
        mailbox: Arc::new(mailrs_mailbox::PgMailboxStore::new(pool.clone())),
        domain: Arc::new(crate::domain_store::DomainStore::new(
            Some(pool.clone()),
            None,
            crate::health::HealthState::new(),
        )),
        pool,
        maildir_root: maildir.path().to_string_lossy().into_owned(),
        net_url: Some(PG_CORE_KEVY.to_string()),
    });
    let pg_core_hits: Hits = Arc::default();
    let pg_core = serve(build_full_router(state, String::new()).route_layer(
        axum::middleware::from_fn_with_state(pg_core_hits.clone(), record),
    ))
    .await;

    Cores {
        fastcore: Client::new(fastcore, ""),
        pg_core: Client::new(pg_core, ""),
        fastcore_hits,
        pg_core_hits,
        _fastcore_kevy: fastcore_kevy,
        _pg_core_kevy: pg_core_kevy,
        _maildir: maildir,
    }
}

/// What two answers disagree on, collected rather than asserted one by one,
/// so a run reports every difference at once.
#[derive(Default)]
pub(super) struct Diffs(pub Vec<String>);

impl Diffs {
    /// Compare two answers to the same call. `ignore` names fields whose
    /// values legitimately differ between the cores (ids each store mints),
    /// matched at any depth.
    pub fn same<T: serde::Serialize>(
        &mut self,
        call: &str,
        fastcore: &T,
        pg_core: &T,
        ignore: &[&str],
    ) {
        let mut a = serde_json::to_value(fastcore).expect("json");
        let mut b = serde_json::to_value(pg_core).expect("json");
        strip(&mut a, ignore);
        strip(&mut b, ignore);
        if a != b {
            self.0
                .push(format!("{call}\n    fastcore: {a}\n    pg-core:  {b}"));
        }
    }

    /// Both cores must answer the call, and alike.
    pub fn both<T: serde::Serialize, E: std::fmt::Display>(
        &mut self,
        call: &str,
        fastcore: Result<T, E>,
        pg_core: Result<T, E>,
        ignore: &[&str],
    ) -> Option<(T, T)> {
        match (fastcore, pg_core) {
            (Ok(a), Ok(b)) => {
                self.same(call, &a, &b, ignore);
                Some((a, b))
            }
            (Err(a), Err(b)) if a.to_string() == b.to_string() => None,
            (a, b) => {
                let say = |r: &Result<T, E>| match r {
                    Ok(_) => "ok".to_string(),
                    Err(e) => format!("error: {e}"),
                };
                self.0.push(format!(
                    "{call}\n    fastcore: {}\n    pg-core:  {}",
                    say(&a),
                    say(&b)
                ));
                None
            }
        }
    }
}

fn strip(v: &mut serde_json::Value, ignore: &[&str]) {
    match v {
        serde_json::Value::Object(map) => {
            map.retain(|k, _| !ignore.contains(&k.as_str()));
            map.values_mut().for_each(|x| strip(x, ignore));
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|x| strip(x, ignore)),
        _ => {}
    }
}
