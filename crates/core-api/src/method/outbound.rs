//! The outbound queue row every enqueuer writes and the sender reads.

use serde::{Deserialize, Serialize};

/// One outbound queue row, as the job blob on the network kevy holds it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessageWire {
    /// Store-native primary key.
    pub id: i64,
    /// Envelope sender (SMTP MAIL FROM).
    pub sender: String,
    /// Envelope recipient.
    pub recipient: String,
    /// Optional original sender pre-SRS (for forwarded mail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_sender: Option<String>,
    /// Raw message bytes, base64-encoded.
    pub message_data_base64: String,
    /// Current status (see `QueueStatus`).
    pub status: QueueStatus,
    /// Attempts so far.
    pub attempts: u32,
    /// Last delivery error, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Epoch seconds of next retry attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_retry: Option<i64>,
    /// Epoch seconds when scheduled to send (None = immediately).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_at: Option<i64>,
    /// Epoch seconds when row was created.
    pub created_at: i64,
    /// Epoch seconds when row was last updated.
    pub updated_at: i64,
    /// The send this job belongs to — one send fans out to one job per
    /// recipient, and the sender needs the group to report a per-
    /// recipient outcome against it.
    ///
    /// `Option` because rows enqueued before this field existed
    /// deserialize with `None`, and because the tls-rpt and bounce paths
    /// enqueue mail that no user composed and so has no Send row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_id: Option<String>,
}

/// Outbound queue status enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    Pending,
    Inflight,
    Delivered,
    Failed,
    Bounced,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_status_lowercase() {
        let s = serde_json::to_string(&QueueStatus::Pending).unwrap();
        assert_eq!(s, "\"pending\"");
        let back: QueueStatus = serde_json::from_str("\"delivered\"").unwrap();
        assert_eq!(back, QueueStatus::Delivered);
    }
}
