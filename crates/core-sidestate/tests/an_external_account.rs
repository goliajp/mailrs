//! What a set-up screen and a sync worker need from an external
//! account row, stated as tests.
//!
//! Pure functions only — the store calls take a network connection and
//! are exercised by the fastcore suite.

use mailrs_core_sidestate::families::external_accounts::{
    AccountRow, AuthKind, Endpoint, FIRST_SYNC_NOTE, State, Tls, colour_for, is_due, next_backoff,
    validate, with_failure, with_paused, with_success,
};

fn a_row() -> AccountRow {
    AccountRow {
        id: "acc_1".into(),
        email: "someone@gmail.com".into(),
        display_name: "Work".into(),
        provider: "gmail".into(),
        incoming: Endpoint {
            protocol: "imap".into(),
            host: "imap.gmail.com".into(),
            port: 993,
            tls: Tls::Implicit,
        },
        outgoing: Endpoint {
            protocol: "smtp".into(),
            host: "smtp.gmail.com".into(),
            port: 587,
            tls: Tls::StartTls,
        },
        auth: AuthKind::OAuth2,
        ..AccountRow::default()
    }
}

#[test]
fn a_row_round_trips_through_json() {
    let row = a_row();
    let back: AccountRow = serde_json::from_str(&serde_json::to_string(&row).unwrap()).unwrap();
    assert_eq!(back, row);
}

/// Rows written before a field existed must load, not fail the whole
/// account list.
#[test]
fn an_older_row_loads_with_defaults() {
    let minimal = r#"{"id":"a","email":"x@y.z","display_name":"","provider":"custom",
        "incoming":{"protocol":"imap","host":"h","port":993,"tls":"implicit"},
        "outgoing":{"protocol":"smtp","host":"h","port":587,"tls":"starttls"},
        "auth":"password"}"#;
    let row: AccountRow = serde_json::from_str(minimal).expect("older row");
    assert_eq!(row.state, State::Ok);
    assert_eq!(row.last_error, None);
    assert_eq!(row.failures, 0);
}

/// The secret is never a field on the row. A row is listed, logged and
/// sent to three clients; a token that rides along in it leaks by
/// every one of those paths.
#[test]
fn the_row_carries_no_secret() {
    let json = serde_json::to_string(&a_row()).unwrap();
    for forbidden in ["password", "token", "secret", "refresh"] {
        assert!(
            !json.to_lowercase().contains(&format!("\"{forbidden}\"")),
            "{forbidden} is a field on the row: {json}"
        );
    }
}

#[test]
fn a_usable_account_validates() {
    assert_eq!(validate(&a_row()), Ok(()));
}

