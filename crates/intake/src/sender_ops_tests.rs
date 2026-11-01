use std::cell::RefCell;

use super::*;

/// A sender's rows in memory, changed the way a core changes them.
struct Rows(RefCell<Vec<SenderThread>>);

fn t(id: &str, bucket: &str, at: i64) -> SenderThread {
    SenderThread {
        thread_id: id.into(),
        bucket: bucket.into(),
        latest_date: at,
        archived: false,
        trashed: false,
        held: false,
        starred: false,
        pinned: false,
    }
}

impl SenderStore for Rows {
    fn sender_threads(&self, _: &str, _: &str) -> io::Result<Vec<SenderThread>> {
        Ok(self.0.borrow().clone())
    }
    fn set_bucket(&self, _: &str, id: &str, bucket: &str) -> io::Result<()> {
        for r in self.0.borrow_mut().iter_mut().filter(|r| r.thread_id == id) {
            r.bucket = bucket.into();
        }
        Ok(())
    }
    fn dispose(&self, _: &str, id: &str, how: Disposal, _: i64) -> io::Result<()> {
        for r in self.0.borrow_mut().iter_mut().filter(|r| r.thread_id == id) {
            match how {
                Disposal::Archive => r.archived = true,
                Disposal::Trash => r.trashed = true,
            }
        }
        Ok(())
    }
}

#[test]
fn a_reclassify_moves_only_live_mail_and_an_undo_puts_it_back() {
    let rows = Rows(RefCell::new(vec![
        t("a", "inbox", 3),
        t("b", "notifications", 2),
        t("c", "promotions", 1),
        SenderThread {
            archived: true,
            ..t("d", "inbox", 4)
        },
        t("e", "junk", 5),
        SenderThread {
            held: true,
            ..t("f", "inbox", 6)
        },
    ]));
    let moved = reclassify(&rows, "u", "s", "promotions").unwrap();
    let ids: Vec<&str> = moved.iter().map(|m| m.thread_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["a", "b"],
        "already there, archived, Junk and held stay"
    );
    let bucket = |id: &str| {
        rows.0
            .borrow()
            .iter()
            .find(|r| r.thread_id == id)
            .unwrap()
            .bucket
            .clone()
    };
    assert_eq!(bucket("a"), "promotions");
    assert_eq!(bucket("d"), "inbox");

    // the reader moved one back by hand before undoing: it stays where it is
    rows.set_bucket("u", "b", "inbox").unwrap();
    assert_eq!(restore(&rows, "u", "s", "promotions", &moved).unwrap(), 1);
    assert_eq!(bucket("a"), "inbox");
    assert_eq!(bucket("b"), "inbox");
}

#[test]
fn retention_keeps_the_newest_and_the_young_and_never_the_starred() {
    let threads = vec![
        t("new", "inbox", 1_000),
        t("mid", "inbox", 900),
        SenderThread {
            starred: true,
            ..t("starred", "inbox", 100)
        },
        t("old", "inbox", 100),
        SenderThread {
            archived: true,
            ..t("filed", "inbox", 50)
        },
    ];
    let keep_two = SenderRule {
        keep_latest: Some(2),
        ..Default::default()
    };
    assert_eq!(plan_retention(&threads, &keep_two, 1_000), vec!["old"]);
    let young = SenderRule {
        older_than_days: Some(1),
        ..Default::default()
    };
    let now = 100 + 86_400 + 1;
    assert_eq!(plan_retention(&threads, &young, now), vec!["old"]);
    assert!(plan_retention(&threads, &SenderRule::default(), now).is_empty());
}

#[test]
fn the_sweep_disposes_by_age_and_finds_nothing_the_second_time() {
    use mailrs_core_sidestate::families::sender_rules::put;
    let mut c = kevy_client::Connection::connect("mem://intake-sender-sweep").unwrap();
    let rule = SenderRule {
        older_than_days: Some(1),
        disposal: Disposal::Trash,
        ..Default::default()
    };
    put(&mut c, "u@x", "s@x", &rule).unwrap();
    let rows = Rows(RefCell::new(vec![
        t("old", "inbox", 0),
        t("new", "inbox", 200_000),
    ]));
    let now = 200_001;
    assert_eq!(sweep(&rows, &mut c, now).unwrap(), 1);
    assert!(
        rows.0
            .borrow()
            .iter()
            .find(|r| r.thread_id == "old")
            .unwrap()
            .trashed
    );
    assert_eq!(sweep(&rows, &mut c, now).unwrap(), 0, "converged");
}

#[test]
fn a_delivery_applies_keep_latest_only() {
    use mailrs_core_sidestate::families::sender_rules::put;
    let mut c = kevy_client::Connection::connect("mem://intake-sender-delivery").unwrap();
    let rule = SenderRule {
        keep_latest: Some(1),
        older_than_days: Some(1),
        ..Default::default()
    };
    put(&mut c, "u@x", "s@x", &rule).unwrap();
    let rows = Rows(RefCell::new(vec![
        t("a", "inbox", 300_000),
        t("b", "inbox", 299_000),
    ]));
    after_delivery(&rows, Some(&mut c), "u@x", "Shop <S@x>", 300_001);
    let archived: Vec<bool> = rows.0.borrow().iter().map(|r| r.archived).collect();
    assert_eq!(archived, vec![false, true]);
}
