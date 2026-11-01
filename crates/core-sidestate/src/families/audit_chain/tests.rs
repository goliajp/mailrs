use std::time::{Duration, Instant};

use super::*;
use crate::families::audit;
use crate::keys::admin::AUDIT_CHAIN_STATUS;

fn conn(name: &str) -> Connection {
    Connection::connect(&format!("mem://audit-chain-{name}")).expect("kevy")
}

fn add(c: &mut Connection, n: usize) {
    for i in 0..n {
        audit::append(
            c,
            "admin@x",
            "account.update",
            "a@x",
            &format!("d{i}"),
            1_000,
        )
        .unwrap();
    }
}

fn seal(c: &mut Connection) -> u64 {
    Sealer::default().tick(c, Instant::now()).unwrap()
}

/// Rewrite the log as an intruder would: every row, newest first.
fn rewrite(c: &mut Connection, change: impl FnOnce(&mut Vec<AuditFact>)) {
    let (mut facts, _) = rows(c, None).unwrap();
    change(&mut facts);
    c.del(&[AUDIT_LOG]).unwrap();
    for f in facts.iter().rev() {
        let raw = serde_json::to_vec(f).unwrap();
        c.lpush(AUDIT_LOG, &[raw.as_slice()]).unwrap();
    }
}

fn sealed_ten(name: &str) -> Connection {
    let mut c = conn(name);
    add(&mut c, 10);
    assert_eq!(seal(&mut c), 10);
    assert!(verify(&mut c, audit::CAP).unwrap().ok);
    c
}

fn bad(c: &mut Connection) -> (Option<i64>, String) {
    let r = verify(c, audit::CAP).unwrap();
    assert!(!r.ok, "{r:?}");
    (r.first_bad_id, r.reason.unwrap())
}

