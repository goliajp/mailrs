//! Params for the label tools (batch 16).

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateLabelParams {
    /// 1 to 64 characters, no `/`, not a mailbox name (INBOX, Trash, Junk,
    /// Sent, Drafts, Archive), unique for this user ignoring case.
    pub name: String,
    /// One of gray, red, orange, yellow, green, teal, blue, purple.
    #[serde(default = "gray")]
    pub color: String,
}

fn gray() -> String {
    "gray".into()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateLabelParams {
    /// The label's id, from list_labels.
    pub label_id: String,
    /// A new name; absent keeps it.
    #[serde(default)]
    pub name: Option<String>,
    /// A new colour; absent keeps it.
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LabelIdParams {
    /// The label's id, from list_labels.
    pub label_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThreadLabelParams {
    /// The conversation.
    pub thread_id: String,
    /// The label's id, from list_labels.
    pub label_id: String,
}
