//! Labels: a user's own names for conversations, shared by both cores so a
//! name one accepts the other accepts too.

use serde::{Deserialize, Serialize};

/// One user's label. `id` never changes; a rename keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    /// Assigned at creation, per user.
    pub id: String,
    /// As shown, trimmed; unique per user ignoring case.
    pub name: String,
    /// One of [`LABEL_COLORS`].
    pub color: String,
    /// Unix seconds.
    pub created_at: i64,
}

/// The colours a label can have, as names the clients map to their palette.
pub const LABEL_COLORS: [&str; 8] = [
    "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple",
];

/// Names a label cannot take: IMAP shows labels as mailboxes, and these
/// are mailboxes already.
const RESERVED: [&str; 6] = ["INBOX", "Trash", "Junk", "Sent", "Drafts", "Archive"];

/// Why a label name or colour was refused, worded for the person who typed it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelError {
    /// Empty or longer than 64 characters.
    #[error("a label name is 1 to 64 characters")]
    Length,
    /// IMAP separates mailbox levels with `/`.
    #[error("a label name cannot contain /")]
    Slash,
    /// A mailbox name.
    #[error("{0} is a mailbox name and cannot be a label")]
    Reserved(String),
    /// Another label of the user has the name.
    #[error("there is already a label called {0}")]
    Taken(String),
    /// Not one of [`LABEL_COLORS`].
    #[error("unknown colour {0}")]
    Color(String),
    /// The user has no label with that id.
    #[error("no such label")]
    NotFound,
}

/// `name` as stored: trimmed, and checked against the rules every core
/// applies. Uniqueness is the store's to check.
pub fn label_name(name: &str) -> Result<String, LabelError> {
    let name = name.trim();
    let n = name.chars().count();
    if n == 0 || n > 64 {
        return Err(LabelError::Length);
    }
    if name.contains('/') {
        return Err(LabelError::Slash);
    }
    if let Some(r) = RESERVED.iter().find(|r| r.eq_ignore_ascii_case(name)) {
        return Err(LabelError::Reserved((*r).to_string()));
    }
    Ok(name.to_string())
}

/// `color` when it is one of [`LABEL_COLORS`].
pub fn label_color(color: &str) -> Result<String, LabelError> {
    match LABEL_COLORS.contains(&color) {
        true => Ok(color.to_string()),
        false => Err(LabelError::Color(color.to_string())),
    }
}

/// Whether `name` is taken among `labels`, other than by `except`.
pub fn name_taken(labels: &[Label], name: &str, except: Option<&str>) -> bool {
    labels
        .iter()
        .any(|l| Some(l.id.as_str()) != except && l.name.to_lowercase() == name.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_checked() {
        assert_eq!(label_name("  Work ").unwrap(), "Work");
        assert_eq!(label_name(""), Err(LabelError::Length));
        assert_eq!(label_name(&"x".repeat(65)), Err(LabelError::Length));
        assert_eq!(label_name("a/b"), Err(LabelError::Slash));
        assert_eq!(label_name("inbox"), Err(LabelError::Reserved("INBOX".into())));
        assert!(label_name("待办").is_ok());
    }

    #[test]
    fn a_taken_name_is_found_ignoring_case_and_the_label_itself() {
        let l = Label {
            id: "1".into(),
            name: "Work".into(),
            color: "blue".into(),
            created_at: 0,
        };
        assert!(name_taken(std::slice::from_ref(&l), "work", None));
        assert!(!name_taken(std::slice::from_ref(&l), "work", Some("1")));
    }
}