#[test]
fn two_writers_append_and_one_sealer_chains_them_all() {
    let url = "mem://audit-chain-concurrent";
    let mut keep = Connection::connect(url).unwrap();
    let writers: Vec<_> = (0..2)
        .map(|_| {
            std::thread::spawn(move || {
                let mut c = Connection::connect(url).unwrap();
                add(&mut c, 100);
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    assert_eq!(seal(&mut keep), 200);
    let r = verify(&mut keep, audit::CAP).unwrap();
    assert!(r.ok, "{r:?}");
    assert_eq!((r.rows, r.head_id), (200, Some(200)));
    // nothing new: a tick seals nothing and leaves the head where it was
    assert_eq!(seal(&mut keep), 0);
    assert_eq!(head(&mut keep).unwrap().unwrap().id, 200);
}

#[test]
fn a_changed_row_is_found() {
    let mut c = sealed_ten("changed");
    rewrite(&mut c, |f| {
        f.iter_mut().find(|r| r.id == 4).unwrap().detail = "x".into()
    });
    let (id, why) = bad(&mut c);
    assert_eq!(id, Some(4));
    assert!(why.contains("changed"), "{why}");
}

#[test]
fn a_removed_row_is_found() {
    let mut c = sealed_ten("removed");
    rewrite(&mut c, |f| f.retain(|r| r.id != 6));
    let (id, why) = bad(&mut c);
    assert_eq!(id, Some(6));
    assert!(why.contains("removed"), "{why}");
}

#[test]
fn an_inserted_row_is_found() {
    let mut c = sealed_ten("inserted");
    let copy = rows(&mut c, None).unwrap().0[0].clone();
    rewrite(&mut c, |f| {
        f.push(AuditFact {
            id: 3,
            detail: "forged".into(),
            ..copy.clone()
        })
    });
    assert_eq!(bad(&mut c), (Some(3), "two rows have this id".into()));

    let mut c = sealed_ten("inserted-older");
    rewrite(&mut c, |f| {
        f.push(AuditFact {
            id: 0,
            detail: "backdated".into(),
            ..copy
        })
    });
    assert_eq!(
        bad(&mut c),
        (Some(0), "a row is older than the chain".into())
    );
}

#[test]
fn two_rows_swapped_are_found() {
    let mut c = sealed_ten("swapped");
    rewrite(&mut c, |f| {
        let a = f.iter().position(|r| r.id == 2).unwrap();
        let b = f.iter().position(|r| r.id == 7).unwrap();
        let (da, db) = (f[a].detail.clone(), f[b].detail.clone());
        f[a].detail = db;
        f[b].detail = da;
    });
    assert_eq!(bad(&mut c).0, Some(2));
}

#[test]
fn a_changed_head_or_hash_is_found() {
    let mut c = sealed_ten("head");
    c.hset(AUDIT_CHAIN_HEAD, &[(b"hash" as &[u8], ZERO.as_bytes())])
        .unwrap();
    assert_eq!(
        bad(&mut c),
        (Some(10), "the head does not match the chain".into())
    );

    let mut c = sealed_ten("hash");
    c.hset(AUDIT_CHAIN, &[(b"5" as &[u8], ZERO.as_bytes())])
        .unwrap();
    assert_eq!(bad(&mut c).0, Some(5));
}

#[test]
fn trimming_the_oldest_rows_keeps_the_chain_whole() {
    let cap = 5;
    let mut c = conn("trimmed");
    add(&mut c, 8);
    assert_eq!(seal(&mut c), 8);
    // what audit::append does past its cap: keep the newest
    rewrite(&mut c, |f| f.retain(|r| r.id > 3));
    let r = verify(&mut c, cap).unwrap();
    assert!(r.ok, "{r:?}");
    assert_eq!(r.rows, 5);
    assert_eq!(prune(&mut c, cap).unwrap(), 3);
    assert_eq!(anchor(&mut c).unwrap().unwrap().id, 4);
    assert_eq!(chain(&mut c).unwrap().len(), 5);
    add(&mut c, 2);
    seal(&mut c);
    assert!(verify(&mut c, cap).unwrap().ok);
    // below the cap, a missing oldest row is a removal, not a trim
    let mut c = conn("not-trimmed");
    add(&mut c, 8);
    seal(&mut c);
    rewrite(&mut c, |f| f.retain(|r| r.id != 1));
    assert_eq!(bad(&mut c).0, Some(1));
}

#[test]
fn rows_written_before_the_chain_are_sealed_with_the_first_tick() {
    let mut c = conn("legacy");
    add(&mut c, 3);
    // nothing sealed yet: verify has nothing to check
    let r = verify(&mut c, audit::CAP).unwrap();
    assert!(r.ok && r.rows == 0 && r.head_id.is_none(), "{r:?}");
    assert_eq!(seal(&mut c), 3);
    assert_eq!(
        anchor(&mut c).unwrap(),
        Some(Link {
            id: 1,
            hash: ZERO.into()
        })
    );
    assert!(verify(&mut c, audit::CAP).unwrap().ok);
}

#[test]
fn an_id_that_never_got_its_row_is_sealed_as_a_gap_after_a_wait() {
    let mut c = conn("gap");
    add(&mut c, 2);
    c.incr(AUDIT_LOG_COUNTER).unwrap(); // id 3 taken, never written
    add(&mut c, 1);
    let mut s = Sealer::default();
    let t0 = Instant::now();
    assert_eq!(s.tick(&mut c, t0).unwrap(), 2);
    assert_eq!(s.tick(&mut c, t0 + Duration::from_secs(30)).unwrap(), 0);
    assert_eq!(
        s.tick(&mut c, t0 + GAP_WAIT + Duration::from_secs(1))
            .unwrap(),
        2
    );
    let r = verify(&mut c, audit::CAP).unwrap();
    assert!(r.ok && r.rows == 4, "{r:?}");
}

#[test]
fn the_replica_holds_what_was_sealed_and_catches_a_rewritten_chain() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = sealed_ten("replica");
    let s = daily(&mut c, audit::CAP, Some(dir.path()), 5).unwrap();
    assert!(s.ok && s.replica == "ok", "{s:?}");
    let file = dir.path().join("audit-1-10.jsonl");
    let body = std::fs::read(&file).unwrap();
    let sum = std::fs::read_to_string(dir.path().join("audit-1-10.jsonl.sha256")).unwrap();
    assert_eq!(
        sum,
        format!("{}  audit-1-10.jsonl\n", hex(&Sha256::digest(&body)))
    );
    assert_eq!(
        body.split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .count(),
        11
    );

    add(&mut c, 2);
    seal(&mut c);
    assert!(daily(&mut c, audit::CAP, Some(dir.path()), 6).unwrap().ok);
    assert!(dir.path().join("audit-11-12.jsonl.sha256").is_file());

    // the intruder changes row 3 and reseals everything after it: the store
    // agrees with itself, not with the replica
    rewrite(&mut c, |f| {
        f.iter_mut().find(|r| r.id == 3).unwrap().detail = "x".into()
    });
    let (facts, _) = rows(&mut c, None).unwrap();
    let mut sealed = chain(&mut c).unwrap();
    let mut prev = sealed[&2].clone();
    for id in 3..=12 {
        let h = row_hash(&prev, facts.iter().find(|f| f.id == id).unwrap());
        sealed.insert(id, h.clone());
        prev = h;
    }
    let pairs: Vec<(String, String)> = sealed
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let refs: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|(k, v)| (k.as_bytes(), v.as_bytes()))
        .collect();
    c.hset(AUDIT_CHAIN, &refs).unwrap();
    c.hset(AUDIT_CHAIN_HEAD, &[(b"hash" as &[u8], prev.as_bytes())])
        .unwrap();
    assert!(
        verify(&mut c, audit::CAP).unwrap().ok,
        "the store alone cannot tell"
    );
    let s = daily(&mut c, audit::CAP, Some(dir.path()), 7).unwrap();
    assert!(!s.ok, "{s:?}");
    assert_eq!(s.first_bad_id, Some(3));
    assert_eq!(s.replica, "failed");

    // a replica file changed after it was written
    std::fs::write(&file, b"{}\n").unwrap();
    let found = compare_replica(dir.path(), &mut c).unwrap();
    assert!(
        matches!(found, Err((None, ref why)) if why.contains("checksum")),
        "{found:?}"
    );
}

#[test]
fn the_daily_outcome_is_what_health_reads() {
    let mut c = sealed_ten("status");
    assert!(read_status(&mut c).unwrap().is_none());
    let s = daily(&mut c, audit::CAP, None, 42).unwrap();
    assert_eq!(read_status(&mut c).unwrap(), Some(s.clone()));
    assert_eq!((s.verified_at, s.ok, s.rows), (42, true, 10));
    assert_eq!(s.replica, "off");
    let raw = c.get(AUDIT_CHAIN_STATUS).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert!(v.get("first_bad_id").is_none(), "{v}");
}
