//! In-memory [`MailStore`] implementation suitable
//! for tests, examples, and downstream-consumer test harnesses.
//!
//! **Intended use is testing.** The store keeps every value in process memory
//! and never persists across restarts; do not wire it into a real deployment.
//!
//! ## Quick start
//!
//! ```
//! use mailrs_jmap::fixtures::{InMemoryStore, EXAMPLE_USER, make_message};
//!
//! let store = InMemoryStore::new()
//!     .with_message(make_message(1, 10, EXAMPLE_USER));
//! ```
//!
//! ## What it gives you
//!
//! - Stateful in-memory storage with a builder API — `with_mailbox`,
//!   `with_message`, `with_message_raw`, `with_parsed_body`,
//!   `with_mailbox_counts`.
//! - Per-method error injection via `<method>_fails` setters so each error
//!   path in your handler-driving code can be isolated in a single test.
//! - Read-back helpers for assertions — `flags_for(mailbox_id, uid)`.
//! - Convenience constructors — `make_message`, `make_request`,
//!   `parsed_with_text`, `parsed_with_attachment`.
//!
//! Used internally by this crate's integration tests; the same module is
//! exposed to downstream consumers so they can drive their own handler /
//! dispatcher tests without re-implementing the store.

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use serde_json::Value;

use crate::dispatch::JmapRequest;
use crate::store::{MailStore, StoreError};
use crate::types::{
    Attachment, FLAG_SEEN, Mailbox, MailboxCounts, Message, ParsedBody, SubmissionResult,
};

/// Convenience example user used by the constructors in this module. The
/// store does not assume any particular value — callers can use their own.
pub const EXAMPLE_USER: &str = "alice@example.com";

/// In-memory [`MailStore`] backed by `Vec`s under an `RwLock`.
///
/// Build via [`Self::new`] + the chainable `with_*` setters. See the module
/// docs for an example.
pub struct InMemoryStore {
    inner: RwLock<Inner>,
}

struct Inner {
    mailboxes: Vec<Mailbox>,
    messages: Vec<Message>,
    raw_bytes: HashMap<i64, Vec<u8>>,
    parsed_bodies: HashMap<Vec<u8>, ParsedBody>,
    mailbox_counts: HashMap<i64, MailboxCounts>,

    list_mailboxes_error: Option<String>,
    mailbox_status_error: Option<String>,
    list_messages_error: Option<String>,
    get_message_error: Option<String>,
    list_thread_messages_error: Option<String>,
    update_flags_error: Option<String>,
    add_flags_error: Option<String>,

    submission_result: SubmissionResult,
}

