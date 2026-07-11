//! RPC method definitions, grouped by domain.
//!
//! Each submodule defines:
//! - HTTP path constants (`pub const PATH_*`)
//! - Request body types (`*Request`)
//! - Response body types (`*Response`)
//! - A `methods!` enumeration the client / server code can iterate
//!
//! The path prefix is `/v{API_VERSION}` from the crate root. See
//! `docs/CURRENT_STATE_FROZEN.md` §0.4 + §0.5 for the source surface.
//!
//! Path skeleton:
//!
//! ```text
//!   /v1/healthz                                   # health
//!   /v1/readyz                                    # readiness
//!   /v1/metrics                                   # prom (separate, no auth)
//!
//!   /v1/users/{u}/mailboxes                       # mailbox CRUD
//!   /v1/mailboxes/{id}                            #   ...
//!   /v1/mailboxes/{id}/status
//!   /v1/mailboxes/{id}/messages
//!   /v1/mailboxes/{id}/messages/uid/{uid}
//!   /v1/mailboxes/{id}/messages/{uid}/flags
//!   /v1/mailboxes/{id}/expunge
//!   /v1/mailboxes/{id}/changed-since/{modseq}
//!
//!   /v1/messages/{id}                             # message read
//!   /v1/messages/{id}/raw
//!   /v1/messages/{id}/attachments/{idx}
//!   /v1/messages/{id}/attachments/{idx}/text
//!
//!   /v1/users/{u}/threads/{tid}/messages          # thread read
//!   /v1/users/{u}/threads/{tid}/refs
//!   /v1/users/{u}/threads/{tid}/{action}          # thread mutate
//!
//!   /v1/users/{u}/conversations:list              # conversation aggregates
//!   /v1/users/{u}/conversations:search
//!   /v1/users/{u}/conversations/categories
//!   /v1/users/{u}/conversations/unseen-count
//!
//!   /v1/analysis/{message_id}                     # email_analysis
//!   /v1/search/semantic
//!
//!   /v1/contacts/{user}/score                     # contacts
//!   /v1/contacts/{user}/feedback
//!
//!   /v1/users/{u}/drafts                          # drafts
//!   /v1/users/{u}/signatures                      # signatures
//!   /v1/users/{u}/templates                       # templates
//!
//!   /v1/invites/{message_id}                      # iTIP invite handling
//!   /v1/threads/{tid}/reactions                   # reactions
//!
//!   /v1/admin/accounts                            # domain_store
//!   /v1/admin/aliases
//!   /v1/admin/apps
//!   /v1/admin/groups
//!   /v1/admin/email-groups
//!   /v1/admin/domains
//!   /v1/admin/sieve
//!   /v1/admin/totp
//!   /v1/admin/oauth-clients
//!   /v1/admin/oauth-signing-keys
//!   /v1/admin/system-config
//!   /v1/admin/audit-log
//!   /v1/admin/api-keys
//!   /v1/admin/webhooks
//!   /v1/admin/encryption-keys
//!   /v1/admin/reconcile
//!   /v1/admin/backfill-threading
//!   /v1/admin/export
//!   /v1/admin/vacation-dedup
//!   /v1/admin/effective-permissions/{address}
//!   /v1/admin/tls-rpt-events
//!
//!   /v1/outbound/claim                            # sender ↔ core
//!   /v1/outbound/recover-stale
//!   /v1/outbound/stats
//!   /v1/outbound/{id}/{state}
//!   /v1/outbound/suppression
//!   /v1/outbound/enqueue
//! ```

pub mod health;
pub mod mailbox;
// Placeholder modules — to be implemented in subsequent loop iterations
// as Phase 1 deliverables fill in (checklist 1.5–1.7).
pub mod admin;
pub mod analysis;
pub mod contact;
pub mod conversation;
pub mod message;
pub mod outbound;
pub mod thread;
