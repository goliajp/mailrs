//! The threading rule against a thread index held in memory.

use std::collections::HashMap;

use crate::arrival::{Threads, thread_root};

/// One conversation, `q1@x` under "Quarterly report".
struct One(HashMap<&'static str, &'static str>);

impl Threads for One {
    fn thread_for_message_id(&self, _user: &str, mid: &str) -> Option<String> {
        self.0.get(mid).map(|t| t.to_string())
    }
    fn thread_subject(&self, _user: &str, tid: &str) -> Option<String> {
        (tid == "q1@x").then(|| "Quarterly report".to_string())
    }
}

fn root(irt: &str, refs: &[&str], subject: &str) -> String {
    let threads = One(HashMap::from([("q1@x", "q1@x")]));
    let refs: Vec<String> = refs.iter().map(|r| r.to_string()).collect();
    thread_root(&threads, "u@y", "new@x", irt, &refs, subject)
}

#[test]
fn a_reply_on_the_same_subject_joins_its_ancestor() {
    assert_eq!(root("q1@x", &[], "Re: Quarterly report"), "q1@x");
    assert_eq!(
        root("q9@x", &["q1@x", "q9@x"], "RE: quarterly report"),
        "q1@x"
    );
}

/// The root message's id is what In-Reply-To cites, so falling back to it
/// after the subject rule turned the ancestor down put the new topic
/// straight back into the old conversation.
#[test]
fn a_reply_that_changes_the_subject_starts_its_own_conversation() {
    assert_eq!(root("q1@x", &[], "Annual closing"), "new@x");
    assert_eq!(root("", &["q1@x"], "Annual closing"), "new@x");
}

#[test]
fn with_no_known_ancestor_the_headers_name_the_root() {
    assert_eq!(root("q7@x", &["q5@x", "q7@x"], "anything"), "q5@x");
    assert_eq!(root("q7@x", &[], "anything"), "q7@x");
    assert_eq!(root("", &[], "anything"), "new@x");
}
