//! Deleting an account, restoring it and purging it, alike on both cores:
//! a restore before the purge gives back what each core answered before
//! the delete; the delete parks the maildir and marks the address, which
//! then cannot be taken; a purge before the retention period finds nothing
//! due; after it, a dry run counts what each store holds and the purge
//! removes it, the same in the shared store and on disk, and a second pass
//! has nothing to do. Mail dave had released for delivery stays queued
//! throughout; his scheduled send is withheld at each delete, back after
//! the restore and gone with the purge. The purge takes the fingerprint of
//! the message dave alone read, and leaves the one alice still reads.

use mailrs_core_api::client::Client;
use mailrs_core_api::method::admin::{AddAccountRequest, RestoreAccountRequest};
use mailrs_core_api::method::conversation::ListConversationsRequest;
use mailrs_core_api::types::ConversationFilter;
use mailrs_core_sidestate::families::account_purge::pending;

use super::ALICE;
use super::harness::{Cores, Diffs, FASTCORE_KEVY, PG_CORE_KEVY};
use super::intake::{Spools, envelope};
use super::purge_fixture::*;

fn keys_naming(c: &mut kevy_client::Connection, address: &str) -> Vec<String> {
    c.keys(format!("*{address}*").as_bytes())
        .expect("keys")
        .iter()
        .map(|k| String::from_utf8_lossy(k).into_owned())
        .collect()
}

