//! The operator's password clearing answers alike on both cores: the
//! preview, the clearing, and what the account says afterwards.

use axum::Json;
use axum::extract::State;
use mailrs_core_sidestate::families::password_clear::{
    Addresses, operator_clear, operator_preview,
};

use super::harness::{self, Diffs};
use super::{ALICE, accounts};

fn body() -> Addresses {
    Addresses {
        addresses: vec![ALICE.into(), "nobody@example.test".into()],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_operator_clears_alike_on_both_cores() {
    let cores = harness::start_on(
        "mem://operator-fastcore-side-state",
        "mem://operator-pg-core-side-state",
    )
    .await;
    let mut d = Diffs::default();
    accounts::seed(&cores, &mut d).await;

    let fc = operator_preview(State(cores.fastcore_state.clone()), Json(body()))
        .await
        .map(|j| j.0);
    let pg = operator_preview(State(cores.pg_core_state.clone()), Json(body()))
        .await
        .map(|j| j.0);
    let (fc, pg) = (fc.expect("fastcore preview"), pg.expect("pg-core preview"));
    assert_eq!(fc.items[0].exists, true);
    d.same("operator preview", &fc, &pg, &[]);

    let fc = operator_clear(State(cores.fastcore_state.clone()), Json(body()))
        .await
        .map(|j| j.0)
        .expect("fastcore clear");
    let pg = operator_clear(State(cores.pg_core_state.clone()), Json(body()))
        .await
        .map(|j| j.0)
        .expect("pg-core clear");
    assert_eq!(fc["items"][0]["result"], "cleared", "{fc}");
    assert_eq!(fc["items"][1]["result"], "unknown", "{fc}");
    d.same("operator clear", &fc, &pg, &[]);

    let after = d.both(
        "get_account_with_hash after the operator's clearing",
        cores.fastcore.get_account_with_hash(ALICE).await,
        cores.pg_core.get_account_with_hash(ALICE).await,
        &["created_at", "password_cleared_at"],
    );
    let (acct, _) = after.expect("both answered");
    assert_eq!(acct.password_hash.as_deref(), Some(""));
    assert_eq!(
        acct.public.password_cleared_by.as_deref(),
        Some("operator:devops")
    );
    assert!(d.0.is_empty(), "the cores differ:\n{}", d.0.join("\n"));
}
