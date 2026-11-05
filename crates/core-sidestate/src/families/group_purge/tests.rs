use super::*;
use crate::families::shared_inbox::{HeldClaim, assign, drafts, notes, presence};
use crate::keys::admin::AUDIT_LOG;

const DAY: i64 = 86_400;
const T0: i64 = 1_700_000_000;

/// A core whose store holds nothing of the group.
fn none(_: i64, _: bool) -> io::Result<Vec<PurgeCount>> {
    Ok(Vec::new())
}

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://group-purge-{name}")).unwrap()
}

fn team(c: &mut Connection, gid: i64, address: &str) {
    let row = serde_json::json!({ "id": gid, "address": address, "name": "team" });
    c.hset(
        email_groups::GROUPS_KEY.as_bytes(),
        &[(
            gid.to_string().as_bytes(),
            serde_json::to_vec(&row).unwrap().as_slice(),
        )],
    )
    .unwrap();
    c.sadd(
        email_groups::members_key(gid).as_bytes(),
        &[b"a@x".as_slice()],
    )
    .unwrap();
    email_groups::set_shared(c, gid, true).unwrap();
}

/// Working state in every family a team writes, and a claim of a@x's.
fn work(c: &mut Connection, gid: i64) {
    let r = shared_inbox::conv_for_arrival(c, gid, "<m1@x>", "", &[]).unwrap();
    shared_inbox::record_copy(c, r, "a@x", "t1").unwrap();
    let fact = assign::AssignFact {
        at: 1,
        by: "a@x".into(),
        to: Some("a@x".into()),
    };
    assign::append(c, r, &fact).unwrap();
    notes::add(c, r, "a@x", "note", 2).unwrap();
    drafts::announce(c, r, "a@x", 5, 3).unwrap().unwrap();
    presence::heartbeat(c, r, "a@x", presence::State::Composing).unwrap();
    let claim = HeldClaim {
        gid,
        conv: r.conv,
        claim_id: format!("c{gid}"),
    };
    shared_inbox::hold_claim(c, "a@x", &format!("<reply{gid}@x>"), &claim).unwrap();
}

/// Every key and value in the store, sorted: what a pass that writes
/// nothing leaves as it found.
fn snapshot(c: &mut Connection) -> Vec<(String, Vec<Vec<u8>>)> {
    let mut out = Vec::new();
    for key in c.keys(b"*").unwrap() {
        let name = String::from_utf8_lossy(&key).to_string();
        let mut parts: Vec<Vec<u8>> = match c.type_of(&key).unwrap().as_str() {
            "string" => c.get(&key).unwrap().into_iter().collect(),
            "hash" => c.hgetall(&key).unwrap(),
            "list" => c.lrange(&key, 0, -1).unwrap(),
            "set" => c.smembers(&key).unwrap(),
            _ => Vec::new(),
        };
        parts.sort();
        out.push((name, parts));
    }
    out.sort();
    out
}

fn team_keys(c: &mut Connection, gid: i64) -> usize {
    c.keys(shared_inbox::group_glob(gid).as_bytes())
        .unwrap()
        .len()
}

fn claim_gids(c: &mut Connection) -> Vec<i64> {
    let flat = c
        .hgetall(shared_inbox::claims_key("a@x").as_bytes())
        .unwrap();
    let mut out: Vec<i64> = flat
        .chunks(2)
        .filter_map(|kv| serde_json::from_slice::<HeldClaim>(&kv[1]).ok())
        .map(|h| h.gid)
        .collect();
    out.sort();
    out
}

fn audit(c: &mut Connection, action: &str) -> Vec<serde_json::Value> {
    c.lrange(AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(r).ok())
        .filter(|r| r["action"] == action)
        .collect()
}

fn count(row: &GroupPurgeRow, name: &str) -> u64 {
    row.network
        .iter()
        .find(|p| p.name == name)
        .map_or(0, |p| p.count)
}

#[test]
fn a_deleted_group_is_out_of_use_at_once_and_its_state_is_kept() {
    let mut c = conn("soft");
    team(&mut c, 1, "team@x");
    work(&mut c, 1);
    let state = team_keys(&mut c, 1);
    assert_eq!(state, 8);

    assert!(soft_delete(&mut c, 1, "admin@x", T0).unwrap());

    assert_eq!(
        email_groups::find_by_address(&mut c, "team@x").unwrap(),
        None
    );
    assert!(!email_groups::is_shared(&mut c, 1).unwrap());
    assert!(!email_groups::is_team_member(&mut c, 1, "a@x").unwrap());
    assert!(
        email_groups::addresses_with_member(&mut c, "a@x")
            .unwrap()
            .is_empty()
    );
    assert!(shared_inbox::members(&mut c, 1).unwrap().is_empty());
    assert_eq!(team_keys(&mut c, 1), state, "the working state is kept");
    assert_eq!(claim_gids(&mut c), [1]);
    let rows = audit(&mut c, SOFT_DELETED);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["actor"], "admin@x");
    assert_eq!(rows[0]["target"], "1");

    // a second delete changes nothing and keeps the first time
    assert!(!soft_delete(&mut c, 1, "admin@x", T0 + DAY).unwrap());
    let r = run(&mut c, true, None, T0 + 30 * DAY, 30, none).unwrap();
    assert_eq!((r.pending, r.due), (1, 1));
    assert_eq!(r.rows[0].deleted_at, T0);
    assert_eq!(r.rows[0].address, "team@x");
    assert_eq!(audit(&mut c, SOFT_DELETED).len(), 1);
}

