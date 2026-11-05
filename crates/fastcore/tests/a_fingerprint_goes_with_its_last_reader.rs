//! A message's template fingerprint (`L:t:{mid}`, and its segment row
//! `L:ts:{mid}`) is kept once per message, not per reader. Purging one
//! reader leaves it while another reader still has the message, and the
//! purge of the last reader removes it. A re-judge round's gathered
//! conversations (`rejudge:candidates:{obs}`) lose the purged reader's and
//! keep everyone else's.
//!
//! Before, the fingerprint outlived every reader, and the gathered entries
//! of a purged account stayed in the round.

mod purge_support;

use purge_support::*;

const OBSERVATION: &str = "obs-fp";

fn read(state: &mailrs_fastcore::FastcoreState, user: &str, mid: &str) {
    let facts = mailrs_mailbox_kevy::ledger::LedgerFacts {
        message_id: mid.into(),
        user: user.into(),
        from_addr: "carol@y.com".into(),
        from_domain: "y.com".into(),
        from_reg: "y.com".into(),
        arrived: chrono::Utc::now().timestamp(),
        ..Default::default()
    };
    state.mailbox.record_ledger(&facts).unwrap();
}

fn fingerprint(state: &mailrs_fastcore::FastcoreState, mid: &str) {
    let t = mailrs_report::Template {
        simhash: 0x1234_5678_9abc_def0,
        sender_domain: "y.com".into(),
        ..Default::default()
    };
    let now = chrono::Utc::now().timestamp();
    assert!(state.mailbox.record_template(mid, &t, now).unwrap());
}

/// Whether `mid` still has its fingerprint row and its segment row.
fn kept(mail: &mut kevy_client::Connection, mid: &str) -> (bool, bool) {
    let has = |c: &mut kevy_client::Connection, k: String| c.exists(&[k.as_bytes()]).unwrap() == 1;
    (
        has(mail, format!("L:t:{mid}")),
        has(mail, format!("L:ts:{mid}")),
    )
}

fn gathered(mail: &mut kevy_client::Connection) -> Vec<String> {
    let key = format!("rejudge:candidates:{OBSERVATION}");
    let mut fields: Vec<String> = mail
        .hgetall(key.as_bytes())
        .unwrap()
        .chunks(2)
        .map(|p| String::from_utf8_lossy(&p[0]).into_owned())
        .collect();
    fields.sort();
    fields
}

async fn delete(f: &Fixture, who: &str) {
    let (status, _) = call(
        &f.state,
        "DELETE",
        &format!("/v1/admin/accounts/{who}"),
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");
}

#[tokio::test]
async fn a_shared_messages_fingerprint_stays_while_a_reader_remains() {
    // SAFETY: the only test in this binary, before anything reads it
    unsafe { std::env::set_var("MAILRS_DELETED_ACCOUNT_DAYS", "0") };
    let f = fixture().await;
    let mut mail = mail_conn(&f.state);
    read(&f.state, BOB, "shared@y.com");
    read(&f.state, ALICE, "shared@y.com");
    read(&f.state, BOB, "solo@y.com");
    fingerprint(&f.state, "shared@y.com");
    fingerprint(&f.state, "solo@y.com");
    let candidate = serde_json::json!({ "user": "", "tid": "", "mid": "", "from": "",
        "subject": "", "rules": [], "verdict": {}, "arrived": null })
    .to_string();
    for field in [format!("{BOB}\tt-bob"), format!("{ALICE}\tt-alice")] {
        mail.hset(
            format!("rejudge:candidates:{OBSERVATION}").as_bytes(),
            &[(field.as_bytes(), candidate.as_bytes())],
        )
        .unwrap();
    }
    assert_eq!(kept(&mut mail, "shared@y.com"), (true, true));
    assert_eq!(kept(&mut mail, "solo@y.com"), (true, true));

    delete(&f, BOB).await;
    assert_eq!(
        kept(&mut mail, "shared@y.com"),
        (true, true),
        "alice still reads it"
    );
    assert_eq!(
        kept(&mut mail, "solo@y.com"),
        (false, false),
        "bob was its only reader"
    );
    assert_eq!(gathered(&mut mail), [format!("{ALICE}\tt-alice")]);

    delete(&f, ALICE).await;
    assert_eq!(
        kept(&mut mail, "shared@y.com"),
        (false, false),
        "the last reader is gone"
    );
    assert!(gathered(&mut mail).is_empty());
}
