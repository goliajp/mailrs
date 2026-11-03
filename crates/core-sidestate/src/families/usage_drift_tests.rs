use super::*;
use crate::keys::admin::AUDIT_LOG;

fn mail(root: &Path, account: &str, folder: &str, name: &str, bytes: usize) {
    let (local, domain) = account.split_once('@').unwrap();
    let dir = root.join(domain).join(local).join(folder);
    // a Maildir++ folder has all three, `new/` being what marks it one
    for sub in ["cur", "new", "tmp"] {
        std::fs::create_dir_all(dir.parent().unwrap().join(sub)).unwrap();
    }
    std::fs::write(dir.join(name), vec![b'x'; bytes]).unwrap();
}

fn set(conn: &mut Connection, account: &str, value: i64) {
    conn.set(used_key(account).as_bytes(), value.to_string().as_bytes())
        .unwrap();
}

fn counter(conn: &mut Connection, account: &str) -> Option<i64> {
    read_counter(conn, &used_key(account)).unwrap()
}

fn audit_rows(conn: &mut Connection) -> Vec<audit::AuditFact> {
    conn.lrange(AUDIT_LOG, 0, -1)
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(r).unwrap())
        .collect()
}

/// Accounts: one right, one low, one high, one with a counter but no
/// maildir, one with mail but no counter; and a counter for an address
/// that is no longer an account.
fn seeded(name: &str) -> (tempfile::TempDir, Connection) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    mail(root, "right@x.test", "cur", "1:2,S", 10);
    mail(root, "low@x.test", "new", "1", 30);
    mail(root, "low@x.test", ".Sent/cur", "2:2,S", 20);
    mail(root, "high@x.test", "cur", "1:2,S", 5);
    mail(root, "uncounted@x.test", "new", "1", 7);
    let mut conn = Connection::connect(&format!("mem://usage-drift-{name}")).unwrap();
    set(&mut conn, "right@x.test", 10);
    set(&mut conn, "low@x.test", 1);
    set(&mut conn, "high@x.test", 500);
    set(&mut conn, "nomail@x.test", 9);
    set(&mut conn, "gone@x.test", 64);
    (tmp, conn)
}

fn accounts() -> Vec<String> {
    ["right", "low", "high", "uncounted", "nomail"]
        .iter()
        .map(|l| format!("{l}@x.test"))
        .collect()
}

#[test]
fn every_folder_counts_and_deleted_accounts_do_not() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("x.test").join("u");
    for sub in ["cur", "new", "tmp", ".Sent/cur", ".Sent/new", ".Sent/tmp"] {
        std::fs::create_dir_all(home.join(sub)).unwrap();
    }
    std::fs::write(home.join("new/1"), b"abc").unwrap();
    std::fs::write(home.join("cur/2:2,S"), b"de").unwrap();
    std::fs::write(home.join(".Sent/cur/3:2,S"), b"fghij").unwrap();
    std::fs::write(home.join("tmp/4"), b"not mail yet").unwrap();
    std::fs::create_dir_all(tmp.path().join(".deleted/x.test/old/new")).unwrap();
    assert_eq!(
        maildir_usage(tmp.path()).unwrap(),
        [("u@x.test".to_string(), 10)]
    );
}

#[test]
fn a_dry_run_reports_every_counter_and_writes_nothing() {
    let (tmp, mut conn) = seeded("dry");
    let r = pass(&mut conn, tmp.path(), &accounts(), true, 1).unwrap();
    let actions: Vec<(&str, UsageDriftAction, u64)> = r
        .rows
        .iter()
        .map(|x| (x.account.as_str(), x.action, x.actual))
        .collect();
    assert_eq!(
        actions,
        [
            ("high@x.test", UsageDriftAction::WouldWrite, 5),
            ("low@x.test", UsageDriftAction::WouldWrite, 50),
            ("nomail@x.test", UsageDriftAction::WouldWrite, 0),
            ("right@x.test", UsageDriftAction::Equal, 10),
            ("uncounted@x.test", UsageDriftAction::WouldWrite, 7),
        ]
    );
    assert_eq!((r.accounts, r.differing, r.written), (5, 4, 0));
    assert_eq!(r.counter_low, ["low@x.test", "uncounted@x.test"]);
    assert_eq!(
        r.orphan_counters,
        [OrphanCounter {
            address: "gone@x.test".into(),
            counter: Some(64),
            action: UsageDriftAction::WouldWrite,
        }]
    );
    assert_eq!(r.orphans_deleted, 0);
    assert_eq!(counter(&mut conn, "low@x.test"), Some(1));
    assert_eq!(counter(&mut conn, "uncounted@x.test"), None);
    assert_eq!(counter(&mut conn, "gone@x.test"), Some(64));
    assert!(audit_rows(&mut conn).is_empty());
}