#[test]
fn the_state_is_kept_until_its_retention_has_passed_and_a_dry_run_writes_nothing() {
    let mut c = conn("retention");
    team(&mut c, 1, "team@x");
    team(&mut c, 2, "other@x");
    work(&mut c, 1);
    work(&mut c, 2);
    soft_delete(&mut c, 1, "admin@x", T0).unwrap();
    let before = snapshot(&mut c);

    let early = run(&mut c, false, None, T0 + 29 * DAY, 30, none).unwrap();
    assert_eq!((early.pending, early.due), (1, 0));
    assert!(early.rows.is_empty());
    assert_eq!(snapshot(&mut c), before, "nothing is due yet");

    let dry = run(&mut c, true, None, T0 + 30 * DAY, 30, none).unwrap();
    assert_eq!((dry.pending, dry.due, dry.rows.len()), (1, 1, 1));
    assert_eq!(snapshot(&mut c), before, "a dry run writes nothing");
    let row = &dry.rows[0];
    assert_eq!(row.gid, 1);
    // one key of each family a team writes, and the claim
    let names: Vec<String> = c
        .keys(shared_inbox::group_glob(1).as_bytes())
        .unwrap()
        .iter()
        .map(|k| shared_inbox::group_family(1, &String::from_utf8_lossy(k)))
        .chain([shared_inbox::GROUP_CLAIMS.to_string()])
        .collect();
    assert_eq!(names.len(), 9);
    assert_eq!(row.network.len(), 9, "{:?}", row.network);
    for name in &names {
        assert_eq!(count(row, name), 1, "{name}: {:?}", row.network);
    }

    let done = run(&mut c, false, None, T0 + 30 * DAY, 30, none).unwrap();
    assert_eq!(
        done.rows, dry.rows,
        "the purge removes what the dry run counted"
    );
    assert_eq!(team_keys(&mut c, 1), 0);
    assert_eq!(claim_gids(&mut c), [2], "only the group's claims go");
    assert_eq!(team_keys(&mut c, 2), 8, "another group's state stays");
    let purged = audit(&mut c, PURGED);
    assert_eq!(purged.len(), 1);
    assert_eq!(purged[0]["target"], "1");
    let detail: serde_json::Value =
        serde_json::from_str(purged[0]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(
        detail["network"],
        serde_json::to_value(&row.network).unwrap()
    );
    assert_eq!(detail["deleted_at"], T0);
}

#[test]
fn under_zero_days_the_state_is_due_at_the_delete() {
    let mut c = conn("zero");
    team(&mut c, 3, "team@x");
    work(&mut c, 3);
    soft_delete(&mut c, 3, "admin@x", T0).unwrap();
    let r = run(&mut c, false, Some(3), T0, 0, none).unwrap();
    assert_eq!(r.rows.len(), 1);
    assert_eq!(team_keys(&mut c, 3), 0);
    assert!(claim_gids(&mut c).is_empty());
    assert_eq!(audit(&mut c, PURGED).len(), 1);
}

#[test]
fn a_pass_with_nothing_due_converges_to_reading_and_writing_nothing() {
    let mut c = conn("converge");
    team(&mut c, 1, "team@x");
    work(&mut c, 1);
    soft_delete(&mut c, 1, "admin@x", T0).unwrap();
    assert_eq!(run(&mut c, false, None, T0, 0, none).unwrap().rows.len(), 1);
    let after = snapshot(&mut c);
    for _ in 0..3 {
        let r = run(&mut c, false, None, T0 + DAY, 0, none).unwrap();
        assert_eq!((r.pending, r.due, r.rows.len()), (0, 0, 0));
        assert!(r.skipped_live.is_empty());
    }
    assert_eq!(snapshot(&mut c), after, "an idle pass writes nothing");
    assert_eq!(audit(&mut c, PURGED).len(), 1);
}

#[test]
fn a_marked_group_whose_row_is_there_is_left_alone() {
    let mut c = conn("live");
    team(&mut c, 1, "team@x");
    work(&mut c, 1);
    // a delete cut short after its mark: the row is still there
    let mark = Mark {
        deleted_at: T0,
        group: serde_json::json!({ "id": 1, "address": "team@x" }),
    };
    c.hset(
        GROUPS_KEY.as_bytes(),
        &[(
            b"1".as_slice(),
            serde_json::to_vec(&mark).unwrap().as_slice(),
        )],
    )
    .unwrap();
    let r = run(&mut c, false, None, T0, 0, none).unwrap();
    assert_eq!(r.skipped_live, [1]);
    assert!(r.rows.is_empty());
    assert_eq!(team_keys(&mut c, 1), 8);
}

#[test]
fn the_core_s_store_is_purged_with_the_group_and_counted_in_the_same_audit_row() {
    let mut c = conn("mail-store");
    team(&mut c, 6, "team@x");
    work(&mut c, 6);
    soft_delete(&mut c, 6, "admin@x", T0).unwrap();
    let mut calls = Vec::new();
    let mut store = |gid: i64, dry_run: bool| {
        calls.push((gid, dry_run));
        Ok(vec![
            PurgeCount {
                name: "replies".into(),
                count: 2,
            },
            PurgeCount {
                name: "empty".into(),
                count: 0,
            },
        ])
    };
    let dry = run(&mut c, true, None, T0, 0, &mut store).unwrap();
    assert_eq!(
        dry.rows[0].mail_store.len(),
        1,
        "a family with nothing is left out"
    );
    let done = run(&mut c, false, None, T0, 0, &mut store).unwrap();
    assert_eq!(calls, [(6, true), (6, false)]);
    assert_eq!(done.rows[0].mail_store, dry.rows[0].mail_store);
    let purged = audit(&mut c, PURGED);
    let detail: serde_json::Value =
        serde_json::from_str(purged[0]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(
        detail["mail_store"],
        serde_json::json!([{ "name": "replies", "count": 2 }])
    );
    assert_eq!(detail["network"].as_array().unwrap().len(), 9);
}
