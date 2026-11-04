//! Curated ham for the v2 spam/ham corpus: which kept conversations are
//! taken, and why the others are not. Against a real kevy-server for the
//! shared facts, an embedded store and a maildir on disk. Every message is
//! synthetic.

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use mailrs_fastcore::FastcoreState;
use mailrs_mailbox_kevy::{KevyMailboxStore, ThreadRow, UserMessageFacts};
use mailrs_triage::spam::{Corpus, keys};
use tower::ServiceExt;

const USER: &str = "bob@x.com";
const DAY: i64 = 86_400;

struct T<'a> {
    tid: &'a str,
    bucket: &'a str,
    from: &'a str,
    age_days: i64,
    dmarc: &'a str,
}

fn t<'a>(tid: &'a str, from: &'a str) -> T<'a> {
    T {
        tid,
        bucket: "inbox",
        from,
        age_days: 30,
        dmarc: "pass",
    }
}

fn add(store: &KevyMailboxStore, root: &Path, now: i64, x: &T<'_>) {
    let mid = format!("{}@mail.example", x.tid);
    let raw = format!(
        "Authentication-Results: mail.golia.ai;\r\n\tspf=pass;\r\n\tdkim=pass;\r\n\tarc=none;\r\n\tdmarc={}\r\n\
         Received: from mx.sender.example (203.0.113.9:4000)\r\n\tby mail.golia.ai with ESMTP\r\n\
         From: Sender <{}>\r\nTo: {USER}\r\nSubject: {}\r\nMessage-ID: <{mid}>\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\nmonthly statement is ready\r\n",
        x.dmarc, x.from, x.tid
    );
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(raw.as_bytes()).unwrap().0;
    let date = now - x.age_days * DAY;
    let uid = store.allocate_uid(USER, &mid).unwrap();
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob, "sender": format!("Sender <{}>", x.from),
        "recipients": USER, "subject": x.tid, "date": date, "internal_date": date, "size": raw.len(),
        "flags": 0, "message_id": mid, "in_reply_to": "", "thread_id": x.tid, "modseq": 1
    });
    let category = match x.bucket {
        "junk" => "spam",
        other => other,
    };
    store
        .upsert_thread(
            USER,
            &ThreadRow {
                account_id: String::new(),
                thread_id: x.tid.into(),
                subject: x.tid.into(),
                senders_csv: x.from.into(),
                count: 1,
                unread_count: 0,
                latest_date: date,
                latest_preview: String::new(),
                category: category.into(),
                importance_level: "normal".into(),
                importance_score: 0.0,
                requires_action: false,
                pinned: false,
                archived: false,
                quarantined: false,
                has_action: false,
                sent_count: 0,
                snoozed_until: 0,
                trashed_at: 0,
                starred: false,
            },
        )
        .unwrap();
    store
        .upsert_user_message(
            USER,
            x.tid,
            &mid,
            date,
            &serde_json::to_vec(&wire).unwrap(),
            &UserMessageFacts {
                blob_ref: &blob,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
}

/// The reader's own answer in `tid`, a day after the conversation began.
fn reply(store: &KevyMailboxStore, root: &Path, now: i64, tid: &str, age_days: i64) {
    let mid = format!("{tid}-re@x.com");
    let raw = format!(
        "From: Bob <{USER}>\r\nSubject: Re: {tid}\r\nMessage-ID: <{mid}>\r\n\r\nthanks\r\n"
    );
    let md = mailrs_maildir::Maildir::open(root.join("x.com/bob"));
    let blob = md.deliver(raw.as_bytes()).unwrap().0;
    let date = now - (age_days - 1) * DAY;
    let uid = store.allocate_uid(USER, &mid).unwrap();
    let wire = serde_json::json!({
        "id": 0, "mailbox_id": 0, "uid": uid, "blob_ref": blob, "sender": format!("Bob <{USER}>"),
        "recipients": "a@x", "subject": tid, "date": date, "internal_date": date, "size": raw.len(),
        "flags": 0, "message_id": mid, "in_reply_to": "", "thread_id": tid, "modseq": 1
    });
    store
        .upsert_user_message(
            USER,
            tid,
            &mid,
            date,
            &serde_json::to_vec(&wire).unwrap(),
            &UserMessageFacts {
                blob_ref: &blob,
                uid,
                flags: 0,
                modseq: 1,
            },
        )
        .unwrap();
}

async fn rebuild(state: &Arc<FastcoreState>, query: &str) -> serde_json::Value {
    let res = mailrs_fastcore::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!(
                    "/v1/admin/maintenance:bayes-rebuild-v2?source=curated&{query}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    serde_json::from_slice(
        &axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap()
}

fn ham_msgs(url: &str) -> String {
    let mut c = kevy_client::Connection::connect(url).unwrap();
    c.hget(keys(Corpus::V2).meta, b"ham_msgs")
        .unwrap()
        .map(|v| String::from_utf8(v).unwrap())
        .unwrap_or_default()
}

#[tokio::test]
async fn curated_ham_takes_only_what_readers_kept_from_trusted_senders() {
    let (_k, url) = mailrs_test_docker::kevy::kevy_server().await;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("maildir");
    for leaf in ["new", "cur", "tmp"] {
        std::fs::create_dir_all(root.join("x.com/bob").join(leaf)).unwrap();
    }
    // one test in its own integration-test process owns this variable
    unsafe { std::env::set_var("MAILRS_MAILDIR", &root) };
    let store = KevyMailboxStore::new(Arc::new(
        kevy_embedded::Store::open(kevy_embedded::Config::default()).unwrap(),
    ));
    store.ensure_thread_table();
    store.ensure_admin_indexes();
    store
        .upsert_account(USER, r#"{"address":"bob@x.com","active":true}"#)
        .unwrap();
    let now = chrono::Utc::now().timestamp();
    let recent = T {
        age_days: 3,
        ..t("g4", "a@good.example")
    };
    let failed = T {
        dmarc: "fail",
        ..t("f1", "a@fail.example")
    };
    let in_junk = T {
        bucket: "junk",
        ..t("j4", "a@junkdom.example")
    };
    let threads = vec![
        t("g1", "a@good.example"),
        t("g2", "b@good.example"),
        t("g3", "a@mail.good.example"),
        recent,
        t("w1", "a@few.example"),
        t("w2", "a@few.example"),
        failed,
        t("f2", "a@fail.example"),
        t("f3", "a@fail.example"),
        t("j1", "a@junkdom.example"),
        t("j2", "a@junkdom.example"),
        t("j3", "a@junkdom.example"),
        in_junk,
        t("h1", "a@held.example"),
        t("h2", "a@held.example"),
        t("h3", "a@held.example"),
        t("m1", "a@marked.example"),
        t("m2", "a@marked.example"),
        t("m3", "a@marked.example"),
        t("b1", "a@black.example"),
        t("b2", "a@black.example"),
        t("b3", "a@black.example"),
        t("c1", "a@corpus.example"),
        t("c2", "a@corpus.example"),
        t("c3", "a@corpus.example"),
        t("x1", "a@fraud.example"),
        t("x2", "a@fraud.example"),
        t("x3", "a@fraud.example"),
        t("o1", USER),
        t("t1", "a@trash.example"),
        t("t2", "a@trash.example"),
        t("t3", "a@trash.example"),
        // the reader never answered this sender
        t("n1", "a@noreply.example"),
        t("n2", "a@noreply.example"),
        t("n3", "a@noreply.example"),
        // answered elsewhere: the contact record says the reader wrote to them
        t("r1", "a@contact.example"),
        t("r2", "a@contact.example"),
        t("r3", "a@contact.example"),
    ];
    for x in &threads {
        add(&store, &root, now, x);
    }
    // every other conversation the reader answered in place
    for x in &threads {
        let elsewhere = ["n1", "n2", "n3", "r1", "r2", "r3"].contains(&x.tid);
        if !elsewhere && x.from != USER {
            reply(&store, &root, now, x.tid, x.age_days);
        }
    }
    store.set_quarantined(USER, "h1", true).unwrap();
    store.set_trashed(USER, "t1", true, now).unwrap();
    store
        .set_fraud_verdict("x1@mail.example", r#"{"score":5}"#)
        .unwrap();

    let mut c = kevy_client::Connection::connect(&url).unwrap();
    // the reader filed a conversation from a subdomain of marked.example as junk
    mailrs_core_sidestate::families::screening::add_observation(
        &mut c,
        mailrs_reputation::Observation {
            id: String::new(),
            at: now - 100 * DAY,
            source: mailrs_reputation::Source::UserJunk,
            kind: mailrs_reputation::Kind::SenderDomain,
            value: "news.marked.example".into(),
            stance: mailrs_reputation::Stance::Deny,
            retracts: None,
            reason: "moved conversation m0 to Junk".into(),
            actor: Some(USER.into()),
        },
    )
    .unwrap();
    let black = mailrs_core_sidestate::families::sender_lists::blacklist_key(USER);
    c.sadd(black.as_bytes(), &[b"spammer@black.example".as_slice()])
        .unwrap();
    mailrs_core_sidestate::families::contacts::record_sent_to(&mut c, USER, "a@contact.example")
        .unwrap();
    c.sadd(keys(Corpus::Legacy).corpus_spam, &[b"c1".as_slice()])
        .unwrap();

    let mut state = FastcoreState::new(store);
    state.net_url = Some(url.clone());
    state.authserv_id = Some("mail.golia.ai".into());
    let state = Arc::new(state);

    let dry = rebuild(&state, "").await;
    assert_eq!(dry["dry_run"], true, "{dry}");
    // o1, the reader's own mail only, is not even listed: the axis the
    // candidates come from leaves out conversations the reader only sent
    assert_eq!(dry["candidates"], 36, "{dry}");
    assert_eq!(dry["kept"], 12, "g1-3, f2-3, x2-3, t2-3, r1-3: {dry}");
    let r = &dry["rejected"];
    assert_eq!(r["held_or_trashed"], 2, "h1, t1: {dry}");
    assert_eq!(r["recent"], 1, "g4: {dry}");
    assert_eq!(r["own_only"], 0, "{dry}");
    assert_eq!(r["fraud_verdict"], 1, "x1: {dry}");
    assert_eq!(
        r["domain_marked_junk"], 9,
        "marked by the reader, blacklisted, in the spam corpus: {dry}"
    );
    assert_eq!(r["domain_in_junk_or_held"], 5, "j1-3, h2-3: {dry}");
    assert_eq!(r["domain_too_few"], 2, "w1-2: {dry}");
    assert_eq!(r["dmarc_not_pass"], 1, "f1: {dry}");
    assert_eq!(r["no_reply"], 3, "n1-3: {dry}");
    assert_eq!(ham_msgs(&url), "", "a dry run wrote");

    // the caps, in a fixed order
    let capped = rebuild(&state, "per_domain_cap=1&total_cap=3").await;
    assert_eq!(capped["kept"], 3, "{capped}");
    assert_eq!(
        capped["rejected"]["per_domain_cap"].as_u64().unwrap()
            + capped["rejected"]["total_cap"].as_u64().unwrap(),
        9,
        "{capped}"
    );
    assert_eq!(
        capped,
        rebuild(&state, "per_domain_cap=1&total_cap=3").await,
        "the selection is not reproducible"
    );

    // pages cover the roster once each, and train it as ham
    let p1 = rebuild(&state, "dry_run=false&limit=6").await;
    assert_eq!(p1["next_skip"], 6, "{p1}");
    let p2 = rebuild(&state, "dry_run=false&limit=6&skip=6").await;
    assert_eq!(p2["done"], true, "{p2}");
    assert_eq!(ham_msgs(&url), "12");
    for kept in [
        "g1", "g2", "g3", "f2", "f3", "x2", "x3", "t2", "t3", "r1", "r2", "r3",
    ] {
        assert_eq!(
            mailrs_triage::spam::trained_as_spam(&mut c, Corpus::V2, kept),
            Some(false),
            "{kept} was not learnt as ham"
        );
    }
    for left in [
        "n1", "g4", "w1", "f1", "j1", "h2", "m1", "b1", "c2", "x1", "o1", "t1",
    ] {
        assert_eq!(
            mailrs_triage::spam::trained_as_spam(&mut c, Corpus::V2, left),
            None,
            "{left} was learnt"
        );
    }
    // the legacy corpus is not touched
    assert_eq!(
        c.hget(keys(Corpus::Legacy).meta, b"ham_msgs").unwrap(),
        None
    );
}
