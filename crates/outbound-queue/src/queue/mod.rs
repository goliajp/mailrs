mod suppression;

pub use suppression::is_hard_bounce;

/// Lifecycle status of a queued message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueStatus {
    /// Awaiting first delivery attempt.
    Pending,
    /// Currently being delivered by a worker.
    InFlight,
    /// Successfully accepted by the remote MX.
    Delivered,
    /// Last attempt failed; will retry per backoff schedule.
    Failed,
    /// Permanent failure; will NOT retry. A DSN has been queued back.
    Bounced,
}

impl QueueStatus {
    /// Lower-snake-case rendering for SQL persistence.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InFlight => "inflight",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
            Self::Bounced => "bounced",
        }
    }

    /// Inverse of [`Self::as_str`]; `None` on unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "inflight" => Some(Self::InFlight),
            "delivered" => Some(Self::Delivered),
            "failed" => Some(Self::Failed),
            "bounced" => Some(Self::Bounced),
            _ => None,
        }
    }
}

/// One queued outbound message — the full row stored in the outbound queue.
#[derive(Debug, Clone)]
pub struct QueuedMessage {
    /// Store-native primary key.
    pub id: i64,
    /// Envelope sender (reverse path).
    pub sender: String,
    /// Envelope recipient (single forward path; multi-recipient messages
    /// fan out into one row per recipient).
    pub recipient: String,
    /// Recipient's domain — extracted for MX-grouped batching.
    pub domain: String,
    /// Full RFC 5322 message body (including headers).
    pub message_data: Vec<u8>,
    /// Current lifecycle status.
    pub status: QueueStatus,
    /// Number of delivery attempts made so far.
    pub attempts: u32,
    /// Cap after which `Failed` flips to `Bounced`.
    pub max_attempts: u32,
    /// Epoch seconds — the earliest time the next attempt is eligible.
    pub next_retry: i64,
    /// Last error response from the remote MX, if any.
    pub last_error: Option<String>,
    /// `Message-ID:` header value, for log correlation.
    pub message_id: Option<String>,
    /// Epoch seconds when the row was first enqueued.
    pub created_at: i64,
    /// Epoch seconds of the most recent update.
    pub updated_at: i64,
    /// `true` when the message came from a forwarding rule rather than a
    /// local sender.
    pub is_forwarded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_status_roundtrip() {
        let variants = [
            QueueStatus::Pending,
            QueueStatus::InFlight,
            QueueStatus::Delivered,
            QueueStatus::Failed,
            QueueStatus::Bounced,
        ];
        for v in &variants {
            let s = v.as_str();
            let parsed = QueueStatus::parse(s).unwrap();
            assert_eq!(&parsed, v, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn queue_status_parse_unknown() {
        assert_eq!(QueueStatus::parse("unknown"), None);
        assert_eq!(QueueStatus::parse(""), None);
        assert_eq!(QueueStatus::parse("PENDING"), None);
    }

    #[test]
    fn queue_status_as_str_values() {
        assert_eq!(QueueStatus::Pending.as_str(), "pending");
        assert_eq!(QueueStatus::InFlight.as_str(), "inflight");
        assert_eq!(QueueStatus::Delivered.as_str(), "delivered");
        assert_eq!(QueueStatus::Failed.as_str(), "failed");
        assert_eq!(QueueStatus::Bounced.as_str(), "bounced");
    }

    #[test]
    fn queue_status_parse_case_sensitive() {
        // parse is case-sensitive — uppercase variants are not valid
        assert_eq!(QueueStatus::parse("Pending"), None);
        assert_eq!(QueueStatus::parse("InFlight"), None);
        assert_eq!(QueueStatus::parse("DELIVERED"), None);
        assert_eq!(QueueStatus::parse("Failed"), None);
        assert_eq!(QueueStatus::parse("Bounced"), None);
    }

    #[test]
    fn queue_status_parse_whitespace_rejected() {
        assert_eq!(QueueStatus::parse(" pending"), None);
        assert_eq!(QueueStatus::parse("pending "), None);
        assert_eq!(QueueStatus::parse("  "), None);
    }

    #[test]
    fn queue_status_eq() {
        assert_eq!(QueueStatus::Pending, QueueStatus::Pending);
        assert_ne!(QueueStatus::Pending, QueueStatus::Delivered);
        assert_ne!(QueueStatus::Failed, QueueStatus::Bounced);
    }

    #[test]
    fn queue_status_clone() {
        let s = QueueStatus::InFlight;
        let c = s.clone();
        assert_eq!(s, c);
    }

    #[test]
    fn queued_message_clone_preserves_fields() {
        let msg = QueuedMessage {
            id: 42,
            sender: "s@example.com".into(),
            recipient: "r@remote.com".into(),
            domain: "remote.com".into(),
            message_data: vec![1, 2, 3],
            status: QueueStatus::Pending,
            attempts: 3,
            max_attempts: 8,
            next_retry: 1_700_000_000,
            last_error: Some("temporary failure".into()),
            message_id: Some("msg-id-123".into()),
            created_at: 1_699_000_000,
            updated_at: 1_699_500_000,
            is_forwarded: true,
        };
        let cloned = msg.clone();
        assert_eq!(cloned.id, 42);
        assert_eq!(cloned.sender, "s@example.com");
        assert_eq!(cloned.recipient, "r@remote.com");
        assert_eq!(cloned.domain, "remote.com");
        assert_eq!(cloned.message_data, vec![1, 2, 3]);
        assert_eq!(cloned.attempts, 3);
        assert_eq!(cloned.max_attempts, 8);
        assert_eq!(cloned.next_retry, 1_700_000_000);
        assert_eq!(cloned.last_error, Some("temporary failure".into()));
        assert_eq!(cloned.message_id, Some("msg-id-123".into()));
        assert!(cloned.is_forwarded);
    }

    #[test]
    fn queued_message_no_last_error() {
        let msg = QueuedMessage {
            id: 1,
            sender: "s@example.com".into(),
            recipient: "r@remote.com".into(),
            domain: "remote.com".into(),
            message_data: vec![],
            status: QueueStatus::Pending,
            attempts: 0,
            max_attempts: 8,
            next_retry: 0,
            last_error: None,
            message_id: None,
            created_at: 0,
            updated_at: 0,
            is_forwarded: false,
        };
        assert!(msg.last_error.is_none());
        assert!(msg.message_id.is_none());
    }
}
