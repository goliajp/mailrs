//! A user's labels.

use serde::{Deserialize, Serialize};

pub use mailrs_mailbox::labels::{LABEL_COLORS, Label, LabelError, label_color, label_name};

/// `GET` (list, by name) and `POST` (create) `/v1/users/{user}/labels`.
pub const PATH_LABELS: &str = "/v1/users/{user}/labels";
/// `POST` (rename or recolour) and `DELETE` one label.
pub const PATH_LABEL: &str = "/v1/users/{user}/labels/{label_id}";
/// `POST` puts the label on the conversation, `DELETE` takes it off.
pub const PATH_THREAD_LABEL: &str = "/v1/users/{user}/threads/{thread_id}/labels/{label_id}";

/// `POST /v1/users/{user}/labels`. 409 when the name is taken, 400 when
/// the name or colour is not allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLabelRequest {
    pub name: String,
    pub color: String,
}

/// `POST /v1/users/{user}/labels/{label_id}`: what changes; absent stays.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateLabelRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelsResponse {
    pub items: Vec<Label>,
}