fn parked(root: &str) -> usize {
    std::fs::read_dir(std::path::Path::new(root).join(".deleted/example.test"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("dave."))
        .count()
}

/// What a core answers about dave: the account list, his account with its
/// hash, and his conversations.
async fn answers(core: &Client) -> serde_json::Value {
    let req = ListConversationsRequest {
        filter: ConversationFilter {
            limit: 50,
            ..Default::default()
        },
    };
    serde_json::json!({
        "accounts": core.list_accounts().await.expect("accounts"),
        "dave": core.get_account_with_hash(DAVE).await.expect("dave"),
        "conversations": core.list_conversations(DAVE, &req).await.expect("conversations"),
    })
}

/// Delete dave and restore him on both cores: each answers as before.
async fn delete_and_restore(c: &Cores, d: &mut Diffs, held: &[Queued]) {
    let before = [answers(&c.fastcore).await, answers(&c.pg_core).await];
    d.both(
        "remove_account dave to restore",
        c.fastcore.remove_account(DAVE).await,
        c.pg_core.remove_account(DAVE).await,
        &[],
    );
    held.iter().for_each(lapse);
    let restore = RestoreAccountRequest {
        by: "admin@example.test".into(),
        parked: None,
    };
    let back = d.both(
        "restore_account dave",
        c.fastcore.restore_account(DAVE, &restore).await,
        c.pg_core.restore_account(DAVE, &restore).await,
        &[],
    );
    // one send still ahead, one whose time passed
    let counts = mailrs_core_api::method::admin::RestoreAccountResponse {
        rescheduled: 1,
        returned_as_drafts: 1,
    };
    assert_eq!(back, Some((counts, counts)));
    let after = [answers(&c.fastcore).await, answers(&c.pg_core).await];
    for (core, (b, a)) in ["fastcore", "pg-core"]
        .iter()
        .zip(before.iter().zip(after.iter()))
    {
        assert_eq!(a, b, "{core} answers differently after a restore");
    }
    let again = d.both(
        "restore_account dave once restored",
        c.fastcore.restore_account(DAVE, &restore).await,
        c.pg_core.restore_account(DAVE, &restore).await,
        &[],
    );
    assert!(again.is_none(), "a restore of a live account was accepted");
}

pub(super) async fn run(c: &Cores, d: &mut Diffs) {
    let add = AddAccountRequest {
        address: DAVE.into(),
        display_name: "Dave".into(),
        password: "pw".into(),
        password_hash: None,
    };
    d.both(
        "add_account dave",
        c.fastcore.add_account(&add).await,
        c.pg_core.add_account(&add).await,
        &[],
    );
    // dave1 reaches dave alone, dave2 dave and alice: dave is the last
    // reader of the first, and not of the second
    for (mid, to) in [(DAVE_ONLY, vec![DAVE]), (SHARED, vec![DAVE, ALICE])] {
        let mut env = envelope(None);
        env.forward_paths = to.iter().map(|a| a.to_string()).collect();
        Spools::new(c).arrive(c, &env, &mail(mid)).await;
    }
    assert_eq!(fingerprints(c, DAVE_ONLY).await, (true, true));
    assert_eq!(fingerprints(c, SHARED).await, (true, true));
    let held = queue();
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        let mut s = store(url);
        s.set(mailrs_core_sidestate::sieve_key(DAVE).as_bytes(), b"keep;")
            .expect("sieve");
        s.hset(
            mailrs_core_sidestate::families::prefs::drafts_key(DAVE).as_bytes(),
            &[(b"1".as_slice(), b"{}".as_slice())],
        )
        .expect("draft");
    }
    delete_and_restore(c, d, &held).await;
    // the restore gave the scheduled send back its time, and the one whose
    // time passed back to dave as a draft, sent to nobody
    for q in &held {
        assert_eq!(
            job(q.url, q.scheduled),
            (Some("pending".into()), Some(LATER as f64), None),
            "{}: the restore did not put the scheduled send back",
            q.url
        );
        released_untouched(q);
        for id in &q.lapsing {
            assert!(!queued(q.url, *id), "{}: a lapsed send can still go", q.url);
        }
        let drafts = drafts(q.url);
        assert_eq!(drafts.len(), 1, "{}: {drafts:?}", q.url);
        let draft = &drafts[0];
        assert_eq!(draft["to"], "Erin <erin@outside.example>", "{}", q.url);
        assert_eq!(draft["cc"], "fay@outside.example");
        assert_eq!(draft["bcc"], "gus@outside.example");
        assert_eq!(draft["subject"], "お知らせ");
        assert_eq!(
            draft["body"].as_str().map(str::trim),
            Some("see you on monday")
        );
        assert_eq!(draft["withheld_send_at"], 1_790_000_000);
        assert!(draft.get("scheduled_at").is_none(), "{draft}");
        let restored: Vec<String> = audit_details(q.url, "account.restore");
        assert!(
            restored
                .iter()
                .all(|r| r.contains("rescheduled=1 drafts=1")),
            "{}: {restored:?}",
            q.url
        );
    }

    d.both(
        "remove_account dave",
        c.fastcore.remove_account(DAVE).await,
        c.pg_core.remove_account(DAVE).await,
        &[],
    );
    // the delete stops what was not yet released, and only that
    for q in &held {
        assert_eq!(
            job(q.url, q.scheduled),
            (Some("withheld".into()), None, None),
            "{}: a scheduled send of a deleted account can still go",
            q.url
        );
        released_untouched(q);
    }
    let roots = [
        (FASTCORE_KEVY, c.fastcore_state.maildir_root.clone()),
        (PG_CORE_KEVY, c.pg_core_state.maildir_root.clone()),
    ];
    for (url, root) in &roots {
        assert!(
            pending::deleted_at(&mut store(url), DAVE)
                .expect("pending")
                .is_some(),
            "{url}"
        );
        assert_eq!(parked(root), 1, "{url}: the maildir is not parked");
    }
    let again = d.both(
        "add_account dave while pending",
        c.fastcore.add_account(&add).await,
        c.pg_core.add_account(&add).await,
        &[],
    );
    assert!(again.is_none(), "an address pending purge was handed out");

    let early = d.both(
        "account_purge before the retention period",
        c.fastcore.account_purge(false, Some(DAVE)).await,
        c.pg_core.account_purge(false, Some(DAVE)).await,
        &[],
    );
    let (early, _) = early.expect("the purge answers on both cores");
    assert_eq!(early.due, 0, "{early:?}");

    // the retention period is over
    let deleted_at = chrono::Utc::now().timestamp() - 400 * 86_400;
    for url in [FASTCORE_KEVY, PG_CORE_KEVY] {
        let mut s = store(url);
        pending::clear(&mut s, DAVE).expect("clear");
        pending::mark(&mut s, DAVE, deleted_at).expect("mark");
    }
    // the mail stores differ in kind: kevy families against tables
    let dry = d.both(
        "account_purge dry run",
        c.fastcore.account_purge(true, Some(DAVE)).await,
        c.pg_core.account_purge(true, Some(DAVE)).await,
        &["mail_store"],
    );
    let (fc, pg) = dry.expect("the dry run answers on both cores");
    assert_eq!(fc.rows.len(), 1, "{fc:?}");
    let row = &fc.rows[0];
    assert!(row.maildir_files >= 1, "{row:?}");
    let net: Vec<&str> = row.network.iter().map(|p| p.name.as_str()).collect();
    for family in [
        mailrs_core_sidestate::sieve_key("{user}"),
        mailrs_core_sidestate::families::prefs::drafts_key("{user}"),
    ] {
        assert!(
            net.contains(&family.as_str()),
            "{family} not counted: {net:?}"
        );
    }
    assert!(row.unregistered.is_empty(), "{:?}", row.unregistered);
    assert!(!row.mail_store.is_empty(), "fastcore counted no mail rows");
    let tables: Vec<&str> = pg.rows[0]
        .mail_store
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(
        tables.contains(&"messages") && tables.contains(&"mailboxes"),
        "{tables:?}"
    );
    for (url, root) in &roots {
        assert!(
            !keys_naming(&mut store(url), DAVE).is_empty(),
            "{url}: the dry run removed"
        );
        assert_eq!(parked(root), 1, "{url}: the dry run removed the maildir");
    }

    d.both(
        "account_purge",
        c.fastcore.account_purge(false, Some(DAVE)).await,
        c.pg_core.account_purge(false, Some(DAVE)).await,
        &["mail_store"],
    );
    for (url, root) in &roots {
        let left = keys_naming(&mut store(url), DAVE);
        assert!(left.is_empty(), "{url}: dave's keys remain: {left:?}");
        assert_eq!(parked(root), 0, "{url}: the parked maildir remains");
        assert!(
            pending::deleted_at(&mut store(url), DAVE)
                .expect("pending")
                .is_none()
        );
    }
    let mut embedded =
        kevy_client::Connection::Embedded(Box::new(c.fastcore_state.mailbox.store_ref().clone()));
    let left = keys_naming(&mut embedded, DAVE);
    assert!(
        left.is_empty(),
        "fastcore: dave's mail rows remain: {left:?}"
    );
    let (mailboxes,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM mailboxes WHERE user_address = $1")
            .bind(DAVE)
            .fetch_one(&c.pg_core_state.pool)
            .await
            .expect("count");
    assert_eq!(mailboxes, 0, "pg-core: dave's mailboxes remain");
    assert_eq!(
        fingerprints(c, DAVE_ONLY).await,
        (false, false),
        "dave was its last reader"
    );
    assert_eq!(
        fingerprints(c, SHARED).await,
        (true, true),
        "alice still reads it"
    );
    for q in &held {
        assert_eq!(
            job(q.url, q.scheduled),
            (None, None, None),
            "{}: the withheld send outlived the purge",
            q.url
        );
        released_untouched(q);
    }

    let quiet = d.both(
        "account_purge again",
        c.fastcore.account_purge(false, Some(DAVE)).await,
        c.pg_core.account_purge(false, Some(DAVE)).await,
        &[],
    );
    let (quiet, _) = quiet.expect("a second pass answers on both cores");
    assert!(quiet.rows.is_empty(), "{quiet:?}");
}
