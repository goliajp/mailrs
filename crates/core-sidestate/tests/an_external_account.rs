//! What a set-up screen and a sync worker need from an external
//! account row, stated as tests.
//!
//! Pure functions only — the store calls take a network connection and
//! are exercised by the fastcore suite.

use mailrs_core_sidestate::families::external_accounts::{
    AccountRow, AuthKind, Endpoint, State, Tls, colour_for, is_due, next_backoff, validate,
    with_failure, with_success,
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
