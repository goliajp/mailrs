//! Which folders an import reads, as what, and in what order.
//!
//! RFC 6154 attributes decide where a server gives them; the usual names
//! decide where it does not. The Inbox goes first and Sent second, so that
//! a message also listed under All Mail or a label is filed where it
//! belongs the first time and skipped as a duplicate after.

use super::client::Folder;
use crate::handlers::mbox::format::Folder as Dest;

const SKIPPED_ATTRS: [&str; 7] = [
    "\\noselect",
    "\\nonexistent",
    "\\junk",
    "\\trash",
    "\\drafts",
    // views onto mail filed elsewhere
    "\\flagged",
    "\\important",
];

const JUNK_NAMES: [&str; 5] = ["junk", "spam", "junk e-mail", "junk email", "bulk mail"];
const TRASH_NAMES: [&str; 4] = ["trash", "deleted items", "deleted messages", "bin"];
const SENT_NAMES: [&str; 4] = ["sent", "sent items", "sent messages", "sent mail"];

/// The last segment of a folder name, whatever the hierarchy delimiter.
fn leaf(name: &str) -> String {
    name.rsplit(['/', '.'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase()
}

/// The folders to read, in order, each with where its mail is filed.
pub fn plan(folders: Vec<Folder>) -> Vec<(String, Dest)> {
    let mut inbox = Vec::new();
    let mut sent = Vec::new();
    let mut rest = Vec::new();
    for f in folders {
        let attrs: Vec<String> = f.flags.iter().map(|a| a.to_ascii_lowercase()).collect();
        let has = |a: &str| attrs.iter().any(|x| x == a);
        if SKIPPED_ATTRS.iter().any(|a| has(a)) {
            continue;
        }
        let leaf = leaf(&f.name);
        let named = |names: &[&str]| names.contains(&leaf.as_str());
        if named(&JUNK_NAMES) || named(&TRASH_NAMES) || leaf == "drafts" {
            continue;
        }
        if f.name.eq_ignore_ascii_case("INBOX") {
            inbox.push((f.name, Dest::Inbox));
        } else if has("\\sent") || named(&SENT_NAMES) {
            sent.push((f.name, Dest::Sent));
        } else {
            rest.push((f.name, Dest::Archive));
        }
    }
    rest.sort_by(|a, b| a.0.cmp(&b.0));
    inbox.into_iter().chain(sent).chain(rest).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(name: &str, flags: &[&str]) -> Folder {
        Folder {
            name: name.into(),
            flags: flags.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn gmail_reads_inbox_then_sent_then_all_mail() {
        let got = plan(vec![
            f("[Gmail]/All Mail", &["\\HasNoChildren", "\\All"]),
            f("[Gmail]", &["\\Noselect"]),
            f("[Gmail]/Spam", &["\\Junk"]),
            f("[Gmail]/Trash", &["\\Trash"]),
            f("[Gmail]/Drafts", &["\\Drafts"]),
            f("[Gmail]/Starred", &["\\Flagged"]),
            f("[Gmail]/Important", &["\\Important"]),
            f("[Gmail]/Sent Mail", &["\\Sent"]),
            f("Receipts", &[]),
            f("INBOX", &[]),
        ]);
        assert_eq!(
            got,
            vec![
                ("INBOX".to_string(), Dest::Inbox),
                ("[Gmail]/Sent Mail".to_string(), Dest::Sent),
                ("Receipts".to_string(), Dest::Archive),
                ("[Gmail]/All Mail".to_string(), Dest::Archive),
            ]
        );
    }

    /// A server without RFC 6154 attributes, folders by their usual names.
    #[test]
    fn without_attributes_the_usual_names_decide() {
        let got = plan(vec![
            f("INBOX.Sent Items", &[]),
            f("INBOX.Junk E-mail", &[]),
            f("INBOX.Deleted Items", &[]),
            f("INBOX.Drafts", &[]),
            f("INBOX.Projects", &[]),
            f("inbox", &[]),
        ]);
        assert_eq!(
            got,
            vec![
                ("inbox".to_string(), Dest::Inbox),
                ("INBOX.Sent Items".to_string(), Dest::Sent),
                ("INBOX.Projects".to_string(), Dest::Archive),
            ]
        );
    }
}