#[test]
fn a_correction_leaves_no_drift_writes_one_audit_row_each_and_is_idempotent() {
    let (tmp, mut conn) = seeded("apply");
    let r = pass(&mut conn, tmp.path(), &accounts(), false, 7).unwrap();
    assert_eq!((r.written, r.orphans_deleted, r.skipped_raced), (4, 1, 0));
    assert!(r.raced.is_empty());
    assert_eq!(counter(&mut conn, "nomail@x.test"), Some(0));
    assert_eq!(
        counter(&mut conn, "gone@x.test"),
        None,
        "the orphan counter is still there"
    );
    assert_eq!(counter(&mut conn, "high@x.test"), Some(5));
    assert_eq!(counter(&mut conn, "low@x.test"), Some(50));
    assert_eq!(counter(&mut conn, "uncounted@x.test"), Some(7));

    let mut rows = audit_rows(&mut conn);
    rows.sort_by(|a, b| a.target.cmp(&b.target));
    let seen: Vec<(&str, &str, &str, i64)> = rows
        .iter()
        .map(|f| {
            (
                f.action.as_str(),
                f.target.as_str(),
                f.detail.as_str(),
                f.occurred_at,
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (
                USAGE_CORRECTED,
                "gone@x.test",
                "before=64 written=deleted",
                7
            ),
            (
                USAGE_CORRECTED,
                "high@x.test",
                "before=500 written=5 actual=5",
                7
            ),
            (
                USAGE_CORRECTED,
                "low@x.test",
                "before=1 written=50 actual=50",
                7
            ),
            (
                USAGE_CORRECTED,
                "nomail@x.test",
                "before=9 written=0 actual=0",
                7
            ),
            (
                USAGE_CORRECTED,
                "uncounted@x.test",
                "before=none written=7 actual=7",
                7
            ),
        ]
    );

    let again = pass(&mut conn, tmp.path(), &accounts(), true, 8).unwrap();
    assert_eq!(again.differing, 0, "{again:?}");
    assert!(again.orphan_counters.is_empty(), "{again:?}");
    let second = pass(&mut conn, tmp.path(), &accounts(), false, 9).unwrap();
    assert_eq!(
        (second.written, second.orphans_deleted, second.differing),
        (0, 0, 0)
    );
    assert_eq!(
        audit_rows(&mut conn).len(),
        5,
        "a pass with nothing to do wrote audit rows"
    );
}

#[test]
fn a_counter_that_moves_during_the_measurement_is_left_for_the_next_pass() {
    let (tmp, mut conn) = seeded("raced");
    let mut arriving = Connection::connect("mem://usage-drift-raced").unwrap();
    let low = tmp.path().join("x.test").join("low");
    let r = pass_measuring(&mut conn, tmp.path(), &accounts(), false, 3, |dir| {
        let bytes = account_bytes(dir);
        if dir == low {
            // a message lands and its arrival adds to the counter while the
            // pass is between reading the counter and writing it
            arriving
                .incr_by(used_key("low@x.test").as_bytes(), 40)
                .unwrap();
        }
        bytes
    })
    .unwrap();
    assert_eq!(r.skipped_raced, 1);
    assert_eq!(r.raced, ["low@x.test"]);
    assert_eq!(
        counter(&mut conn, "low@x.test"),
        Some(41),
        "the arrival was overwritten"
    );
    assert!(
        audit_rows(&mut conn)
            .iter()
            .all(|f| f.target != "low@x.test"),
        "a skipped account has an audit row"
    );
    assert_eq!((r.written, r.orphans_deleted), (3, 1));
}

#[test]
fn a_counter_that_moved_since_it_was_read_is_neither_set_nor_deleted() {
    let mut conn = Connection::connect("mem://usage-drift-compare").unwrap();
    let key = used_key("a@x.test");
    set(&mut conn, "a@x.test", 5);
    // read as 3, now 5: someone wrote in between
    assert!(!set_if_unchanged(&mut conn, &key, Some(3), 9).unwrap());
    assert!(!delete_if_unchanged(&mut conn, &key, Some(3)).unwrap());
    assert_eq!(counter(&mut conn, "a@x.test"), Some(5));
    assert!(set_if_unchanged(&mut conn, &key, Some(5), 9).unwrap());
    assert!(delete_if_unchanged(&mut conn, &key, Some(9)).unwrap());
    assert_eq!(counter(&mut conn, "a@x.test"), None);
}

/// The shared server path: the watched transaction is what keeps an
/// arrival during the measurement, and an orphan is deleted there too.
#[tokio::test]
async fn on_the_shared_server_a_watched_write_gives_way_to_an_arrival() {
    let (_kevy, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    mail(root, "low@x.test", "new", "1", 30);
    mail(root, "high@x.test", "cur", "1:2,S", 5);
    let mut conn = Connection::connect(&url).unwrap();
    set(&mut conn, "low@x.test", 1);
    set(&mut conn, "high@x.test", 500);
    set(&mut conn, "gone@x.test", 64);
    let mut arriving = Connection::connect(&url).unwrap();
    let low = root.join("x.test").join("low");
    let users = ["low@x.test".to_string(), "high@x.test".to_string()];
    let r = pass_measuring(&mut conn, root, &users, false, 3, |dir| {
        if dir == low {
            arriving
                .incr_by(used_key("low@x.test").as_bytes(), 40)
                .unwrap();
        }
        account_bytes(dir)
    })
    .unwrap();
    assert_eq!(r.raced, ["low@x.test"]);
    assert_eq!((r.written, r.orphans_deleted), (1, 1));
    assert_eq!(counter(&mut conn, "low@x.test"), Some(41));
    assert_eq!(counter(&mut conn, "high@x.test"), Some(5));
    assert_eq!(counter(&mut conn, "gone@x.test"), None);

    // the next pass sees the arrival on disk too and settles it
    mail(root, "low@x.test", "new", "2", 40);
    let next = pass(&mut conn, root, &users, false, 4).unwrap();
    assert_eq!((next.written, next.skipped_raced), (1, 0));
    assert_eq!(counter(&mut conn, "low@x.test"), Some(70));
    let last = pass(&mut conn, root, &users, false, 5).unwrap();
    assert_eq!(
        (last.written, last.orphans_deleted, last.differing),
        (0, 0, 0)
    );
}