#[test]
fn what_cannot_work_is_refused_with_the_reason() {
    /// A word the message must contain, and the thing to break.
    type Case = (&'static str, fn(&mut AccountRow));

    let cases: [Case; 5] = [
        ("email", |r| r.email = "not-an-address".into()),
        ("host", |r| r.incoming.host = String::new()),
        ("port", |r| r.incoming.port = 0),
        ("protocol", |r| r.incoming.protocol = "gopher".into()),
        ("protocol", |r| r.outgoing.protocol = "imap".into()),
    ];
    for (word, break_it) in cases {
        let mut row = a_row();
        break_it(&mut row);
        let err = validate(&row).expect_err("accepted something unusable");
        assert!(
            err.to_lowercase().contains(word),
            "{err} did not mention {word}"
        );
    }
}

/// Sending over a protocol that only reads is the mistake a hand-typed
/// custom account makes, and it fails at the first send rather than at
/// set-up unless it is checked here.
#[test]
fn a_pop_account_may_still_send_over_smtp() {
    let mut row = a_row();
    row.incoming.protocol = "pop3".into();
    row.incoming.port = 995;
    assert_eq!(validate(&row), Ok(()));
}

/// Every account gets a colour without anyone choosing one, and the
/// same account keeps it — the dot beside a row is useless if it moves.
#[test]
fn a_colour_is_assigned_and_is_stable() {
    let a = colour_for("acc_1");
    assert_eq!(a, colour_for("acc_1"));
    assert!(a.starts_with('#') && a.len() == 7, "{a}");
    let others: Vec<_> = (0..6).map(|i| colour_for(&format!("acc_{i}"))).collect();
    assert!(
        others
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1
    );
}

#[test]
fn a_never_synced_account_is_due_at_once() {
    let row = a_row();
    assert!(is_due(&row, 1_000_000), "a new account waited to sync");
}

#[test]
fn a_failing_account_backs_off_and_a_success_clears_it() {
    let mut row = a_row();
    let mut last = 0;
    for _ in 0..5 {
        row = with_failure(row, 1_000, "connection refused");
        let wait = next_backoff(row.failures);
        assert!(wait >= last, "backoff went backwards");
        last = wait;
    }
    assert_eq!(row.state, State::Error);
    assert_eq!(row.last_error.as_deref(), Some("connection refused"));

    row = with_success(row, 2_000);
    assert_eq!(row.failures, 0);
    assert_eq!(row.state, State::Ok);
    assert_eq!(
        row.last_error, None,
        "a recovered account still showed last week's failure"
    );
}

/// Backoff is bounded. An account that has been failing for a month
/// must still be retried today, because the fix is usually at the other
/// end and nobody tells us when it lands.
#[test]
fn backoff_stops_growing() {
    assert!(next_backoff(1000) <= 6 * 3600);
    assert!(next_backoff(1000) >= next_backoff(3));
}

/// A password that stopped working is the person's to fix, and it needs
/// a different word from "the server was down" — one is a button, the
/// other is waiting.
#[test]
fn a_rejected_credential_asks_for_attention_rather_than_retrying_forever() {
    let row = with_failure(a_row(), 1_000, "AUTHENTICATIONFAILED");
    assert_eq!(row.state, State::NeedsAuth);
    assert!(
        !is_due(&row, 1_000_000_000),
        "a broken password was retried on a timer"
    );
}

/// Switched off by its owner, and honoured.
///
/// `Paused` was a state with a reader and no writer until 2026-08-23:
/// `is_due` respected it and the account list rendered it, and nothing
/// anywhere could set it. So this is the first test it has ever had.
#[test]
fn a_paused_account_is_not_read() {
    let mut row = a_row();
    row.state = State::Paused;
    row.last_sync = 0;
    assert!(
        !is_due(&row, 1_000_000_000),
        "a never-synced account is due at once, and pausing has to beat that"
    );
    row.last_sync = 1;
    row.next_attempt = 0;
    assert!(!is_due(&row, 1_000_000_000), "a paused account was read");
}

/// Pausing stops the reading and nothing else.
///
/// The credential is still held and still valid; refusing to send from
/// an address somebody owns would be a second meaning nobody asked
/// for. Nothing in the row says otherwise — this pins that.
#[test]
fn pausing_leaves_the_way_out_alone() {
    let before = a_row();
    let row = with_paused(before.clone(), true);
    assert_eq!(row.state, State::Paused);
    assert_eq!(row.email, before.email);
    assert_eq!(row.outgoing, before.outgoing, "the way out was changed");
    assert_eq!(row.username, before.username);
}

/// Resuming is not "un-pause and wait": somebody who pressed it is
/// waiting for mail, and the failure that preceded the pause is no
/// longer what the row is about.
#[test]
fn resuming_makes_it_due_at_once() {
    let failed = with_failure(a_row(), 1_000, "connection refused");
    assert!(failed.next_attempt > 1_000, "a failure set no retry time");
    let back = with_paused(with_paused(failed, true), false);
    assert_eq!(back.state, State::Ok);
    assert_eq!(
        back.next_attempt, 0,
        "resuming left it waiting out a backoff"
    );
    assert_eq!(back.last_error, None, "an old reason survived the resume");
    assert!(is_due(&back, 1_000), "a resumed account was not read");
}

/// A rejected credential is not something pausing can fix, and
/// resuming one would put it back on a timer that cannot succeed.
#[test]
fn a_rejected_credential_cannot_be_paused_or_resumed() {
    let broken = with_failure(a_row(), 1_000, "AUTHENTICATIONFAILED");
    assert_eq!(broken.state, State::NeedsAuth);
    assert_eq!(with_paused(broken.clone(), true).state, State::NeedsAuth);
    let resumed = with_paused(broken.clone(), false);
    assert_eq!(
        resumed.state,
        State::NeedsAuth,
        "a refused password was resumed"
    );
    assert_eq!(
        resumed.last_error, broken.last_error,
        "the reason was cleared"
    );
}

/// A re-read that was running when somebody paused is not still
/// running afterwards, and the row must not go on saying it is.
#[test]
fn pausing_clears_what_it_was_doing() {
    let mut row = a_row();
    row.last_sync = 1_000;
    row.progress = Some("reading Inbox again from the start".into());
    assert_eq!(with_paused(row.clone(), true).progress, None);
    // Resuming an account that has synced before has nothing to wait
    // for either. The never-synced case is the exception, and it has
    // its own test.
    assert_eq!(with_paused(row, false).progress, None);
}

/// A pause that outlives a failure must not resurrect the retry timer.
#[test]
fn pausing_a_failing_account_stops_the_retries() {
    let failed = with_failure(a_row(), 1_000, "connection refused");
    assert!(
        is_due(&failed, failed.next_attempt),
        "a failure stopped retrying"
    );
    let mut paused = failed.clone();
    paused.state = State::Paused;
    assert!(
        !is_due(&paused, 1_000_000_000),
        "pausing a failing account left it on the backoff timer"
    );
}

/// A newly connected account says what it is waiting for.
///
/// The sync loop rests for up to five minutes when nothing has been
/// due, so connecting an account and seeing nothing happen is the
/// normal case rather than a fault — and the screen said nothing about
/// it at all. The note is cleared by the first success, like every
/// other progress note; a row still saying it a day later is a row
/// nobody updated.
#[test]
fn the_first_sync_note_goes_away_when_it_syncs() {
    let mut row = a_row();
    row.progress = Some(FIRST_SYNC_NOTE.to_string());
    row.last_sync = 0;
    assert!(
        is_due(&row, 1),
        "a never-synced account must be due at once"
    );

    let done = with_success(row, 1_000);
    assert_eq!(done.progress, None, "the waiting note outlived the wait");
    assert_eq!(done.last_sync, 1_000);
}

/// And a failure does not turn it into the reason: one is work, the
/// other is a fault, and the row has a field for each.
#[test]
fn a_failure_does_not_become_the_waiting_note() {
    let mut row = a_row();
    row.progress = Some(FIRST_SYNC_NOTE.to_string());
    let failed = with_failure(row, 1_000, "connection refused");
    assert!(
        failed.last_error.is_some(),
        "a failure with no reason is a row that says nothing"
    );
    assert_ne!(
        failed.progress.as_deref(),
        Some("connection refused"),
        "the reason was written into the progress note"
    );
}

/// Resuming an account that has never synced puts it back in the wait
/// it was in — so it must go on saying so.
///
/// `with_paused` cleared the note unconditionally at first, which is
/// right for a re-read that a pause interrupted and wrong here: the
/// row went silent again for as long as the loop's rest, which is the
/// silence the note exists to break.
#[test]
fn resuming_a_never_synced_account_still_says_it_is_waiting() {
    let mut row = a_row();
    row.last_sync = 0;
    row.progress = Some(FIRST_SYNC_NOTE.to_string());

    let paused = with_paused(row, true);
    assert_eq!(
        paused.progress, None,
        "a paused account said it was working"
    );

    let back = with_paused(paused, false);
    assert_eq!(
        back.progress.as_deref(),
        Some(FIRST_SYNC_NOTE),
        "a resumed account went silent while it waited"
    );
}

/// An account that has synced before has nothing to wait for, so
/// resuming it says nothing rather than inventing a first read.
#[test]
fn resuming_an_account_that_has_synced_says_nothing() {
    let mut row = a_row();
    row.last_sync = 1_000;
    let back = with_paused(with_paused(row, true), false);
    assert_eq!(back.progress, None);
}
