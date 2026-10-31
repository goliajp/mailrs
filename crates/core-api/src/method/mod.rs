//! RPC method definitions, grouped by domain.
//!
//! Each submodule defines HTTP path constants (`pub const PATH_*`) and
//! the request and response bodies. The path prefix is `/v{API_VERSION}`
//! from the crate root.
//!
//! What the client asks a core for:
//!
//! ```text
//!   /v1/healthz, /v1/readyz                       # health
//!
//!   /v1/users/{u}/conversations:list              # conversation lists
//!   /v1/users/{u}/conversations:search
//!   /v1/users/{u}/conversations:by-thread-ids
//!   /v1/users/{u}/conversations:mark-all-read
//!   /v1/users/{u}/conversations:mark-list-read
//!   /v1/users/{u}/conversations/categories
//!   /v1/users/{u}/conversations/unseen-count
//!
//!   /v1/users/{u}/threads/{tid}                   # one conversation
//!   /v1/users/{u}/threads/{tid}/messages
//!   /v1/users/{u}/threads/{tid}/{action}
//!   /v1/users/{u}/sent                            # file a sent copy
//!   /v1/users/{u}/sent-messages
//!   /v1/users/{u}/quarantine
//!
//!   /v1/users/{u}/mailboxes                       # messages
//!   /v1/users/{u}/messages/by-uid/{uid}
//!   /v1/users/{u}/messages/by-message-id/{mid}
//!   /v1/users/{u}/messages/{uid}/flags
//!   /v1/users/{u}/messages/{mid}/fraud-verdict
//!
//!   /v1/users/{u}/drafts, signatures, templates   # side state
//!
//!   /v1/admin/accounts                            # directory
//!   /v1/admin/aliases:local
//!   /v1/admin/domains
//!   /v1/admin/recipients/{address}/deliverable
//!   /v1/admin/webhook-subscriptions
//! ```
//!
//! fastcore also serves operator routes no client asks for
//! (`/v1/admin/maintenance:*` and a few one-shot repairs); they are listed
//! with their purpose in `scripts/core-routes-unasked-allow.txt`.

pub mod admin;
pub mod contact;
pub mod conversation;
pub mod health;
pub mod label;
pub mod mailbox;
pub mod message;
pub mod outbound;
pub mod sent;
pub mod thread;
