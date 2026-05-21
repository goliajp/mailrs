//! Mail REST handlers, split by feature group.
//!
//! Each sub-module owns a small thematic set of handlers (folders, messages,
//! send, attachments, drafts, signatures, keys, preview, spam, bimi, proxy)
//! plus its own request / response DTOs. Cross-cutting helpers
//! (`verify_sender`, `resolve_thread_reply`, `build_rfc5322_*`,
//! `deliver_message*`, `extract_address`, `wrap_email_html`) live in
//! `common.rs` so all sub-modules can `use super::common::*` to pick them up.
//!
//! `web/mod.rs` keeps using handlers as `mail::send_message`, `mail::get_folders`,
//! etc. — the `pub(super) use {folders::*, messages::*, ...}` block below makes
//! every handler visible at the `mail::` path with no call-site changes.
//!
//! External call sites (e.g. `mcp/mod.rs`, `web/rsvp.rs`, `web/auth.rs`,
//! `web/jmap.rs`) refer to a few helpers via `crate::web::mail::deliver_message`,
//! `crate::web::mail::build_rfc5322_with_attachments`, etc. These remain
//! reachable at the `mail::` path because `common.rs` exports them as
//! `pub(crate)` and `mod.rs` re-exports `common::*` for the same callers.

pub(super) use super::{
    classify_email, clamp_limit, clamp_offset, default_limit, ApiResult, AuthUser, SendResult,
    WebState, MAX_ADMIN_FIELD_LEN, MAX_EMAIL_BODY_LEN, MAX_PATH_LEN, MAX_RECIPIENTS,
};

pub mod attachments;
pub mod bimi;
pub mod common;
pub mod drafts;
pub mod folders;
pub mod keys;
pub mod messages;
pub mod preview;
pub mod proxy;
pub mod send;
pub mod signatures;
pub mod spam;

pub(super) use attachments::*;
pub(super) use bimi::*;
pub(crate) use common::*;
pub(super) use drafts::*;
pub(super) use folders::*;
pub(super) use keys::*;
pub(super) use messages::*;
pub(super) use preview::*;
pub(super) use proxy::*;
pub(super) use send::*;
pub(super) use signatures::*;
pub(super) use spam::*;
