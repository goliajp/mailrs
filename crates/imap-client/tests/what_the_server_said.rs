//! Reading a server's answers, and deciding what to ask next.

use mailrs_imap_client::{FetchPlan, FolderState, Untagged, parse_line, plan_fetch};

fn opened(lines: &[&str]) -> FolderState {
    let mut s = FolderState::default();
    for l in lines {
        if let Some(u) = parse_line(l) {
            s.apply(&u);
        }
    }
    s
}

#[test]
fn a_select_response_is_read() {
    let s = opened(&[
        "* 231 EXISTS",
        "* 0 RECENT",
        "* OK [UIDVALIDITY 1234567890] UIDs valid",
        "* OK [UIDNEXT 4392] Predicted next UID",
        "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)",
    ]);
    assert_eq!(s.exists, 231);
    assert_eq!(s.uidvalidity, Some(1_234_567_890));
    assert_eq!(s.uidnext, Some(4392));
}

/// The one that loses mail silently.
///
/// RFC 3501 §2.3.1.1: a uid is meaningful only within a uidvalidity.
/// When the server changes it, every remembered number means nothing —
/// and a client that carries on from its old highest uid downloads
/// nothing and reports success forever.
#[test]
fn a_changed_uidvalidity_forces_a_full_resync() {
    let mut s = opened(&["* OK [UIDVALIDITY 999] .", "* OK [UIDNEXT 4392] ."]);
    // Synced before, under a *different* number. Leaving this unset
    // would take the never-synced branch instead, and the test would
    // pass with the rule it names deleted — which is what a fault
    // injection caught it doing.
    s.remembered_uidvalidity = Some(42);
    let plan = plan_fetch(&s, Some(4300)).expect("something to do");
    assert!(
        matches!(plan, FetchPlan::Everything { .. }),
        "carried on from a uid that no longer means anything: {plan:?}"
    );
}

#[test]
fn an_unchanged_uidvalidity_asks_only_for_what_is_new() {
    let mut s = opened(&["* OK [UIDVALIDITY 999] .", "* OK [UIDNEXT 4392] ."]);
    s.remembered_uidvalidity = Some(999);
    let plan = plan_fetch(&s, Some(4300)).expect("something to do");
    assert_eq!(plan.range(), "4301:*");
}

#[test]
fn a_folder_seen_for_the_first_time_asks_for_everything() {
    let s = opened(&["* OK [UIDVALIDITY 999] .", "* 12 EXISTS"]);
    assert!(matches!(
        plan_fetch(&s, None),
        Some(FetchPlan::Everything { .. })
    ));
}

/// Nothing new is not an error and not a reason to ask again.
#[test]
fn an_unchanged_folder_asks_for_nothing() {
    let mut s = opened(&["* OK [UIDVALIDITY 999] .", "* OK [UIDNEXT 4392] ."]);
    s.remembered_uidvalidity = Some(999);
    assert!(plan_fetch(&s, Some(4391)).is_none());
}

#[test]
fn a_fetch_response_yields_the_uid_and_the_flags() {
    let Some(Untagged::Fetch(f)) =
        parse_line("* 12 FETCH (UID 4390 FLAGS (\\Seen \\Answered) RFC822.SIZE 2048 BODY[] {14}")
    else {
        panic!("not read as a fetch");
    };
    assert_eq!(f.uid, Some(4390));
    assert!(f.seen);
    assert!(f.answered);
    assert_eq!(f.size, Some(2048));
    assert_eq!(f.literal_len, Some(14));
}

/// A flag list without `\Seen` means unread, and getting this backwards
/// marks somebody's whole mailbox read on first sync.
#[test]
fn absent_flags_are_absent_rather_than_assumed() {
    let Some(Untagged::Fetch(f)) = parse_line("* 1 FETCH (UID 7 FLAGS ())") else {
        panic!("not read as a fetch");
    };
    assert!(!f.seen);
    assert!(!f.answered);
    assert!(!f.deleted);
}

#[test]
fn a_deleted_message_says_so() {
    let Some(Untagged::Fetch(f)) = parse_line("* 1 FETCH (UID 7 FLAGS (\\Deleted \\Seen))") else {
        panic!("not read as a fetch");
    };
    assert!(f.deleted);
    assert!(f.seen);
}

#[test]
fn a_list_response_yields_the_folder_and_what_it_is_for() {
    let Some(Untagged::List(l)) =
        parse_line("* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Sent Mail\"")
    else {
        panic!("not read as a list");
    };
    assert_eq!(l.name, "[Gmail]/Sent Mail");
    assert_eq!(l.delimiter.as_deref(), Some("/"));
    assert!(l.is_sent);
    assert!(!l.selectable_is_false);
}

/// `\Noselect` names a folder that cannot be opened. Trying anyway is
/// an error per folder, every sync, forever.
#[test]
fn a_folder_that_cannot_be_opened_says_so() {
    let Some(Untagged::List(l)) = parse_line("* LIST (\\Noselect \\HasChildren) \"/\" \"[Gmail]\"")
    else {
        panic!("not read as a list");
    };
    assert!(l.selectable_is_false);
}

#[test]
fn a_tagged_answer_is_told_from_an_untagged_one() {
    assert!(parse_line("a001 OK SELECT completed").is_none());
    assert!(parse_line("* 231 EXISTS").is_some());
}

/// The failure that must not be retried on a timer.
#[test]
fn a_rejected_login_is_recognised_by_its_response_code() {
    assert!(mailrs_imap_client::is_authentication_failure(
        "a001 NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)"
    ));
    assert!(mailrs_imap_client::is_authentication_failure(
        "a001 NO [AUTHORIZATIONFAILED] cannot authenticate"
    ));
    assert!(!mailrs_imap_client::is_authentication_failure(
        "a001 NO [SERVERBUG] Internal error occurred"
    ));
}

/// Nothing here may panic on what a server actually sends, because a
/// crash in the sync worker takes every account down with it.
#[test]
fn nonsense_is_ignored_rather_than_panicking() {
    for line in [
        "",
        "*",
        "* ",
        "* FETCH",
        "* 12 FETCH (",
        "* OK [UIDVALIDITY not-a-number] .",
        "* LIST",
        "* LIST () \"/\"",
        "* 99999999999999999999999 EXISTS",
        "\u{1F600} unicode",
    ] {
        let _ = parse_line(line);
    }
}