impl InMemoryStore {
    /// Construct an empty store.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                mailboxes: Vec::new(),
                messages: Vec::new(),
                raw_bytes: HashMap::new(),
                parsed_bodies: HashMap::new(),
                mailbox_counts: HashMap::new(),
                list_mailboxes_error: None,
                mailbox_status_error: None,
                list_messages_error: None,
                get_message_error: None,
                list_thread_messages_error: None,
                update_flags_error: None,
                add_flags_error: None,
                submission_result: SubmissionResult {
                    success: true,
                    message: None,
                },
            }),
        }
    }

    /// Append a mailbox with the given id and name.
    pub fn with_mailbox(self, id: i64, name: &str) -> Self {
        self.inner.write().unwrap().mailboxes.push(Mailbox {
            id,
            name: name.to_string(),
        });
        self
    }

    /// Append a pre-built [`Message`]. Use [`make_message`] for a sane default
    /// shape and mutate before passing in if you need overrides.
    pub fn with_message(self, msg: Message) -> Self {
        self.inner.write().unwrap().messages.push(msg);
        self
    }

    /// Map a message id to raw RFC 5322 bytes. [`MailStore::read_message_raw`]
    /// returns these bytes; without this setter that method returns `None`.
    pub fn with_message_raw(self, msg_id: i64, raw: Vec<u8>) -> Self {
        self.inner.write().unwrap().raw_bytes.insert(msg_id, raw);
        self
    }

    /// Map a specific raw byte sequence to a parsed body. The store's
    /// [`MailStore::parse_message`] returns the override when called with
    /// matching bytes, and `ParsedBody::default()` otherwise.
    pub fn with_parsed_body(self, raw: Vec<u8>, parsed: ParsedBody) -> Self {
        self.inner
            .write()
            .unwrap()
            .parsed_bodies
            .insert(raw, parsed);
        self
    }

    /// Override mailbox counts for a specific mailbox id. Without this the
    /// store derives total/unread from the message list.
    pub fn with_mailbox_counts(self, mb_id: i64, counts: MailboxCounts) -> Self {
        self.inner
            .write()
            .unwrap()
            .mailbox_counts
            .insert(mb_id, counts);
        self
    }

    /// Make [`MailStore::list_mailboxes`] return an error carrying `msg`.
    pub fn list_mailboxes_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_mailboxes_error = Some(msg.to_string());
        self
    }

    /// Make [`MailStore::mailbox_status`] return an error carrying `msg`.
    pub fn mailbox_status_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().mailbox_status_error = Some(msg.to_string());
        self
    }

    /// Make [`MailStore::list_messages`] return an error carrying `msg`.
    pub fn list_messages_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_messages_error = Some(msg.to_string());
        self
    }

    /// Make [`MailStore::get_message_by_db_id`] return an error carrying `msg`.
    pub fn get_message_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().get_message_error = Some(msg.to_string());
        self
    }

    /// Make [`MailStore::list_thread_messages`] return an error carrying `msg`.
    pub fn list_thread_messages_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().list_thread_messages_error = Some(msg.to_string());
        self
    }

    /// Make [`MailStore::update_flags`] return an error carrying `msg`.
    pub fn update_flags_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().update_flags_error = Some(msg.to_string());
        self
    }

    /// Make [`MailStore::add_flags`] return an error carrying `msg`.
    pub fn add_flags_fails(self, msg: &str) -> Self {
        self.inner.write().unwrap().add_flags_error = Some(msg.to_string());
        self
    }

    /// Configure [`MailStore::submit_message`] to fail with a description.
    pub fn submission_fails_with(self, msg: &str) -> Self {
        self.inner.write().unwrap().submission_result = SubmissionResult {
            success: false,
            message: Some(msg.to_string()),
        };
        self
    }

    /// Configure [`MailStore::submit_message`] to fail without a description.
    pub fn submission_fails_silently(self) -> Self {
        self.inner.write().unwrap().submission_result = SubmissionResult {
            success: false,
            message: None,
        };
        self
    }

    /// Read back the current flag bitmask for `(mailbox_id, uid)`. `None`
    /// when the row is missing. Tests use this to assert the effect of
    /// `Email/set` updates and destroys.
    pub fn flags_for(&self, mailbox_id: i64, uid: u32) -> Option<u32> {
        self.inner
            .read()
            .unwrap()
            .messages
            .iter()
            .find(|m| m.mailbox_id == mailbox_id && m.uid == uid)
            .map(|m| m.flags)
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MailStore for InMemoryStore {
    async fn list_mailboxes(&self, _user: &str) -> Result<Vec<Mailbox>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_mailboxes_error {
            return Err(msg.clone().into());
        }
        Ok(inner.mailboxes.clone())
    }

    async fn mailbox_status(&self, mailbox_id: i64) -> Result<MailboxCounts, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.mailbox_status_error {
            return Err(msg.clone().into());
        }
        if let Some(counts) = inner.mailbox_counts.get(&mailbox_id) {
            return Ok(*counts);
        }
        // Default: total = messages in mailbox, unread = those without FLAG_SEEN.
        let total = inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id)
            .count() as u32;
        let unread = inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id && m.flags & FLAG_SEEN == 0)
            .count() as u32;
        Ok(MailboxCounts { total, unread })
    }

    async fn list_messages(
        &self,
        mailbox_id: i64,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_messages_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .messages
            .iter()
            .filter(|m| m.mailbox_id == mailbox_id)
            .skip(offset as usize)
            .take(limit as usize)
            .cloned()
            .collect())
    }

    async fn get_message_by_db_id(
        &self,
        user: &str,
        id: i64,
    ) -> Result<Option<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.get_message_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .messages
            .iter()
            .find(|m| m.id == id && m.user_address == user)
            .cloned())
    }

    async fn list_thread_messages(
        &self,
        user: &str,
        thread_id: &str,
    ) -> Result<Vec<Message>, StoreError> {
        let inner = self.inner.read().unwrap();
        if let Some(ref msg) = inner.list_thread_messages_error {
            return Err(msg.clone().into());
        }
        Ok(inner
            .messages
            .iter()
            .filter(|m| m.thread_id == thread_id && m.user_address == user)
            .cloned()
            .collect())
    }

    async fn update_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.update_flags_error {
            return Err(msg.clone().into());
        }
        if let Some(m) = inner
            .messages
            .iter_mut()
            .find(|m| m.mailbox_id == mailbox_id && m.uid == uid)
        {
            m.flags = flags;
        }
        Ok(())
    }

    async fn add_flags(&self, mailbox_id: i64, uid: u32, flags: u32) -> Result<(), StoreError> {
        let mut inner = self.inner.write().unwrap();
        if let Some(ref msg) = inner.add_flags_error {
            return Err(msg.clone().into());
        }
        if let Some(m) = inner
            .messages
            .iter_mut()
            .find(|m| m.mailbox_id == mailbox_id && m.uid == uid)
        {
            m.flags |= flags;
        }
        Ok(())
    }

    async fn read_message_raw(&self, message: &Message) -> Option<Vec<u8>> {
        self.inner
            .read()
            .unwrap()
            .raw_bytes
            .get(&message.id)
            .cloned()
    }

    fn parse_message(&self, raw: &[u8]) -> ParsedBody {
        self.inner
            .read()
            .unwrap()
            .parsed_bodies
            .get(raw)
            .cloned()
            .unwrap_or_default()
    }

    async fn submit_message(
        &self,
        _user: &str,
        _message: &Message,
        _raw: &[u8],
    ) -> SubmissionResult {
        self.inner.read().unwrap().submission_result.clone()
    }
}

