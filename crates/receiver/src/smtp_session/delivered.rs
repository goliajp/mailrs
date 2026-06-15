//! The seam between the receiver's DATA handler and the core's
//! post-delivery consumer (S5.4).
//!
//! The receiver writes the maildir file, builds a plain [`DeliveredMessage`],
//! and hands it off over [`ProcessTx`]. The spg/kevy-bound consumer
//! (`ProcessDeps` + `spawn_process_consumer` + `process_delivered`) lives in
//! the server core — only this context-free value crosses the boundary, so
//! the receiver never touches the stateful deps. This is the seam the
//! cross-process notification consumer (P6) plugs into.

use std::sync::Arc;

/// One message successfully written to maildir, ready for indexing and the
/// post-delivery pass. Built by the DATA handler from the delivery result.
/// Derived values (subject, sender, thread_id, effective message-id) are
/// computed inside the core's `process_delivered`, not carried here.
pub struct DeliveredMessage {
    pub maildir_id: String,
    pub user: String,
    pub rcpt: String,
    pub rcpt_folder: String,
    pub reverse_path: String,
    pub full_message: Arc<Vec<u8>>,
    pub msg_message_id: String,
    pub msg_in_reply_to: String,
    pub msg_size: usize,
}

/// Channel the DATA handler hands delivered messages to, off the hot path.
/// Only the plain [`DeliveredMessage`] crosses — the core-side `ProcessDeps`
/// is owned by the consumer, so the receiver hands off a context-free value
/// and never touches the spg/kevy-bound deps. Capacity 1024; when the
/// channel is full the caller blocks on `send` (backpressure to the single
/// consumer's rate — never a dropped message).
pub type ProcessTx = tokio::sync::mpsc::Sender<DeliveredMessage>;
