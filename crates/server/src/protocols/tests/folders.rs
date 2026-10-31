//! What a client asks about folders without opening them: STATUS, LSUB.

use mailrs_imap_proto::special_use_flag;

use super::Step;
use super::steps::{logout, signed_in, step};

// the account's label is listed after the mailboxes
const FOLDERS: [&str; 6] = [
    "INBOX",
    "Drafts",
    "Junk",
    "Sent",
    "Trash",
    super::harness::LABEL,
];

/// INBOX holds two messages, the first of them `\Seen`.
pub(super) fn status() -> Vec<Step> {
    let lsub: Vec<String> = FOLDERS
        .iter()
        .map(|n| {
            format!(
                "* LSUB (\\HasNoChildren{}) \"/\" {n}\r\n",
                special_use_flag(n)
            )
        })
        .chain(["f7 OK LSUB completed\r\n".to_string()])
        .collect();
    let mut steps = signed_in("f1");
    steps.extend([
        step(
            "f2 STATUS INBOX (MESSAGES RECENT UIDNEXT UIDVALIDITY UNSEEN HIGHESTMODSEQ)\r\n",
            &[
                "* STATUS INBOX (MESSAGES 2 RECENT 2 UIDNEXT 4 UIDVALIDITY * UNSEEN 1 HIGHESTMODSEQ *)\r\n",
                "f2 OK STATUS completed\r\n",
            ],
        ),
        step(
            "f3 STATUS Sent (MESSAGES UNSEEN)\r\n",
            &[
                "* STATUS Sent (MESSAGES 0 UNSEEN 0)\r\n",
                "f3 OK STATUS completed\r\n",
            ],
        ),
        step(
            "f4 STATUS \"inbox\" (MESSAGES)\r\n",
            &["* STATUS INBOX (MESSAGES 2)\r\n", "f4 OK STATUS completed\r\n"],
        ),
        step(
            "f5 STATUS Nowhere (MESSAGES)\r\n",
            &["f5 NO [NONEXISTENT] no such mailbox\r\n"],
        ),
        step(
            "f6 STATUS INBOX (MESSAGES SIZES)\r\n",
            &["f6 BAD unknown STATUS item SIZES\r\n"],
        ),
        step(
            "f7 LSUB \"\" \"*\"\r\n",
            &lsub.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        logout("f8"),
    ]);
    steps
}

/// The folder set cannot change; subscribing to a folder that exists
/// succeeds and changes nothing.
pub(super) fn fixed() -> Vec<Step> {
    let cannot = "NO [CANNOT] folders cannot be created, deleted or renamed\r\n";
    let list: Vec<String> = FOLDERS
        .iter()
        .map(|n| {
            format!(
                "* LIST (\\HasNoChildren{}) \"/\" {n}\r\n",
                special_use_flag(n)
            )
        })
        .chain(["g8 OK LIST completed\r\n".to_string()])
        .collect();
    let mut steps = signed_in("g1");
    steps.extend([
        step("g2 CREATE Projects\r\n", &[&format!("g2 {cannot}")]),
        step("g3 DELETE Sent\r\n", &[&format!("g3 {cannot}")]),
        step("g4 RENAME Sent Outbox\r\n", &[&format!("g4 {cannot}")]),
        step("g5 SUBSCRIBE Sent\r\n", &["g5 OK SUBSCRIBE completed\r\n"]),
        step(
            "g6 UNSUBSCRIBE \"Trash\"\r\n",
            &["g6 OK UNSUBSCRIBE completed\r\n"],
        ),
        step(
            "g7 SUBSCRIBE Nowhere\r\n",
            &["g7 NO [NONEXISTENT] no such mailbox\r\n"],
        ),
        step(
            "g8 LIST \"\" \"*\"\r\n",
            &list.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        logout("g9"),
    ]);
    steps
}
