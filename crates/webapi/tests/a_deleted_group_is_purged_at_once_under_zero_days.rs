//! Under a retention of 0 days, deleting a team inbox purges its working
//! state at the delete: every `shared:{gid}:*` key and the reply claims
//! members hold for it, with one audit row counting what went. Another
//! team's state, and alice's claims for it, stay.

mod live;
mod team_seed;

use mailrs_core_sidestate::families::shared_inbox;
use team_seed::*;

const KEVY: &str = "mem://webapi-group-purge-at-once";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deleting_a_team_under_zero_days_purges_its_working_state() {
    // SAFETY: the only test in this binary, before anything reads them
    unsafe {
        std::env::set_var("MAILRS_KEVY_URL", KEVY);
        std::env::set_var("MAILRS_DELETED_ACCOUNT_DAYS", "0");
    }
    let live = live::start(KEVY).await;
    let (admin, _) = people(&live).await;
    let gid = create_team(&live, &admin).await;
    let mut net = kevy_client::Connection::connect(KEVY).expect("kevy");
    work(&mut net, gid);
    // a second team, kept
    let other = gid + 100;
    work(&mut net, other);
    let others = team_keys(&mut net, other);
    let before = team_keys(&mut net, gid);
    assert!(before.len() >= 7, "{before:?}");
    assert_eq!(claim_groups(&mut net), [gid, other]);

    delete_team(&live, &admin, gid).await;

    assert_eq!(team_keys(&mut net, gid), Vec::<String>::new());
    assert_eq!(
        claim_groups(&mut net),
        [other],
        "only the deleted team's claim goes"
    );
    assert_eq!(
        team_keys(&mut net, other),
        others,
        "another team's state stays"
    );
    let purged = audit_rows(&mut net, "email_group.purged");
    assert_eq!(purged.len(), 1, "{purged:?}");
    assert_eq!(purged[0]["target"], gid.to_string());
    let detail: serde_json::Value =
        serde_json::from_str(purged[0]["detail"].as_str().expect("detail")).expect("json");
    let removed = detail["removed"].as_array().expect("removed");
    // one key of each family, all of them, and the claim
    let mut names: Vec<String> = before
        .iter()
        .map(|k| shared_inbox::group_family(gid, k))
        .collect();
    names.push(shared_inbox::GROUP_CLAIMS.to_string());
    names.sort();
    let mut counted: Vec<String> = removed
        .iter()
        .inspect(|p| assert_eq!(p["count"], 1, "{detail}"))
        .map(|p| p["name"].as_str().unwrap().to_string())
        .collect();
    counted.sort();
    assert_eq!(counted, names, "{detail}");
}