/// Build a [`Message`] with sane defaults. Tests override only the fields
/// they care about by mutating the returned value before handing it to
/// [`InMemoryStore::with_message`].
pub fn make_message(id: i64, mailbox_id: i64, user: &str) -> Message {
    Message {
        id,
        mailbox_id,
        uid: id as u32,
        sender: "Sender <sender@example.com>".to_string(),
        recipients: user.to_string(),
        subject: format!("message {id}"),
        date: 1_700_000_000 + id,
        size: 256,
        flags: 0,
        internal_date: 1_700_000_000 + id,
        message_id: format!("msg-{id}@example.com"),
        in_reply_to: String::new(),
        thread_id: format!("thread-{id}"),
        user_address: user.to_string(),
        new_content: Some(format!("snippet {id}")),
        blob_id: format!("blob-{id}"),
    }
}

/// Assemble a [`JmapRequest`] from a slice of `(method, args, call_id)`
/// tuples. Always declares the mail capability.
pub fn make_request(calls: &[(&str, Value, &str)]) -> JmapRequest {
    JmapRequest {
        using: vec!["urn:ietf:params:jmap:mail".to_string()],
        method_calls: calls
            .iter()
            .map(|(m, a, c)| (m.to_string(), a.clone(), c.to_string()))
            .collect(),
    }
}

/// Build a [`ParsedBody`] containing only a plain-text part.
pub fn parsed_with_text(text: &str) -> ParsedBody {
    ParsedBody {
        text: Some(text.to_string()),
        html: None,
        attachments: vec![],
    }
}

/// Build a [`ParsedBody`] containing one attachment of the given size and no
/// body parts.
pub fn parsed_with_attachment(filename: &str, content_type: &str, size: u32) -> ParsedBody {
    ParsedBody {
        text: None,
        html: None,
        attachments: vec![Attachment {
            filename: filename.to_string(),
            content_type: content_type.to_string(),
            size,
        }],
    }
}
